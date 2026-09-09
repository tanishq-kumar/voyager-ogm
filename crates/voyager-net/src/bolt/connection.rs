//! Bolt asynchronous connection implementation over raw TCP streams.

use arrow_array::builder::{BooleanBuilder, Float64Builder, Int64Builder, StringBuilder};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use bytes::BytesMut;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::debug;

use super::messages::{BoltRequest, BoltResponse};
use super::packstream::BoltValue;
use super::stream::{encode_chunks, read_message_frame, write_message_frame};
use crate::config::{Auth, ConnectionConfig};
use crate::engine::{AsyncConnection, QueryResult, QuerySummary};
use crate::error::{NetError, Result};
use crate::uri::ParsedUri;

/// Magic 4-byte header sent at the start of every Bolt TCP connection (`0x60 0x60 0xB0 0x17`).
pub const BOLT_MAGIC_PREAMBLE: [u8; 4] = [0x60, 0x60, 0xB0, 0x17];

/// Default version proposals sent during handshake negotiation (v5.4, v5.0, v4.4, v4.2).
pub const BOLT_PROPOSED_VERSIONS: [u8; 16] = [
    0x00, 0x00, 0x04, 0x05, // v5.4
    0x00, 0x00, 0x00, 0x05, // v5.0
    0x00, 0x00, 0x04, 0x04, // v4.4
    0x00, 0x00, 0x02, 0x04, // v4.2
];

/// Negotiated Bolt protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoltVersion {
    /// Major protocol version (e.g. 5).
    pub major: u8,
    /// Minor protocol version (e.g. 4).
    pub minor: u8,
}

/// Physical asynchronous Bolt connection managing socket I/O, PackStream framing, and state.
pub struct BoltConnection<S: AsyncRead + AsyncWrite + Unpin + Send + Sync = TcpStream> {
    stream: S,
    version: BoltVersion,
    is_valid: bool,
    in_transaction: bool,
    server_agent: Option<String>,
    /// Last bookmark string received from the server.
    pub last_bookmark: Option<String>,
    /// Last execution metadata received from RUN and PULL responses.
    pub last_metadata: Option<HashMap<String, BoltValue>>,
}

impl BoltConnection<TcpStream> {
    /// Establishes a new physical TCP connection to the target Bolt endpoint and performs handshake/auth.
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        let uri = ParsedUri::parse(&config.uri)?;
        let addr = uri.socket_addr();

        debug!("Connecting to Bolt server at {}", addr);
        let stream = timeout(config.connect_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| NetError::Timeout(format!("Connection to {} timed out", addr)))?
            .map_err(|e| {
                NetError::ConnectionFailed(format!("Failed to connect to {}: {}", addr, e))
            })?;

        // Disable Nagle's algorithm to eliminate 40-200ms delayed-ACK packet stalls
        let _ = stream.set_nodelay(true);
        let mut stream = stream;

        // 1. Send Handshake
        let mut handshake_bytes = Vec::with_capacity(20);
        handshake_bytes.extend_from_slice(&BOLT_MAGIC_PREAMBLE);
        handshake_bytes.extend_from_slice(&BOLT_PROPOSED_VERSIONS);

        stream.write_all(&handshake_bytes).await.map_err(|e| {
            NetError::ConnectionFailed(format!("Failed to write Bolt handshake: {}", e))
        })?;
        stream.flush().await.map_err(|e| {
            NetError::ConnectionFailed(format!("Failed to flush Bolt handshake: {}", e))
        })?;

        // 2. Read Server Version Response (4 bytes)
        let mut version_resp = [0u8; 4];
        stream.read_exact(&mut version_resp).await.map_err(|e| {
            NetError::ConnectionFailed(format!("Failed to read Bolt version response: {}", e))
        })?;

        if version_resp == [0, 0, 0, 0] {
            return Err(NetError::ProtocolError(
                "Bolt server rejected proposed protocol versions".to_string(),
            ));
        }

        let major = version_resp[3];
        let minor = version_resp[2];
        let version = BoltVersion { major, minor };
        debug!("Negotiated Bolt protocol version v{}.{}", major, minor);

