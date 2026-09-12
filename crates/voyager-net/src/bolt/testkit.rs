//! Official Neo4j TestKit Backend protocol implementation for `voyager-net`.
//!
//! Provides a bidirectional JSON-over-TCP/IPC server conforming to the Neo4j TestKit specification,
//! enabling official TestKit test runners to execute conformance test suites against `voyager-net`.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use super::connection::BoltConnection;
use super::packstream::BoltValue;
use crate::config::{Auth, ConnectionConfig};
use crate::engine::AsyncConnection;
use crate::error::Result;

/// Unique ID counter for TestKit backend entities.
static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    ID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

/// Active query result stored in TestKit backend session.
struct TestkitResult {
    _keys: Vec<String>,
    records: Vec<Vec<BoltValue>>,
    current_index: usize,
    summary: crate::engine::QuerySummary,
    stream_error: Option<(String, String)>,
    query_text: String,
    query_params: HashMap<String, Value>,
    raw_metadata: Option<HashMap<String, BoltValue>>,
}

/// Active session in TestKit backend.
struct TestkitSession {
    _driver_id: String,
    database: Option<String>,
    bookmarks: Vec<String>,
    last_bookmark: Option<String>,
    conn: Mutex<BoltConnection<TcpStream>>,
}

/// Active driver in TestKit backend.
struct TestkitDriver {
    config: ConnectionConfig,
}

/// Active transaction in TestKit backend.
struct TestkitTx {
    session_id: String,
    failed: bool,
}

/// The TestKit backend state managing drivers, sessions, transactions, and results.
#[derive(Default)]
pub struct TestkitBackend {
    drivers: Mutex<HashMap<String, TestkitDriver>>,
    sessions: Mutex<HashMap<String, TestkitSession>>,
    transactions: Mutex<HashMap<String, TestkitTx>>,
    results: Mutex<HashMap<String, TestkitResult>>,
}

impl TestkitBackend {
    /// Creates a new, empty `TestkitBackend`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Converts a BoltValue to a plain JSON Value (for plan/profile/notifications).
    pub fn bolt_value_to_plain_json(val: &BoltValue) -> Value {
        match val {
            BoltValue::Null => Value::Null,
            BoltValue::Boolean(b) => json!(b),
            BoltValue::Integer(i) => json!(i),
            BoltValue::Float(f) => json!(f),
            BoltValue::String(s) => json!(s),
            BoltValue::Bytes(b) => json!(b),
            BoltValue::List(l) => {
                Value::Array(l.iter().map(Self::bolt_value_to_plain_json).collect())
            }
            BoltValue::Map(m) => {
                let mut map = serde_json::Map::new();
                for (k, v) in m {
                    map.insert(k.clone(), Self::bolt_value_to_plain_json(v));
                }
                Value::Object(map)
            }
            _ => Value::Null,
        }
    }

