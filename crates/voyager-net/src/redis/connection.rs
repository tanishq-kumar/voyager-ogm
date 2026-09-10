//! Connection implementation for Redis and FalkorDB graph engines.

use async_trait::async_trait;
use bytes::BytesMut;
use std::collections::HashMap;
use tokio::net::TcpStream;

use crate::config::Auth;
use crate::engine::{AsyncConnection, QueryResult};
use crate::error::{NetError, Result};
use crate::redis::codec::{read_resp_value, write_command};
use crate::redis::graph::FalkorQueryResult;
use crate::redis::resp::RespValue;
use crate::redis::transaction::{RedisTransaction, format_cypher_with_params};
use crate::uri::ParsedUri;

/// Connection to a Redis / FalkorDB server implementing [`AsyncConnection`].
pub struct RedisConnection {
    stream: TcpStream,
    uri: ParsedUri,
    buffer: BytesMut,
    is_resp3: bool,
    server_info: HashMap<String, RespValue>,
    default_graph: String,
    in_transaction: bool,
    is_valid: bool,
}

impl RedisConnection {
    /// Asynchronously establishes a new connection to Redis / FalkorDB based on the parsed URI.
    pub async fn connect(uri: &ParsedUri) -> Result<Self> {
        let socket_addr = uri.socket_addr();
        let stream = TcpStream::connect(&socket_addr).await.map_err(|e| {
            NetError::ConnectionFailed(format!(
                "Failed to connect to Redis/FalkorDB host at {}: {}",
                socket_addr, e
            ))
        })?;

        // Disable Nagle's algorithm for low-latency graph query execution
        let _ = stream.set_nodelay(true);

        // Resolve default graph name: from database segment, query params, or default
        let default_graph = if let Some(db) = &uri.database {
            db.clone()
        } else if let Some(graph) = uri.params.get("graph") {
            graph.clone()
        } else {
            "voyager_graph".to_string()
        };

        let mut conn = Self {
            stream,
            uri: uri.clone(),
            buffer: BytesMut::with_capacity(8192),
            is_resp3: false,
            server_info: HashMap::new(),
            default_graph,
            in_transaction: false,
            is_valid: true,
        };

        // Perform protocol handshake & authentication
        conn.handshake().await?;

        Ok(conn)
    }

    /// Performs the RESP handshake, protocol negotiation (RESP3), and authentication.
    async fn handshake(&mut self) -> Result<()> {
        let auth_credentials = match &self.uri.auth {
            Auth::Basic {
                username, password, ..
            } => Some((Some(username.as_str()), password.as_str())),
            Auth::Bearer { token } => Some((None, token.as_str())),
            Auth::None => None,
        };

        // 1. Try RESP3 handshake via HELLO 3
        let hello_result = match auth_credentials {
            Some((Some(user), pass)) => {
                write_command(&mut self.stream, &["HELLO", "3", "AUTH", user, pass]).await
            }
            Some((None, pass)) => {
                write_command(&mut self.stream, &["HELLO", "3", "AUTH", "default", pass]).await
            }
            None => write_command(&mut self.stream, &["HELLO", "3"]).await,
        };

        let mut switched_to_resp3 = false;
        if hello_result.is_ok()
            && let Ok(resp) = read_resp_value(&mut self.stream, &mut self.buffer).await
        {
            match resp {
                RespValue::Map(pairs) => {
                    self.is_resp3 = true;
                    switched_to_resp3 = true;
                    for (k, v) in pairs {
                        self.server_info.insert(k.to_string_lossy(), v);
                    }
                }
                RespValue::Array(Some(items)) => {
                    // Some servers reply with flat arrays to HELLO
                    self.is_resp3 = true;
                    switched_to_resp3 = true;
                    for chunk in items.chunks(2) {
                        if chunk.len() == 2 {
                            self.server_info
                                .insert(chunk[0].to_string_lossy(), chunk[1].clone());
                        }
                    }
                }
                _ => {
                    // Server rejected HELLO 3 (e.g. unknown command), fall back to RESP2
                }
            }
        }

        // 2. If HELLO 3 wasn't supported, fall back to RESP2 and authenticate via AUTH if needed
        if !switched_to_resp3 && let Some((user_opt, pass)) = auth_credentials {
            let auth_result = if let Some(user) = user_opt {
                write_command(&mut self.stream, &["AUTH", user, pass]).await
            } else {
                write_command(&mut self.stream, &["AUTH", pass]).await
            };

            if auth_result.is_ok() {
                let resp = read_resp_value(&mut self.stream, &mut self.buffer).await?;
                match resp {
                    RespValue::SimpleString(s) if s == "OK" => {}
                    RespValue::Error(err) => {
                        return Err(NetError::AuthenticationFailed(format!(
                            "Redis authentication failed: {}",
                            err
                        )));
                    }
                    other => {
                        return Err(NetError::ProtocolError(format!(
                            "Unexpected authentication response: {:?}",
                            other
                        )));
                    }
                }
            }
        }

        // 3. If database index was specified as a numeric index in query params, switch db
        if let Some(db_str) = self.uri.params.get("db")
            && let Ok(db_idx) = db_str.parse::<u32>()
        {
            write_command(&mut self.stream, &["SELECT", &db_idx.to_string()]).await?;
            let resp = read_resp_value(&mut self.stream, &mut self.buffer).await?;
            if let RespValue::Error(err) = resp {
                return Err(NetError::ExecutionError(format!(
                    "Failed to SELECT Redis db {}: {}",
                    db_idx, err
                )));
            }
        }

        Ok(())
    }