        // 3. Send HELLO / Auth Handshake
        let auth_pair = match &config.auth {
            Auth::Basic {
                username, password, ..
            } => Some((username.as_str(), password.as_str())),
            _ => match &uri.auth {
                Auth::Basic {
                    username, password, ..
                } => Some((username.as_str(), password.as_str())),
                _ => None,
            },
        };

        let db = config.database.as_deref().or(uri.database.as_deref());

        let mut conn = Self {
            stream,
            version,
            is_valid: true,
            in_transaction: false,
            server_agent: None,
            last_bookmark: None,
            last_metadata: None,
        };

        let is_v51_or_higher = version.major > 5 || (version.major == 5 && version.minor >= 1);

        if is_v51_or_higher {
            // 3a. Bolt 5.1+: HELLO without auth, followed by LOGON
            let hello_req = BoltRequest::hello_v51("Voyager-OGM/0.4.6", None);
            conn.send_request(&hello_req).await?;
            let resp = conn.receive_response().await?;

            match resp {
                BoltResponse::Success { metadata } => {
                    if let Some(agent) = metadata.get("server").and_then(|v| v.as_str()) {
                        conn.server_agent = Some(agent.to_string());
                        debug!("Connected to Bolt server agent: {}", agent);
                    }
                }
                BoltResponse::Failure { metadata } => {
                    let code = metadata
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("HelloError");
                    let msg = metadata
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Handshake failed");
                    conn.is_valid = false;
                    return Err(NetError::ProtocolError(format!("{}: {}", code, msg)));
                }
                other => {
                    conn.is_valid = false;
                    return Err(NetError::ProtocolError(format!(
                        "Unexpected response to HELLO: {:?}",
                        other
                    )));
                }
            }

            // 3b. Send LOGON
            let logon_req = BoltRequest::logon(auth_pair);
            conn.send_request(&logon_req).await?;
            let resp = conn.receive_response().await?;

            match resp {
                BoltResponse::Success { .. } => {}
                BoltResponse::Failure { metadata } => {
                    let code = metadata
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("AuthError");
                    let msg = metadata
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Authentication failed");
                    conn.is_valid = false;
                    return Err(NetError::AuthenticationFailed(format!("{}: {}", code, msg)));
                }
                other => {
                    conn.is_valid = false;
                    return Err(NetError::ProtocolError(format!(
                        "Unexpected response to LOGON: {:?}",
                        other
                    )));
                }
            }
        } else {
            // 3. Legacy Bolt <= 5.0: HELLO with inline auth
            let hello_req = BoltRequest::hello_legacy("Voyager-OGM/0.4.6", auth_pair, db);
            conn.send_request(&hello_req).await?;
            let resp = conn.receive_response().await?;

            match resp {
                BoltResponse::Success { metadata } => {
                    if let Some(agent) = metadata.get("server").and_then(|v| v.as_str()) {
                        conn.server_agent = Some(agent.to_string());
                        debug!("Connected to Bolt server agent: {}", agent);
                    }
                }
                BoltResponse::Failure { metadata } => {
                    let code = metadata
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("AuthError");
                    let msg = metadata
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Authentication failed");
                    conn.is_valid = false;
                    return Err(NetError::AuthenticationFailed(format!("{}: {}", code, msg)));
                }
                other => {
                    conn.is_valid = false;
                    return Err(NetError::ProtocolError(format!(
                        "Unexpected response to HELLO: {:?}",
                        other
                    )));
                }
            }
        }