    /// Converts a BoltValue to a TestKit CypherType JSON object.
    pub fn bolt_value_to_cypher_json(val: &BoltValue) -> Value {
        match val {
            BoltValue::Null => json!({"name": "CypherNull"}),
            BoltValue::Boolean(b) => json!({"name": "CypherBool", "data": {"value": b}}),
            BoltValue::Integer(i) => json!({"name": "CypherInt", "data": {"value": i}}),
            BoltValue::Float(f) => {
                if f.is_nan() {
                    json!({"name": "CypherFloat", "data": {"value": "NaN"}})
                } else if f.is_infinite() {
                    if *f > 0.0 {
                        json!({"name": "CypherFloat", "data": {"value": "+Infinity"}})
                    } else {
                        json!({"name": "CypherFloat", "data": {"value": "-Infinity"}})
                    }
                } else {
                    json!({"name": "CypherFloat", "data": {"value": f}})
                }
            }
            BoltValue::String(s) => json!({"name": "CypherString", "data": {"value": s}}),
            BoltValue::Bytes(b) => {
                let hex_str = b
                    .iter()
                    .map(|byte| format!("{:02x}", byte))
                    .collect::<Vec<String>>()
                    .join(" ");
                json!({
                    "name": "CypherBytes",
                    "data": {
                        "value": hex_str
                    }
                })
            }
            BoltValue::List(l) => {
                let items: Vec<Value> = l.iter().map(Self::bolt_value_to_cypher_json).collect();
                json!({
                    "name": "CypherList",
                    "data": {
                        "value": items
                    }
                })
            }
            BoltValue::Map(m) => {
                let mut map = serde_json::Map::new();
                for (k, v) in m {
                    map.insert(k.clone(), Self::bolt_value_to_cypher_json(v));
                }
                json!({
                    "name": "CypherMap",
                    "data": {
                        "value": map
                    }
                })
            }
            BoltValue::Node(n) => {
                let mut props = serde_json::Map::new();
                for (k, v) in &n.properties {
                    props.insert(k.clone(), Self::bolt_value_to_cypher_json(v));
                }
                let labels: Vec<Value> = n
                    .labels
                    .iter()
                    .map(|l| json!({"name": "CypherString", "data": {"value": l}}))
                    .collect();

                json!({
                    "name": "Node",
                    "data": {
                        "id": json!({"name": "CypherInt", "data": {"value": n.id}}),
                        "labels": json!({"name": "CypherList", "data": {"value": labels}}),
                        "props": json!({"name": "CypherMap", "data": {"value": props}}),
                        "elementId": json!({"name": "CypherString", "data": {"value": n.element_id.as_deref().unwrap_or("")}})
                    }
                })
            }
            BoltValue::Relationship(r) => {
                let mut props = serde_json::Map::new();
                for (k, v) in &r.properties {
                    props.insert(k.clone(), Self::bolt_value_to_cypher_json(v));
                }

                json!({
                    "name": "Relationship",
                    "data": {
                        "id": json!({"name": "CypherInt", "data": {"value": r.id}}),
                        "startNodeId": json!({"name": "CypherInt", "data": {"value": r.start_node_id}}),
                        "endNodeId": json!({"name": "CypherInt", "data": {"value": r.end_node_id}}),
                        "type": json!({"name": "CypherString", "data": {"value": r.rel_type}}),
                        "props": json!({"name": "CypherMap", "data": {"value": props}}),
                        "elementId": json!({"name": "CypherString", "data": {"value": r.element_id.as_deref().unwrap_or("")}}),
                        "startNodeElementId": json!({"name": "CypherString", "data": {"value": r.start_element_id.as_deref().unwrap_or("")}}),
                        "endNodeElementId": json!({"name": "CypherString", "data": {"value": r.end_element_id.as_deref().unwrap_or("")}})
                    }
                })
            }
            BoltValue::Path(p) => {
                let nodes: Vec<Value> = p
                    .nodes
                    .iter()
                    .map(|n| {
                        let mut props = serde_json::Map::new();
                        for (k, v) in &n.properties {
                            props.insert(k.clone(), Self::bolt_value_to_cypher_json(v));
                        }
                        let labels: Vec<Value> = n
                            .labels
                            .iter()
                            .map(|l| json!({"name": "CypherString", "data": {"value": l}}))
                            .collect();

                        json!({
                            "name": "Node",
                            "data": {
                                "id": json!({"name": "CypherInt", "data": {"value": n.id}}),
                                "labels": json!({"name": "CypherList", "data": {"value": labels}}),
                                "props": json!({"name": "CypherMap", "data": {"value": props}}),
                                "elementId": json!({"name": "CypherString", "data": {"value": n.element_id.as_deref().unwrap_or("")}})
                            }
                        })
                    })
                    .collect();

                let mut rels = Vec::new();
                let mut current_node_idx = 0usize;

                for step_idx in 0..(p.sequence.len() / 2) {
                    let rel_code = p.sequence[2 * step_idx];
                    let next_node_idx = p.sequence[2 * step_idx + 1] as usize;
                    let rel_idx = (rel_code.abs() - 1) as usize;

                    if let (Some(prev_node), Some(next_node), Some(unbound_rel)) = (
                        p.nodes.get(current_node_idx),
                        p.nodes.get(next_node_idx),
                        p.relationships.get(rel_idx),
                    ) {
                        let (start_id, end_id, start_eid, end_eid) = if rel_code > 0 {
                            (
                                prev_node.id,
                                next_node.id,
                                prev_node.element_id.as_deref().unwrap_or(""),
                                next_node.element_id.as_deref().unwrap_or(""),
                            )
                        } else {
                            (
                                next_node.id,
                                prev_node.id,
                                next_node.element_id.as_deref().unwrap_or(""),
                                prev_node.element_id.as_deref().unwrap_or(""),
                            )
                        };

                        let mut props = serde_json::Map::new();
                        for (k, v) in &unbound_rel.properties {
                            props.insert(k.clone(), Self::bolt_value_to_cypher_json(v));
                        }

                        rels.push(json!({
                            "name": "Relationship",
                            "data": {
                                "id": json!({"name": "CypherInt", "data": {"value": unbound_rel.id}}),
                                "startNodeId": json!({"name": "CypherInt", "data": {"value": start_id}}),
                                "endNodeId": json!({"name": "CypherInt", "data": {"value": end_id}}),
                                "type": json!({"name": "CypherString", "data": {"value": unbound_rel.rel_type}}),
                                "props": json!({"name": "CypherMap", "data": {"value": props}}),
                                "elementId": json!({"name": "CypherString", "data": {"value": unbound_rel.element_id.as_deref().unwrap_or("")}}),
                                "startNodeElementId": json!({"name": "CypherString", "data": {"value": start_eid}}),
                                "endNodeElementId": json!({"name": "CypherString", "data": {"value": end_eid}})
                            }
                        }));

                        current_node_idx = next_node_idx;
                    }
                }

                json!({
                    "name": "Path",
                    "data": {
                        "nodes": json!({"name": "CypherList", "data": {"value": nodes}}),
                        "relationships": json!({"name": "CypherList", "data": {"value": rels}})
                    }
                })
            }
            BoltValue::Structure { .. } => json!({"name": "CypherNull"}),
        }
    }