    /// Returns `true` if this connection negotiated RESP3.
    pub fn is_resp3(&self) -> bool {
        self.is_resp3
    }

    /// Returns the default graph name associated with this connection.
    pub fn default_graph(&self) -> &str {
        &self.default_graph
    }

    /// Sets the default graph name for subsequent queries.
    pub fn set_default_graph(&mut self, graph: String) {
        self.default_graph = graph;
    }

    /// Returns the metadata key-value map reported by the server during HELLO.
    pub fn server_info(&self) -> &HashMap<String, RespValue> {
        &self.server_info
    }

    /// Internal helper to update connection transaction status.
    pub(crate) fn set_in_transaction(&mut self, in_tx: bool) {
        self.in_transaction = in_tx;
    }

    /// Executes an arbitrary raw Redis / Valkey command and returns the raw [`RespValue`].
    pub async fn execute_raw(&mut self, args: &[&str]) -> Result<RespValue> {
        write_command(&mut self.stream, args).await?;
        read_resp_value(&mut self.stream, &mut self.buffer).await
    }

    /// Executes a Cypher query on FalkorDB via `GRAPH.QUERY`.
    pub async fn graph_query(
        &mut self,
        graph: &str,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<FalkorQueryResult> {
        let formatted = format_cypher_with_params(query, params);
        let resp = self
            .execute_raw(&["GRAPH.QUERY", graph, &formatted])
            .await?;
        FalkorQueryResult::parse(resp)
    }

    /// Executes a read-only Cypher query on FalkorDB via `GRAPH.RO_QUERY`.
    pub async fn graph_ro_query(
        &mut self,
        graph: &str,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<FalkorQueryResult> {
        let formatted = format_cypher_with_params(query, params);
        let resp = self
            .execute_raw(&["GRAPH.RO_QUERY", graph, &formatted])
            .await?;
        FalkorQueryResult::parse(resp)
    }

    /// Deletes an entire graph from FalkorDB via `GRAPH.DELETE`.
    pub async fn graph_delete(&mut self, graph: &str) -> Result<()> {
        let resp = self.execute_raw(&["GRAPH.DELETE", graph]).await?;
        match resp {
            RespValue::SimpleString(_) => Ok(()),
            RespValue::Error(e) if e.contains("does not exist") || e.contains("Graph key") => {
                // Deleting a non-existent graph is considered safe/idempotent
                Ok(())
            }
            RespValue::Error(e) => Err(NetError::ExecutionError(format!(
                "Failed to delete graph '{}': {}",
                graph, e
            ))),
            _ => Ok(()),
        }
    }

    /// Begins a new transaction on this connection using `MULTI`.
    pub async fn begin_transaction(&mut self) -> Result<RedisTransaction<'_>> {
        RedisTransaction::begin(self).await
    }
}

#[async_trait]
impl AsyncConnection for RedisConnection {
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        let graph = self.default_graph.clone();
        let falkor_res = self.graph_query(&graph, query, params).await?;
        falkor_res.to_query_result()
    }

    async fn ping(&mut self) -> Result<()> {
        let resp = self.execute_raw(&["PING"]).await?;
        match resp {
            RespValue::SimpleString(s) if s == "PONG" => Ok(()),
            RespValue::BulkString(Some(b)) if b == b"PONG" => Ok(()),
            RespValue::Error(err) => Err(NetError::ExecutionError(format!("PING failed: {}", err))),
            other => Err(NetError::ProtocolError(format!(
                "Unexpected PING response: {:?}",
                other
            ))),
        }
    }

    async fn reset(&mut self) -> Result<()> {
        if self.in_transaction {
            let _ = self.execute_raw(&["DISCARD"]).await;
            self.in_transaction = false;
        }
        self.buffer.clear();
        Ok(())
    }

    fn is_valid(&self) -> bool {
        self.is_valid
    }

    fn is_in_transaction(&self) -> bool {
        self.in_transaction
    }

    async fn close(&mut self) -> Result<()> {
        let _ = write_command(&mut self.stream, &["QUIT"]).await;
        self.is_valid = false;
        Ok(())
    }
}