        Ok(conn)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + Sync> BoltConnection<S> {
    /// Creates a `BoltConnection` from an existing stream and negotiated version (useful for tests/stubs).
    pub fn from_stream(stream: S, version: BoltVersion) -> Self {
        Self {
            stream,
            version,
            is_valid: true,
            in_transaction: false,
            server_agent: None,
            last_bookmark: None,
            last_metadata: None,
        }
    }

    /// Returns the negotiated Bolt protocol version.
    pub fn version(&self) -> BoltVersion {
        self.version
    }

    /// Returns the server agent string if reported by the database server.
    pub fn server_agent(&self) -> Option<&str> {
        self.server_agent.as_deref()
    }

    /// Sends an encoded `BoltRequest` message frame over the stream.
    pub async fn send_request(&mut self, request: &BoltRequest) -> Result<()> {
        let mut payload = BytesMut::new();
        request.encode(&mut payload);

        let mut chunk_buf = BytesMut::new();
        encode_chunks(&payload, &mut chunk_buf);

        write_message_frame(&mut self.stream, &chunk_buf).await
    }

    /// Receives and decodes a single `BoltResponse` message from the stream.
    pub async fn receive_response(&mut self) -> Result<BoltResponse> {
        let mut frame_bytes = read_message_frame(&mut self.stream).await?;
        BoltResponse::decode(&mut frame_bytes)
    }

    /// Converts raw records and column names into an Apache Arrow `RecordBatch`.
    pub fn build_record_batch(
        columns: &[String],
        records: &[Vec<BoltValue>],
    ) -> Result<RecordBatch> {
        if columns.is_empty() {
            let schema = Arc::new(Schema::empty());
            return Ok(RecordBatch::new_empty(schema));
        }

        let num_rows = records.len();
        let mut fields = Vec::with_capacity(columns.len());
        let mut arrow_arrays: Vec<ArrayRef> = Vec::with_capacity(columns.len());

        for (col_idx, col_name) in columns.iter().enumerate() {
            let mut int_count = 0usize;
            let mut float_count = 0usize;
            let mut bool_count = 0usize;
            let mut non_null_count = 0usize;

            for row in records {
                match row.get(col_idx) {
                    Some(BoltValue::Integer(_)) => {
                        int_count += 1;
                        non_null_count += 1;
                    }
                    Some(BoltValue::Float(_)) => {
                        float_count += 1;
                        non_null_count += 1;
                    }
                    Some(BoltValue::Boolean(_)) => {
                        bool_count += 1;
                        non_null_count += 1;
                    }
                    Some(BoltValue::Null) | None => {}
                    Some(_) => {
                        non_null_count += 1;
                    }
                }
            }

            if non_null_count > 0 && int_count == non_null_count {
                // Pure homogeneous 64-bit integer column
                let mut builder = Int64Builder::with_capacity(num_rows);
                for row in records {
                    match row.get(col_idx) {
                        Some(BoltValue::Integer(i)) => builder.append_value(*i),
                        _ => builder.append_null(),
                    }
                }
                fields.push(Field::new(col_name, DataType::Int64, true));
                arrow_arrays.push(Arc::new(builder.finish()));
            } else if non_null_count > 0 && float_count == non_null_count {
                // Pure homogeneous 64-bit floating point column
                let mut builder = Float64Builder::with_capacity(num_rows);
                for row in records {
                    match row.get(col_idx) {
                        Some(BoltValue::Float(f)) => builder.append_value(*f),
                        _ => builder.append_null(),
                    }
                }
                fields.push(Field::new(col_name, DataType::Float64, true));
                arrow_arrays.push(Arc::new(builder.finish()));
            } else if non_null_count > 0
                && (int_count + float_count) == non_null_count
                && int_count > 0
                && float_count > 0
            {
                // Mixed numeric column (Integers + Floats) -> Safely promote to Float64
                let mut builder = Float64Builder::with_capacity(num_rows);
                for row in records {
                    match row.get(col_idx) {
                        Some(BoltValue::Float(f)) => builder.append_value(*f),
                        Some(BoltValue::Integer(i)) => builder.append_value(*i as f64),
                        _ => builder.append_null(),
                    }
                }
                fields.push(Field::new(col_name, DataType::Float64, true));
                arrow_arrays.push(Arc::new(builder.finish()));
            } else if non_null_count > 0 && bool_count == non_null_count {
                // Pure homogeneous boolean column
                let mut builder = BooleanBuilder::with_capacity(num_rows);
                for row in records {
                    match row.get(col_idx) {
                        Some(BoltValue::Boolean(b)) => builder.append_value(*b),
                        _ => builder.append_null(),
                    }
                }
                fields.push(Field::new(col_name, DataType::Boolean, true));
                arrow_arrays.push(Arc::new(builder.finish()));
            } else {
                // Heterogeneous / variant column, pure string, or complex graph structures
                // Safely format as UTF-8 string to guarantee ZERO SILENT DATA LOSS
                let mut builder = StringBuilder::with_capacity(num_rows, num_rows * 32);
                for row in records {
                    match row.get(col_idx) {
                        Some(BoltValue::String(s)) => builder.append_value(s),
                        Some(BoltValue::Integer(i)) => builder.append_value(i.to_string()),
                        Some(BoltValue::Float(f)) => builder.append_value(f.to_string()),
                        Some(BoltValue::Boolean(b)) => builder.append_value(b.to_string()),
                        Some(BoltValue::Null) | None => builder.append_null(),
                        Some(other) => {
                            let json_str = serde_json::to_string(other)
                                .unwrap_or_else(|_| format!("{:?}", other));
                            builder.append_value(&json_str);
                        }
                    }
                }
                fields.push(Field::new(col_name, DataType::Utf8, true));
                arrow_arrays.push(Arc::new(builder.finish()));
            }
        }

        let schema = Arc::new(Schema::new(fields));
        RecordBatch::try_new(schema, arrow_arrays).map_err(NetError::ArrowError)
    }

    /// Parses execution mutation metrics from a `SUCCESS` response metadata map.
    fn parse_summary_metrics(metadata: &HashMap<String, BoltValue>) -> QuerySummary {
        let mut summary = QuerySummary::empty();

        if let Some(BoltValue::Map(stats)) = metadata.get("stats") {
            if let Some(BoltValue::Integer(n)) = stats.get("nodes-created") {
                summary.nodes_created = *n as u64;
            }
            if let Some(BoltValue::Integer(n)) = stats.get("nodes-deleted") {
                summary.nodes_deleted = *n as u64;
            }
            if let Some(BoltValue::Integer(n)) = stats.get("relationships-created") {
                summary.relationships_created = *n as u64;
            }
            if let Some(BoltValue::Integer(n)) = stats.get("relationships-deleted") {
                summary.relationships_deleted = *n as u64;
            }
            if let Some(BoltValue::Integer(n)) = stats.get("properties-set") {
                summary.properties_set = *n as u64;
            }
            if let Some(BoltValue::Integer(n)) = stats.get("labels-added") {
                summary.labels_added = *n as u64;
            }
            if let Some(BoltValue::Integer(n)) = stats.get("labels-removed") {
                summary.labels_removed = *n as u64;
            }
            if let Some(BoltValue::Integer(n)) = stats.get("indexes-added") {
                summary.indexes_added = *n as u64;
            }
            if let Some(BoltValue::Integer(n)) = stats.get("constraints-added") {
                summary.constraints_added = *n as u64;
            }
        }

        if let Some(BoltValue::Integer(time)) =
            metadata.get("t_last").or_else(|| metadata.get("t_first"))
        {
            summary.execution_time_ms = *time as u64;
        }

        summary
    }

    /// Executes a parameterized query returning raw BoltValue records, projected columns, and summary.
    pub async fn execute_raw_records(
        &mut self,
        query: &str,
        params: &HashMap<String, BoltValue>,
    ) -> Result<(Vec<String>, Vec<Vec<BoltValue>>, QuerySummary)> {
        let (columns, records, summary, stream_err) = self
            .execute_raw_records_with_options(query, params, None, None, None, None)
            .await?;
        if let Some((code, msg)) = stream_err {
            return Err(NetError::ProtocolError(format!("{}: {}", code, msg)));
        }
        Ok((columns, records, summary))
    }

    /// Executes a parameterized query with advanced metadata options, returning records and stream errors.
    pub async fn execute_raw_records_with_options(
        &mut self,
        query: &str,
        params: &HashMap<String, BoltValue>,
        database: Option<&str>,
        tx_meta: Option<HashMap<String, BoltValue>>,
        timeout_ms: Option<i64>,
        bookmarks: Option<Vec<String>>,
    ) -> Result<(
        Vec<String>,
        Vec<Vec<BoltValue>>,
        QuerySummary,
        Option<(String, String)>,
    )> {
        if !self.is_valid {
            return Err(NetError::Closed(
                "Bolt connection is closed or broken".to_string(),
            ));
        }

        let mut extra = HashMap::new();
        if let Some(db) = database {
            extra.insert("db".to_string(), BoltValue::String(db.to_string()));
        }
        if let Some(meta) = tx_meta {
            extra.insert("tx_metadata".to_string(), BoltValue::Map(meta));
        }
        if let Some(timeout) = timeout_ms {
            extra.insert("tx_timeout".to_string(), BoltValue::Integer(timeout));
        }
        if let Some(bms) = bookmarks {
            let bm_vals: Vec<BoltValue> = bms.into_iter().map(BoltValue::String).collect();
            extra.insert("bookmarks".to_string(), BoltValue::List(bm_vals));
        }

        // 1. Pipeline RUN + PULL
        let run_req = BoltRequest::Run {
            query: query.to_string(),
            params: params.clone(),
            extra,
        };
        let pull_req = BoltRequest::pull_all();

        let mut write_buf = BytesMut::new();
        let mut run_payload = BytesMut::new();
        run_req.encode(&mut run_payload);
        encode_chunks(&run_payload, &mut write_buf);

        let mut pull_payload = BytesMut::new();
        pull_req.encode(&mut pull_payload);
        encode_chunks(&pull_payload, &mut write_buf);

        write_message_frame(&mut self.stream, &write_buf).await?;

        let mut collected_metadata = HashMap::new();

        // 2. Receive RUN response
        let run_resp = self.receive_response().await?;
        let columns: Vec<String> = match run_resp {
            BoltResponse::Success { ref metadata } => {
                collected_metadata.extend(metadata.clone());
                metadata
                    .get("fields")
                    .and_then(|v| v.as_list())
                    .map(|l| {
                        l.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<String>>()
                    })
                    .unwrap_or_default()
            }
            BoltResponse::Failure { metadata } => {
                let code = metadata
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Neo.ClientError.Statement.SyntaxError");
                let msg = metadata
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Query failed");
                let _ = self.receive_response().await;
                let _ = self.reset().await;
                return Err(NetError::ProtocolError(format!("{}: {}", code, msg)));
            }
            other => {
                return Err(NetError::ProtocolError(format!(
                    "Unexpected RUN response: {:?}",
                    other
                )));
            }
        };

        // 3. Receive stream of RECORDs
        let mut records = Vec::new();
        let mut stream_err = None;
        let summary = loop {
            let resp = self.receive_response().await?;
            match resp {
                BoltResponse::Record { fields } => {
                    records.push(fields);
                }
                BoltResponse::Success { metadata } => {
                    if let Some(BoltValue::String(bm)) = metadata.get("bookmark") {
                        self.last_bookmark = Some(bm.clone());
                    } else if let Some(BoltValue::List(bms)) = metadata.get("bookmarks")
                        && let Some(BoltValue::String(bm)) = bms.last()
                    {
                        self.last_bookmark = Some(bm.clone());
                    }
                    collected_metadata.extend(metadata.clone());
                    self.last_metadata = Some(collected_metadata);
                    break Self::parse_summary_metrics(&metadata);
                }
                BoltResponse::Failure { metadata } => {
                    let code = metadata
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("StreamError");
                    let msg = metadata
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Stream record failed");
                    let _ = self.reset().await;
                    stream_err = Some((code.to_string(), msg.to_string()));
                    break QuerySummary::default();
                }
                other => {
                    return Err(NetError::ProtocolError(format!(
                        "Unexpected stream response: {:?}",
                        other
                    )));
                }
            }
        };

        Ok((columns, records, summary, stream_err))
    }

    /// Begins an explicit transaction block.
    pub async fn begin_tx(&mut self, database: Option<&str>) -> Result<()> {
        self.begin_tx_with_options(database, None, None, None).await
    }

    /// Begins an explicit transaction block with optional configuration (bookmarks, metadata, timeout).
    pub async fn begin_tx_with_options(
        &mut self,
        database: Option<&str>,
        tx_meta: Option<HashMap<String, BoltValue>>,
        timeout_ms: Option<i64>,
        bookmarks: Option<Vec<String>>,
    ) -> Result<()> {
        let mut extra = HashMap::new();
        if let Some(db) = database {
            extra.insert("db".to_string(), BoltValue::String(db.to_string()));
        }
        if let Some(meta) = tx_meta {
            extra.insert("tx_metadata".to_string(), BoltValue::Map(meta));
        }
        if let Some(timeout) = timeout_ms {
            extra.insert("tx_timeout".to_string(), BoltValue::Integer(timeout));
        }
        if let Some(bms) = bookmarks {
            let bm_vals: Vec<BoltValue> = bms.into_iter().map(BoltValue::String).collect();
            extra.insert("bookmarks".to_string(), BoltValue::List(bm_vals));
        }
        self.send_request(&BoltRequest::Begin { extra }).await?;
        let resp = self.receive_response().await?;
        match resp {
            BoltResponse::Success { .. } => {
                self.in_transaction = true;
                Ok(())
            }
            BoltResponse::Failure { metadata } => {
                let code = metadata
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Neo.ClientError.Transaction.InvalidBookmark");
                let msg = metadata
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Begin transaction failed");
                let _ = self.reset().await;
                Err(NetError::ProtocolError(format!("{}: {}", code, msg)))
            }
            other => Err(NetError::ProtocolError(format!(
                "Unexpected BEGIN response: {:?}",
                other
            ))),
        }
    }

    /// Commits an open explicit transaction.
    pub async fn commit_tx(&mut self) -> Result<()> {
        self.send_request(&BoltRequest::Commit).await?;
        let resp = self.receive_response().await?;
        match resp {
            BoltResponse::Success { metadata } => {
                self.in_transaction = false;
                if let Some(BoltValue::String(bm)) = metadata.get("bookmark") {
                    self.last_bookmark = Some(bm.clone());
                } else if let Some(BoltValue::List(bms)) = metadata.get("bookmarks")
                    && let Some(BoltValue::String(bm)) = bms.last()
                {
                    self.last_bookmark = Some(bm.clone());
                }
                Ok(())
            }
            BoltResponse::Failure { metadata } => {
                self.in_transaction = false;
                let code = metadata
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Neo.ClientError.Transaction.TransactionHookFailed");
                let msg = metadata
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Commit transaction failed");
                let _ = self.reset().await;
                Err(NetError::ProtocolError(format!("{}: {}", code, msg)))
            }
            other => {
                self.in_transaction = false;
                Err(NetError::ProtocolError(format!(
                    "Unexpected COMMIT response: {:?}",
                    other
                )))
            }
        }
    }

    /// Aborts and rolls back an open explicit transaction.
    pub async fn rollback_tx(&mut self) -> Result<()> {
        self.send_request(&BoltRequest::Rollback).await?;
        let resp = self.receive_response().await?;
        match resp {
            BoltResponse::Success { .. } => {
                self.in_transaction = false;
                Ok(())
            }
            BoltResponse::Failure { metadata } => {
                self.in_transaction = false;
                let code = metadata
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Neo.ClientError.Transaction.TransactionHookFailed");
                let msg = metadata
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Rollback transaction failed");
                let _ = self.reset().await;
                Err(NetError::ProtocolError(format!("{}: {}", code, msg)))
            }
            other => {
                self.in_transaction = false;
                Err(NetError::ProtocolError(format!(
                    "Unexpected ROLLBACK response: {:?}",
                    other
                )))
            }
        }
    }
}

#[async_trait]
impl<S: AsyncRead + AsyncWrite + Unpin + Send + Sync> AsyncConnection for BoltConnection<S> {
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        if !self.is_valid {
            return Err(NetError::Closed(
                "Bolt connection is closed or broken".to_string(),
            ));
        }