    /// Converts a TestKit CypherType JSON object to BoltValue.
    pub fn cypher_json_to_bolt_value(val: &Value) -> BoltValue {
        if let Some(obj) = val.as_object() {
            if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                let data = obj.get("data");
                match name {
                    "CypherNull" => BoltValue::Null,
                    "CypherBool" => {
                        let b = data
                            .and_then(|d| d.get("value"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        BoltValue::Boolean(b)
                    }
                    "CypherInt" => {
                        let i = data
                            .and_then(|d| d.get("value"))
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        BoltValue::Integer(i)
                    }
                    "CypherFloat" => {
                        let f_val = data.and_then(|d| d.get("value"));
                        if let Some(f) = f_val.and_then(|v| v.as_f64()) {
                            BoltValue::Float(f)
                        } else if let Some(s) = f_val.and_then(|v| v.as_str()) {
                            match s {
                                "+Infinity" | "Infinity" | "inf" | "+inf" => {
                                    BoltValue::Float(f64::INFINITY)
                                }
                                "-Infinity" | "-inf" => BoltValue::Float(f64::NEG_INFINITY),
                                "NaN" | "nan" => BoltValue::Float(f64::NAN),
                                _ => BoltValue::Float(s.parse::<f64>().unwrap_or(0.0)),
                            }
                        } else {
                            BoltValue::Float(0.0)
                        }
                    }
                    "CypherBytes" | "CypherByteArray" => {
                        if let Some(s) = data.and_then(|d| d.get("value")).and_then(|v| v.as_str())
                        {
                            let bytes: Vec<u8> = s
                                .split_whitespace()
                                .filter_map(|hex| u8::from_str_radix(hex, 16).ok())
                                .collect();
                            BoltValue::Bytes(bytes)
                        } else {
                            BoltValue::Bytes(Vec::new())
                        }
                    }
                    "CypherString" => {
                        let s = data
                            .and_then(|d| d.get("value"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        BoltValue::String(s.to_string())
                    }
                    "CypherList" => {
                        let list = data
                            .and_then(|d| d.get("value"))
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().map(Self::cypher_json_to_bolt_value).collect())
                            .unwrap_or_default();
                        BoltValue::List(list)
                    }
                    "CypherMap" => {
                        let mut map = HashMap::new();
                        if let Some(obj_val) = data
                            .and_then(|d| d.get("value"))
                            .and_then(|v| v.as_object())
                        {
                            for (k, v) in obj_val {
                                map.insert(k.clone(), Self::cypher_json_to_bolt_value(v));
                            }
                        }
                        BoltValue::Map(map)
                    }
                    _ => BoltValue::from(val),
                }
            } else {
                BoltValue::from(val)
            }
        } else {
            BoltValue::from(val)
        }
    }

    fn make_driver_error(err_str: &str) -> Value {
        let clean = err_str
            .strip_prefix("Protocol error: ")
            .or_else(|| err_str.strip_prefix("Authentication failed: "))
            .or_else(|| err_str.strip_prefix("Connection failed: "))
            .unwrap_or(err_str);
        let (code, msg) = if let Some((c, m)) = clean.split_once(": ") {
            if c.starts_with("Neo.") {
                (c.to_string(), m.to_string())
            } else if clean.contains("Unauthorized") || clean.contains("AuthError") {
                (
                    "Neo.ClientError.Security.Unauthorized".to_string(),
                    clean.to_string(),
                )
            } else {
                (
                    "Neo.ClientError.Statement.SyntaxError".to_string(),
                    clean.to_string(),
                )
            }
        } else if clean.contains("Unauthorized") || clean.contains("AuthError") {
            (
                "Neo.ClientError.Security.Unauthorized".to_string(),
                clean.to_string(),
            )
        } else if clean.contains("SyntaxError") || clean.contains("Invalid input") {
            (
                "Neo.ClientError.Statement.SyntaxError".to_string(),
                clean.to_string(),
            )
        } else if clean.contains("ParameterMissing") || clean.contains("Expected parameter") {
            (
                "Neo.ClientError.Statement.ParameterMissing".to_string(),
                clean.to_string(),
            )
        } else if clean.contains("LockClientStopped") {
            (
                "Neo.ClientError.Transaction.LockClientStopped".to_string(),
                clean.to_string(),
            )
        } else if clean.contains("InvalidBookmark") {
            (
                "Neo.ClientError.Transaction.InvalidBookmark".to_string(),
                clean.to_string(),
            )
        } else if clean.contains("Terminated") {
            (
                "Neo.ClientError.Transaction.Terminated".to_string(),
                clean.to_string(),
            )
        } else {
            (
                "Neo.ClientError.Database.GeneralError".to_string(),
                clean.to_string(),
            )
        };

        json!({
            "name": "DriverError",
            "data": {
                "id": next_id(),
                "msg": msg,
                "code": code,
                "errorType": "DriverError",
                "retryable": false
            }
        })
    }

    /// Processes an incoming TestKit JSON request and produces the TestKit JSON response.
    pub async fn process_request(&self, request: Value) -> Value {
        let name = request.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let data = request.get("data").cloned().unwrap_or(json!({}));

        match name {
            "GetFeatures" => {
                json!({
                    "name": "FeatureList",
                    "data": {
                        "features": [
                            "Feature:Bolt:4.2",
                            "Feature:Bolt:4.4",
                            "Feature:Bolt:5.0",
                            "Feature:Bolt:5.4",
                            "Optimization:PullPipelining",
                            "Optimization:ConnectionReuse"
                        ]
                    }
                })
            }
            "StartTest" | "StartSubTest" => {
                json!({"name": "RunTest"})
            }
            "NewDriver" => {
                let uri = data
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or("bolt://127.0.0.1:7687");
                let mut config = ConnectionConfig::from_uri(uri);

                if let Some(auth_tok) = data
                    .get("authorizationToken")
                    .or_else(|| data.get("authToken"))
                {
                    let tok_data = auth_tok.get("data").unwrap_or(auth_tok);
                    let scheme = tok_data
                        .get("scheme")
                        .and_then(|v| v.as_str())
                        .unwrap_or("none");
                    if scheme == "basic" {
                        let user = tok_data
                            .get("principal")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let pass = tok_data
                            .get("credentials")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        config.auth = Auth::Basic {
                            username: user.to_string(),
                            password: pass.to_string(),
                            realm: None,
                        };
                    }
                }

                let id = next_id();
                let driver = TestkitDriver { config };
                self.drivers.lock().await.insert(id.clone(), driver);
                json!({"name": "Driver", "data": {"id": id}})
            }
            "VerifyConnectivity" => {
                let driver_id = data.get("driverId").and_then(|v| v.as_str()).unwrap_or("");
                let drivers_guard = self.drivers.lock().await;
                if let Some(driver) = drivers_guard.get(driver_id) {
                    match BoltConnection::connect(&driver.config).await {
                        Ok(mut conn) => {
                            let _ = conn.close().await;
                            json!({"name": "Driver", "data": {"id": driver_id}})
                        }
                        Err(e) => Self::make_driver_error(&e.to_string()),
                    }
                } else {
                    Self::make_driver_error("Driver not found")
                }
            }
            "GetServerInfo" => {
                let driver_id = data.get("driverId").and_then(|v| v.as_str()).unwrap_or("");
                let drivers_guard = self.drivers.lock().await;
                if let Some(driver) = drivers_guard.get(driver_id) {
                    match BoltConnection::connect(&driver.config).await {
                        Ok(mut conn) => {
                            let agent = conn.server_agent().unwrap_or("Neo4j/5.26.0").to_string();
                            let _ = conn.close().await;
                            json!({
                                "name": "ServerInfo",
                                "data": {
                                    "address": "127.0.0.1:7687",
                                    "agent": agent,
                                    "protocolVersion": "5.4"
                                }
                            })
                        }
                        Err(e) => Self::make_driver_error(&e.to_string()),
                    }
                } else {
                    Self::make_driver_error("Driver not found")
                }
            }
            "CheckMultiDBSupport" => {
                let driver_id = data.get("driverId").and_then(|v| v.as_str()).unwrap_or("");
                json!({
                    "name": "MultiDBSupport",
                    "data": {
                        "id": driver_id,
                        "available": true
                    }
                })
            }
            "CheckDriverIsAuthenticated" => {
                let driver_id = data.get("driverId").and_then(|v| v.as_str()).unwrap_or("");
                let drivers_guard = self.drivers.lock().await;
                if let Some(driver) = drivers_guard.get(driver_id) {
                    match BoltConnection::connect(&driver.config).await {
                        Ok(mut conn) => {
                            let _ = conn.close().await;
                            json!({
                                "name": "DriverIsAuthenticated",
                                "data": {
                                    "id": driver_id,
                                    "authenticated": true
                                }
                            })
                        }
                        Err(e) => Self::make_driver_error(&e.to_string()),
                    }
                } else {
                    Self::make_driver_error("Driver not found")
                }
            }
            "DriverClose" => {
                let driver_id = data.get("driverId").and_then(|v| v.as_str()).unwrap_or("");
                self.drivers.lock().await.remove(driver_id);
                json!({"name": "Driver", "data": {"id": driver_id}})
            }
            "NewSession" => {
                let driver_id = data.get("driverId").and_then(|v| v.as_str()).unwrap_or("");
                let database = data
                    .get("database")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let bookmarks: Vec<String> = data
                    .get("bookmarks")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|s| s.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let last_bookmark = bookmarks.last().cloned();

                let drivers_guard = self.drivers.lock().await;
                let driver = match drivers_guard.get(driver_id) {
                    Some(d) => d,
                    None => return Self::make_driver_error("Driver not found"),
                };

                match BoltConnection::connect(&driver.config).await {
                    Ok(conn) => {
                        let session_id = next_id();
                        self.sessions.lock().await.insert(
                            session_id.clone(),
                            TestkitSession {
                                _driver_id: driver_id.to_string(),
                                database,
                                bookmarks,
                                last_bookmark,
                                conn: Mutex::new(conn),
                            },
                        );
                        json!({"name": "Session", "data": {"id": session_id}})
                    }
                    Err(e) => Self::make_driver_error(&e.to_string()),
                }
            }
            "SessionClose" => {
                let session_id = data.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(session) = self.sessions.lock().await.remove(session_id) {
                    let mut conn = session.conn.lock().await;
                    let _ = conn.close().await;
                }
                json!({"name": "Session", "data": {"id": session_id}})
            }
            "SessionLastBookmarks" => {
                let session_id = data.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                let sessions_guard = self.sessions.lock().await;
                let bms = if let Some(session) = sessions_guard.get(session_id) {
                    if let Some(ref bm) = session.last_bookmark {
                        vec![bm.clone()]
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };
                json!({
                    "name": "Bookmarks",
                    "data": {
                        "bookmarks": bms
                    }
                })
            }
            "SessionRun" => {
                let session_id = data.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                let cypher = data.get("cypher").and_then(|v| v.as_str()).unwrap_or("");

                let params_val = data.get("params");
                let mut params_map = HashMap::new();
                if let Some(obj) = params_val.and_then(|v| v.as_object()) {
                    for (k, v) in obj {
                        params_map.insert(k.clone(), Self::cypher_json_to_bolt_value(v));
                    }
                }

                let tx_meta_val = data.get("txMeta");
                let mut tx_meta = None;
                if let Some(obj) = tx_meta_val.and_then(|v| v.as_object()) {
                    let mut meta_map = HashMap::new();
                    for (k, v) in obj {
                        meta_map.insert(k.clone(), Self::cypher_json_to_bolt_value(v));
                    }
                    tx_meta = Some(meta_map);
                }
                let timeout_ms = data.get("timeout").and_then(|v| v.as_i64());

                let mut sessions_guard = self.sessions.lock().await;
                let session = match sessions_guard.get_mut(session_id) {
                    Some(s) => s,
                    None => {
                        return Self::make_driver_error("Session not found");
                    }
                };

                let mut session_bookmarks = session.bookmarks.clone();
                if let Some(ref last_bm) = session.last_bookmark
                    && !session_bookmarks.contains(last_bm)
                {
                    session_bookmarks.push(last_bm.clone());
                }
                let bookmarks_opt = if session_bookmarks.is_empty() {
                    None
                } else {
                    Some(session_bookmarks)
                };

                let mut conn = session.conn.lock().await;
                match conn
                    .execute_raw_records_with_options(
                        cypher,
                        &params_map,
                        session.database.as_deref(),
                        tx_meta,
                        timeout_ms,
                        bookmarks_opt,
                    )
                    .await
                {
                    Ok((keys, records, summary, stream_err)) => {
                        if let Some(ref bm) = conn.last_bookmark {
                            session.last_bookmark = Some(bm.clone());
                        }
                        let raw_params = params_val
                            .and_then(|v| v.as_object())
                            .cloned()
                            .map(|m| m.into_iter().collect())
                            .unwrap_or_default();
                        let result_id = next_id();
                        self.results.lock().await.insert(
                            result_id.clone(),
                            TestkitResult {
                                _keys: keys.clone(),
                                records,
                                current_index: 0,
                                summary,
                                stream_error: stream_err,
                                query_text: cypher.to_string(),
                                query_params: raw_params,
                                raw_metadata: conn.last_metadata.clone(),
                            },
                        );

                        json!({
                            "name": "Result",
                            "data": {
                                "id": result_id,
                                "keys": keys
                            }
                        })
                    }
                    Err(e) => Self::make_driver_error(&e.to_string()),
                }
            }
            "ResultNext" => {
                let result_id = data.get("resultId").and_then(|v| v.as_str()).unwrap_or("");
                let mut results_guard = self.results.lock().await;

                if let Some(res) = results_guard.get_mut(result_id) {
                    if res.current_index < res.records.len() {
                        let record = &res.records[res.current_index];
                        res.current_index += 1;

                        let values: Vec<Value> =
                            record.iter().map(Self::bolt_value_to_cypher_json).collect();
                        json!({
                            "name": "Record",
                            "data": {
                                "values": values
                            }
                        })
                    } else if let Some((code, msg)) = res.stream_error.take() {
                        json!({
                            "name": "DriverError",
                            "data": {
                                "id": next_id(),
                                "msg": msg,
                                "code": code,
                                "errorType": "DriverError",
                                "retryable": false
                            }
                        })
                    } else {
                        json!({"name": "NullRecord"})
                    }
                } else {
                    json!({
                        "name": "DriverError",
                        "data": {
                            "id": next_id(),
                            "msg": "Result not found",
                            "code": "Neo.ClientError.Statement.TypeError",
                            "errorType": "DriverError",
                            "retryable": false
                        }
                    })
                }
            }
            "ResultConsume" => {
                let result_id = data.get("resultId").and_then(|v| v.as_str()).unwrap_or("");
                let results_guard = self.results.lock().await;

                if let Some(res) = results_guard.get(result_id) {
                    let query_type = res
                        .raw_metadata
                        .as_ref()
                        .and_then(|m| m.get("type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(if res.summary.is_mutating() { "rw" } else { "r" });

                    let plan = res
                        .raw_metadata
                        .as_ref()
                        .and_then(|m| m.get("plan"))
                        .map(Self::bolt_value_to_plain_json)
                        .unwrap_or(Value::Null);

                    let profile = res
                        .raw_metadata
                        .as_ref()
                        .and_then(|m| m.get("profile"))
                        .map(Self::bolt_value_to_plain_json)
                        .unwrap_or(Value::Null);

                    let notifications = res
                        .raw_metadata
                        .as_ref()
                        .and_then(|m| m.get("notifications"))
                        .map(Self::bolt_value_to_plain_json)
                        .unwrap_or(Value::Null);

                    let database = res
                        .raw_metadata
                        .as_ref()
                        .and_then(|m| m.get("db"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("neo4j");

                    json!({
                        "name": "Summary",
                        "data": {
                            "serverInfo": {
                                "address": "127.0.0.1:7687",
                                "agent": "Neo4j/5.26.30",
                                "protocolVersion": "5.4"
                            },
                            "counters": {
                                "nodesCreated": res.summary.nodes_created,
                                "nodesDeleted": res.summary.nodes_deleted,
                                "relationshipsCreated": res.summary.relationships_created,
                                "relationshipsDeleted": res.summary.relationships_deleted,
                                "propertiesSet": res.summary.properties_set,
                                "labelsAdded": res.summary.labels_added,
                                "labelsRemoved": res.summary.labels_removed,
                                "indexesAdded": res.summary.indexes_added,
                                "indexesRemoved": 0,
                                "constraintsAdded": res.summary.constraints_added,
                                "constraintsRemoved": 0,
                                "containsUpdates": res.summary.is_mutating(),
                                "containsSystemUpdates": false,
                                "systemUpdates": 0
                            },
                            "database": database,
                            "notifications": notifications,
                            "plan": plan,
                            "profile": profile,
                            "query": {
                                "text": res.query_text.clone(),
                                "parameters": res.query_params.clone()
                            },
                            "queryType": query_type,
                            "resultAvailableAfter": 1,
                            "resultConsumedAfter": 1
                        }
                    })
                } else {
                    json!({
                        "name": "DriverError",
                        "data": {
                            "id": next_id(),
                            "msg": "Result not found",
                            "code": "Neo.ClientError.Statement.TypeError",
                            "errorType": "DriverError",
                            "retryable": false
                        }
                    })
                }
            }
            "SessionBeginTransaction" => {
                let session_id = data.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                let tx_meta_val = data.get("txMeta");
                let mut tx_meta = None;
                if let Some(obj) = tx_meta_val.and_then(|v| v.as_object()) {
                    let mut meta_map = HashMap::new();
                    for (k, v) in obj {
                        meta_map.insert(k.clone(), Self::cypher_json_to_bolt_value(v));
                    }
                    tx_meta = Some(meta_map);
                }
                let timeout_ms = data.get("timeout").and_then(|v| v.as_i64());
                let bookmarks = data.get("bookmarks").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(|s| s.to_string()))
                        .collect::<Vec<String>>()
                });

                let sessions_guard = self.sessions.lock().await;
                let session = match sessions_guard.get(session_id) {
                    Some(s) => s,
                    None => {
                        return Self::make_driver_error("Session not found");
                    }
                };

                let mut session_bookmarks = session.bookmarks.clone();
                if let Some(bms) = bookmarks {
                    session_bookmarks.extend(bms);
                }
                if let Some(ref last_bm) = session.last_bookmark
                    && !session_bookmarks.contains(last_bm)
                {
                    session_bookmarks.push(last_bm.clone());
                }
                let bookmark_opt = if session_bookmarks.is_empty() {
                    None
                } else {
                    Some(session_bookmarks)
                };

                let mut conn = session.conn.lock().await;
                match conn
                    .begin_tx_with_options(
                        session.database.as_deref(),
                        tx_meta,
                        timeout_ms,
                        bookmark_opt,
                    )
                    .await
                {
                    Ok(()) => {
                        let tx_id = next_id();
                        self.transactions.lock().await.insert(
                            tx_id.clone(),
                            TestkitTx {
                                session_id: session_id.to_string(),
                                failed: false,
                            },
                        );
                        json!({"name": "Transaction", "data": {"id": tx_id}})
                    }
                    Err(e) => Self::make_driver_error(&e.to_string()),
                }
            }
            "SessionReadTransaction" | "SessionWriteTransaction" => {
                let session_id = data.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                let tx_meta_val = data.get("txMeta");
                let mut tx_meta = None;
                if let Some(obj) = tx_meta_val.and_then(|v| v.as_object()) {
                    let mut meta_map = HashMap::new();
                    for (k, v) in obj {
                        meta_map.insert(k.clone(), Self::cypher_json_to_bolt_value(v));
                    }
                    tx_meta = Some(meta_map);
                }
                let timeout_ms = data.get("timeout").and_then(|v| v.as_i64());
                let bookmarks = data.get("bookmarks").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(|s| s.to_string()))
                        .collect::<Vec<String>>()
                });

                let sessions_guard = self.sessions.lock().await;
                let session = match sessions_guard.get(session_id) {
                    Some(s) => s,
                    None => {
                        return Self::make_driver_error("Session not found");
                    }
                };

                let mut session_bookmarks = session.bookmarks.clone();
                if let Some(bms) = bookmarks {
                    session_bookmarks.extend(bms);
                }
                if let Some(ref last_bm) = session.last_bookmark
                    && !session_bookmarks.contains(last_bm)
                {
                    session_bookmarks.push(last_bm.clone());
                }
                let bookmark_opt = if session_bookmarks.is_empty() {
                    None
                } else {
                    Some(session_bookmarks)
                };

                let mut conn = session.conn.lock().await;
                match conn
                    .begin_tx_with_options(
                        session.database.as_deref(),
                        tx_meta,
                        timeout_ms,
                        bookmark_opt,
                    )
                    .await
                {
                    Ok(()) => {
                        let tx_id = next_id();
                        self.transactions.lock().await.insert(
                            tx_id.clone(),
                            TestkitTx {
                                session_id: session_id.to_string(),
                                failed: false,
                            },
                        );
                        json!({"name": "RetryableTry", "data": {"id": tx_id}})
                    }
                    Err(e) => Self::make_driver_error(&e.to_string()),
                }
            }
            "TransactionRun" => {
                let tx_id = data.get("txId").and_then(|v| v.as_str()).unwrap_or("");
                let cypher = data.get("cypher").and_then(|v| v.as_str()).unwrap_or("");

                let params_val = data.get("params");
                let mut params_map = HashMap::new();
                if let Some(obj) = params_val.and_then(|v| v.as_object()) {
                    for (k, v) in obj {
                        params_map.insert(k.clone(), Self::cypher_json_to_bolt_value(v));
                    }
                }

                let mut tx_guard = self.transactions.lock().await;
                let tx = match tx_guard.get_mut(tx_id) {
                    Some(t) => t,
                    None => {
                        return Self::make_driver_error("Transaction not found");
                    }
                };

                if tx.failed {
                    return Self::make_driver_error(
                        "Neo.ClientError.Transaction.Terminated: Cannot run query in a failed transaction",
                    );
                }

                let sessions_guard = self.sessions.lock().await;
                let session = match sessions_guard.get(&tx.session_id) {
                    Some(s) => s,
                    None => {
                        return Self::make_driver_error("Session not found");
                    }
                };

                let mut conn = session.conn.lock().await;
                match conn
                    .execute_raw_records_with_options(cypher, &params_map, None, None, None, None)
                    .await
                {
                    Ok((keys, records, summary, stream_err)) => {
                        if stream_err.is_some() {
                            tx.failed = true;
                        }
                        let raw_params = params_val
                            .and_then(|v| v.as_object())
                            .cloned()
                            .map(|m| m.into_iter().collect())
                            .unwrap_or_default();
                        let result_id = next_id();
                        self.results.lock().await.insert(
                            result_id.clone(),
                            TestkitResult {
                                _keys: keys.clone(),
                                records,
                                current_index: 0,
                                summary,
                                stream_error: stream_err,
                                query_text: cypher.to_string(),
                                query_params: raw_params,
                                raw_metadata: conn.last_metadata.clone(),
                            },
                        );

                        json!({
                            "name": "Result",
                            "data": {
                                "id": result_id,
                                "keys": keys
                            }
                        })
                    }
                    Err(e) => {
                        tx.failed = true;
                        Self::make_driver_error(&e.to_string())
                    }
                }
            }
            "RetryablePositive" => {
                let session_id = data.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                let mut tx_guard = self.transactions.lock().await;
                let tx_entry = tx_guard
                    .iter()
                    .find(|(_, tx)| tx.session_id == session_id)
                    .map(|(k, _)| k.clone());

                if let Some(tx_id) = tx_entry
                    && let Some(tx) = tx_guard.remove(&tx_id)
                {
                    let mut sessions_guard = self.sessions.lock().await;
                    if let Some(session) = sessions_guard.get_mut(&tx.session_id) {
                        let mut conn = session.conn.lock().await;
                        let _ = conn.commit_tx().await;
                        if let Some(ref bm) = conn.last_bookmark {
                            session.last_bookmark = Some(bm.clone());
                        }
                    }
                }
                json!({"name": "RetryableDone"})
            }
            "RetryableNegative" => {
                let session_id = data.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                let error_id = data.get("errorId").and_then(|v| v.as_str()).unwrap_or("");
                let mut tx_guard = self.transactions.lock().await;
                let tx_entry = tx_guard
                    .iter()
                    .find(|(_, tx)| tx.session_id == session_id)
                    .map(|(k, _)| k.clone());

                if let Some(tx_id) = tx_entry
                    && let Some(tx) = tx_guard.remove(&tx_id)
                {
                    let sessions_guard = self.sessions.lock().await;
                    if let Some(session) = sessions_guard.get(&tx.session_id) {
                        let mut conn = session.conn.lock().await;
                        let _ = conn.rollback_tx().await;
                    }
                }

                if error_id.is_empty() {
                    json!({
                        "name": "FrontendError",
                        "data": {
                            "msg": "Client transaction function failed"
                        }
                    })
                } else {
                    json!({
                        "name": "DriverError",
                        "data": {
                            "id": error_id,
                            "msg": "Transaction aborted",
                            "errorType": "DriverError",
                            "code": "Neo.ClientError.Transaction.TransactionHookFailed",
                            "retryable": false
                        }
                    })
                }
            }
            "TransactionCommit" => {
                let tx_id = data.get("txId").and_then(|v| v.as_str()).unwrap_or("");
                let mut tx_guard = self.transactions.lock().await;
                if let Some(tx) = tx_guard.remove(tx_id) {
                    if tx.failed {
                        return Self::make_driver_error(
                            "Neo.ClientError.Transaction.TransactionHookFailed: Cannot commit a failed transaction",
                        );
                    }
                    let mut sessions_guard = self.sessions.lock().await;
                    if let Some(session) = sessions_guard.get_mut(&tx.session_id) {
                        let mut conn = session.conn.lock().await;
                        match conn.commit_tx().await {
                            Ok(()) => {
                                if let Some(ref bm) = conn.last_bookmark {
                                    session.last_bookmark = Some(bm.clone());
                                }
                                json!({"name": "Transaction", "data": {"id": tx_id}})
                            }
                            Err(e) => Self::make_driver_error(&e.to_string()),
                        }
                    } else {
                        Self::make_driver_error("Session not found")
                    }
                } else {
                    Self::make_driver_error("Transaction not found")
                }
            }
            "TransactionClose" => {
                let tx_id = data.get("txId").and_then(|v| v.as_str()).unwrap_or("");
                let mut tx_guard = self.transactions.lock().await;
                if let Some(tx) = tx_guard.remove(tx_id) {
                    let sessions_guard = self.sessions.lock().await;
                    if let Some(session) = sessions_guard.get(&tx.session_id) {
                        let mut conn = session.conn.lock().await;
                        let _ = conn.rollback_tx().await;
                    }
                }
                json!({"name": "Transaction", "data": {"id": tx_id}})
            }
            "TransactionRollback" => {
                let tx_id = data.get("txId").and_then(|v| v.as_str()).unwrap_or("");
                let mut tx_guard = self.transactions.lock().await;
                if let Some(tx) = tx_guard.remove(tx_id) {
                    let sessions_guard = self.sessions.lock().await;
                    if let Some(session) = sessions_guard.get(&tx.session_id) {
                        let mut conn = session.conn.lock().await;
                        let _ = conn.rollback_tx().await;
                    }
                    json!({"name": "Transaction", "data": {"id": tx_id}})
                } else {
                    Self::make_driver_error("Transaction not found")
                }
            }
            other => {
                json!({
                    "name": "BackendError",
                    "data": {
                        "msg": format!("Unsupported TestKit backend request: {}", other)
                    }
                })
            }
        }
    }

    /// Runs the TestKit backend server on the given TCP listener.
    pub async fn run_server(listener: TcpListener) -> Result<()> {
        let backend = std::sync::Arc::new(Self::new());

        while let Ok((socket, _)) = listener.accept().await {
            let backend_clone = backend.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = socket.into_split();
                let mut buf_reader = BufReader::new(reader);
                let mut line = String::new();

                while let Ok(n) = buf_reader.read_line(&mut line).await {
                    if n == 0 {
                        break;
                    }

                    let trimmed = line.trim();
                    let is_nutkit = trimmed == "#request begin";
                    let is_step = trimmed == "#=== step: start =============";

                    if is_nutkit || is_step {
                        let end_delimiter = if is_nutkit {
                            "#request end"
                        } else {
                            "#=== step: end ============="
                        };

                        let mut json_str = String::new();
                        loop {
                            let mut step_line = String::new();
                            if buf_reader.read_line(&mut step_line).await.is_err() {
                                break;
                            }
                            if step_line.trim() == end_delimiter {
                                break;
                            }
                            json_str.push_str(&step_line);
                        }

                        if let Ok(req_val) = serde_json::from_str::<Value>(&json_str) {
                            let resp_val = backend_clone.process_request(req_val).await;
                            let resp_str = serde_json::to_string(&resp_val).unwrap_or_default();

                            let framed_resp = if is_nutkit {
                                format!("#response begin\n{}\n#response end\n", resp_str)
                            } else {
                                format!(
                                    "#=== step: start =============\n{}\n#=== step: end =============\n",
                                    resp_str
                                )
                            };
                            let _ = writer.write_all(framed_resp.as_bytes()).await;
                            let _ = writer.flush().await;
                        }
                    }
                    line.clear();
                }
            });
        }

        Ok(())
    }
}