        // Convert JSON params to BoltValue
        let bolt_params: HashMap<String, BoltValue> = params
            .iter()
            .map(|(k, v)| (k.clone(), BoltValue::from(v)))
            .collect();

        // 1. Pipeline RUN + PULL in a single write buffer
        let run_req = BoltRequest::run(query, bolt_params, None);
        let pull_req = BoltRequest::pull_all();

        let mut write_buf = BytesMut::new();

        let mut run_payload = BytesMut::new();
        run_req.encode(&mut run_payload);
        encode_chunks(&run_payload, &mut write_buf);

        let mut pull_payload = BytesMut::new();
        pull_req.encode(&mut pull_payload);
        encode_chunks(&pull_payload, &mut write_buf);

        // Send pipelined requests
        write_message_frame(&mut self.stream, &write_buf).await?;

        // 2. Receive RUN response (Expect SUCCESS with column fields)
        let run_resp = self.receive_response().await?;
        let columns: Vec<String> = match run_resp {
            BoltResponse::Success { ref metadata } => metadata
                .get("fields")
                .and_then(|v| v.as_list())
                .map(|l| {
                    l.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default(),
            BoltResponse::Failure { metadata } => {
                let code = metadata
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("QueryError");
                let msg = metadata
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Query failed");
                // Consume in-flight IGNORED response from pipelined PULL
                let _ = self.receive_response().await;
                // Issue RESET to clear failure state
                let _ = self.reset().await;
                return Err(NetError::ProtocolError(format!("{}: {}", code, msg)));
            }
            other => {
                return Err(NetError::ProtocolError(format!(
                    "Unexpected RUN response: {:?}",
                    other
                )));
            }
        };

        // 3. Receive stream of RECORD responses followed by final SUCCESS
        let mut records = Vec::new();

        let summary = loop {
            let resp = self.receive_response().await?;
            match resp {
                BoltResponse::Record { fields } => {
                    records.push(fields);
                }
                BoltResponse::Success { metadata } => {
                    break Self::parse_summary_metrics(&metadata);
                }
                BoltResponse::Failure { metadata } => {
                    let code = metadata
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("StreamError");
                    let msg = metadata
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Stream record failed");
                    let _ = self.reset().await;
                    return Err(NetError::ProtocolError(format!("{}: {}", code, msg)));
                }
                other => {
                    return Err(NetError::ProtocolError(format!(
                        "Unexpected stream response: {:?}",
                        other
                    )));
                }
            }
        };

        // 4. Build Arrow RecordBatch
        let batch = Self::build_record_batch(&columns, &records)?;
        let batches = if batch.num_rows() > 0 || !columns.is_empty() {
            vec![batch]
        } else {
            Vec::new()
        };

        Ok(QueryResult::new(columns, batches, summary))
    }

    async fn ping(&mut self) -> Result<()> {
        if !self.is_valid {
            return Err(NetError::Closed("Connection is closed".to_string()));
        }
        self.reset().await
    }

    async fn reset(&mut self) -> Result<()> {
        self.send_request(&BoltRequest::Reset).await?;
        let resp = self.receive_response().await?;
        match resp {
            BoltResponse::Success { .. } => {
                self.in_transaction = false;
                Ok(())
            }
            BoltResponse::Failure { metadata } => {
                self.is_valid = false;
                let msg = metadata
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Reset failed");
                Err(NetError::ProtocolError(format!("RESET failed: {}", msg)))
            }
            other => {
                self.is_valid = false;
                Err(NetError::ProtocolError(format!(
                    "Unexpected RESET response: {:?}",
                    other
                )))
            }
        }
    }

    fn is_valid(&self) -> bool {
        self.is_valid
    }

    fn is_in_transaction(&self) -> bool {
        self.in_transaction
    }

    async fn close(&mut self) -> Result<()> {
        if self.is_valid {
            let _ = self.send_request(&BoltRequest::Goodbye).await;
            self.is_valid = false;
        }
        Ok(())
    }
}
