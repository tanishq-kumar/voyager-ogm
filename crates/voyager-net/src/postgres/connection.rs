//! PostgreSQL physical asynchronous connection engine implementing [`AsyncConnection`].

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;
use tokio::io::AsyncRead;
use tokio::net::TcpStream;

use crate::config::Auth;
use crate::engine::{AsyncConnection, QueryResult, QuerySummary};
use crate::error::{NetError, Result};
use crate::postgres::agtype::rows_to_record_batch;
use crate::postgres::auth::{ScramClient, compute_md5_password};
use crate::postgres::codec::{
    read_backend_message, write_frontend_message, write_frontend_messages, write_startup_message,
};
use crate::postgres::message::{
    AuthenticationRequest, BackendMessage, FrontendMessage, StartupMessage, TransactionStatus,
};
use crate::postgres::params::{PreparedPostgresStatement, prepare_postgres_statement};
use crate::uri::ParsedUri;

/// Asynchronous physical connection to a PostgreSQL database server or Apache AGE instance.
pub struct PostgresConnection {
    stream: TcpStream,
    uri: ParsedUri,
    server_parameters: HashMap<String, String>,
    backend_pid: i32,
    secret_key: i32,
    transaction_status: TransactionStatus,
    is_age_initialized: bool,
    query_timeout: Option<std::time::Duration>,
    last_command_tag: Option<String>,
}

impl PostgresConnection {
    /// Asynchronously establishes a new connection to PostgreSQL / Apache AGE based on the parsed URI.
    pub async fn connect(uri: &ParsedUri) -> Result<Self> {
        if uri.tls != crate::config::TlsMode::Disabled {
            return Err(NetError::TlsError(format!(
                "TLS encryption requested for PostgreSQL endpoint '{}', but native TLS transport is not enabled. Plaintext downgrade rejected.",
                uri.host
            )));
        }
        let timeout_secs = uri
            .params
            .get("connect_timeout")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5);
        let timeout_dur = std::time::Duration::from_secs(timeout_secs);
        match tokio::time::timeout(timeout_dur, Self::connect_raw(uri)).await {
            Ok(res) => res,
            Err(_) => Err(NetError::Timeout(format!(
                "Connection or handshake to PostgreSQL host at {} timed out after {:?}",
                uri.socket_addr(),
                timeout_dur
            ))),
        }
    }

    async fn connect_raw(uri: &ParsedUri) -> Result<Self> {
        let socket_addr = uri.socket_addr();
        let stream = TcpStream::connect(&socket_addr).await.map_err(|e| {
            NetError::ConnectionFailed(format!(
                "Failed to connect to PostgreSQL host at {}: {}",
                socket_addr, e
            ))
        })?;

        // Optimize latency: disable Nagle's algorithm
        let _ = stream.set_nodelay(true);

        let query_timeout = uri
            .params
            .get("query_timeout")
            .and_then(|s| s.parse::<u64>().ok())
            .map(std::time::Duration::from_secs)
            .or(Some(std::time::Duration::from_secs(30)));

        let mut conn = Self {
            stream,
            uri: uri.clone(),
            server_parameters: HashMap::new(),
            backend_pid: 0,
            secret_key: 0,
            transaction_status: TransactionStatus::Idle,
            is_age_initialized: false,
            query_timeout,
            last_command_tag: None,
        };

        // Perform protocol handshake & authentication
        conn.handshake().await?;

        // If target protocol or parameters indicate Apache AGE, initialize AGE extension
        if uri.raw.starts_with("age:")
            || uri.params.contains_key("graph")
            || uri.params.contains_key("graph_name")
            || uri.params.contains_key("age")
            || uri.params.get("dialect").map(|s| s.as_str()) == Some("age")
        {
            conn.init_age().await?;
        }

        Ok(conn)
    }

    /// Performs PostgreSQL Frontend/Backend Protocol v3.0 startup and authentication handshake.
    async fn handshake(&mut self) -> Result<()> {
        let username = match &self.uri.auth {
            Auth::Basic { username, .. } => username.clone(),
            _ => "postgres".to_string(),
        };

        let password = match &self.uri.auth {
            Auth::Basic { password, .. } => password.clone(),
            _ => String::new(),
        };

        let database = self.uri.database.as_deref().unwrap_or("postgres");
        let startup = StartupMessage::new(&username, Some(database));

        write_startup_message(&mut self.stream, &startup).await?;

        let mut scram_client: Option<ScramClient> = None;

        loop {
            let msg = read_backend_message(&mut self.stream).await?;
            match msg {
                BackendMessage::Authentication(auth_req) => match auth_req {
                    AuthenticationRequest::Ok => {
                        // Authentication succeeded, continue waiting for ReadyForQuery
                    }
                    AuthenticationRequest::CleartextPassword => {
                        let mut pwd_bytes = password.as_bytes().to_vec();
                        pwd_bytes.push(0);
                        write_frontend_message(
                            &mut self.stream,
                            &FrontendMessage::Password(pwd_bytes),
                        )
                        .await?;
                    }
                    AuthenticationRequest::MD5Password { salt } => {
                        let md5_bytes = compute_md5_password(&username, &password, &salt);
                        write_frontend_message(
                            &mut self.stream,
                            &FrontendMessage::Password(md5_bytes),
                        )
                        .await?;
                    }
                    AuthenticationRequest::SASL { mechanisms } => {
                        if !mechanisms.iter().any(|m| m == "SCRAM-SHA-256") {
                            return Err(NetError::AuthenticationFailed(format!(
                                "No supported SASL mechanism offered by server: {:?}",
                                mechanisms
                            )));
                        }
                        let client = ScramClient::new();
                        let first_msg = client.client_first_message();
                        scram_client = Some(client);

                        write_frontend_message(
                            &mut self.stream,
                            &FrontendMessage::SASLInitialResponse {
                                mechanism: "SCRAM-SHA-256".to_string(),
                                data: Some(first_msg),
                            },
                        )
                        .await?;
                    }
                    AuthenticationRequest::SASLContinue(challenge) => {
                        let client = scram_client.as_mut().ok_or_else(|| {
                            NetError::AuthenticationFailed("SCRAM client state missing".to_string())
                        })?;
                        let response = client.process_challenge(&challenge, &password)?;
                        write_frontend_message(
                            &mut self.stream,
                            &FrontendMessage::SASLResponse(response),
                        )
                        .await?;
                    }
                    AuthenticationRequest::SASLFinal(final_data) => {
                        let client = scram_client.as_ref().ok_or_else(|| {
                            NetError::AuthenticationFailed("SCRAM client state missing".to_string())
                        })?;
                        client.verify_server_final(&final_data)?;
                    }
                    other => {
                        return Err(NetError::AuthenticationFailed(format!(
                            "Unsupported authentication request: {:?}",
                            other
                        )));
                    }
                },
                BackendMessage::ParameterStatus { name, value } => {
                    self.server_parameters.insert(name, value);
                }
                BackendMessage::BackendKeyData {
                    process_id,
                    secret_key,
                } => {
                    self.backend_pid = process_id;
                    self.secret_key = secret_key;
                }
                BackendMessage::ReadyForQuery(status) => {
                    self.transaction_status = status;
                    break;
                }
                BackendMessage::ErrorResponse(diag) => {
                    return Err(NetError::AuthenticationFailed(diag.format_error()));
                }
                BackendMessage::NoticeResponse(_) => {
                    // Ignore notices during handshake
                }
                other => {
                    return Err(NetError::ProtocolError(format!(
                        "Unexpected backend message during startup handshake: {:?}",
                        other
                    )));
                }
            }
        }

        Ok(())
    }

    /// Initializes Apache AGE extension and search path if not already loaded.
    pub async fn init_age(&mut self) -> Result<()> {
        if self.is_age_initialized {
            return Ok(());
        }

        // Execute LOAD 'age'; SET search_path = ag_catalog, "$user", public;
        self.execute_simple("LOAD 'age'; SET search_path = ag_catalog, \"$user\", public;")
            .await?;
        self.is_age_initialized = true;
        Ok(())
    }

    /// Executes a simple query string via PostgreSQL Simple Query Protocol (`'Q'`).
    pub async fn execute_simple(&mut self, query: &str) -> Result<QueryResult> {
        if let Some(to) = self.query_timeout {
            match tokio::time::timeout(to, self.execute_simple_inner(query)).await {
                Ok(res) => res,
                Err(_) => {
                    self.transaction_status = TransactionStatus::FailedTransaction;
                    Err(NetError::Timeout(format!(
                        "Query execution timed out after {:?}",
                        to
                    )))
                }
            }
        } else {
            self.execute_simple_inner(query).await
        }
    }

    async fn execute_simple_inner(&mut self, query: &str) -> Result<QueryResult> {
        let start_time = Instant::now();
        write_frontend_message(&mut self.stream, &FrontendMessage::Query(query.to_string()))
            .await?;

        let mut columns = Vec::new();
        let mut rows: Vec<Vec<Option<String>>> = Vec::new();
        let mut summary = QuerySummary::empty();
        let mut query_error = None;

        loop {
            let msg = read_backend_message(&mut self.stream).await?;
            match msg {
                BackendMessage::RowDescription(fields) => {
                    columns = fields.into_iter().map(|f| f.name).collect();
                }
                BackendMessage::DataRow(vals) => {
                    let mut row = Vec::with_capacity(vals.len());
                    for val in vals {
                        match val {
                            Some(bytes) => {
                                let s = String::from_utf8_lossy(&bytes).into_owned();
                                row.push(Some(s));
                            }
                            None => row.push(None),
                        }
                    }
                    rows.push(row);
                }
                BackendMessage::CommandComplete(tag) => {
                    self.last_command_tag = Some(tag.clone());
                    parse_command_tag(&tag, &mut summary);
                }
                BackendMessage::ReadyForQuery(status) => {
                    self.transaction_status = status;
                    break;
                }
                BackendMessage::ErrorResponse(diag) => {
                    query_error = Some(NetError::ProtocolError(diag.format_error()));
                }
                BackendMessage::EmptyQueryResponse => {}
                BackendMessage::NoticeResponse(_) => {}
                BackendMessage::ParameterStatus { name, value } => {
                    self.server_parameters.insert(name, value);
                }
                other => {
                    tracing::trace!("Ignored message during simple query: {:?}", other);
                }
            }
        }

        if let Some(err) = query_error {
            return Err(err);
        }

        summary.execution_time_ms = start_time.elapsed().as_millis() as u64;

        let batch = rows_to_record_batch(&columns, &rows)?;
        let batches = if batch.num_rows() > 0 || !columns.is_empty() {
            vec![batch]
        } else {
            Vec::new()
        };

        Ok(QueryResult::new(columns, batches, summary))
    }

    /// Executes a parameterized statement via the PostgreSQL extended query protocol.
    ///
    /// Sends `Parse` / `Bind` / `Describe` / `Execute` / `Sync` in one flush and
    /// drains responses through `ReadyForQuery`, including after an
    /// `ErrorResponse`, so the connection remains usable and transaction state
    /// reflects the server's report.
    async fn execute_extended_inner(
        &mut self,
        prepared: &PreparedPostgresStatement,
    ) -> Result<QueryResult> {
        let start_time = Instant::now();
        let parameter_count = prepared.parameter_values.len();

        let messages = [
            FrontendMessage::Parse {
                name: String::new(),
                query: prepared.statement.clone(),
                param_types: prepared.parameter_oids.clone(),
            },
            FrontendMessage::Bind {
                portal: String::new(),
                statement: String::new(),
                param_formats: vec![0; parameter_count],
                params: prepared.parameter_values.clone(),
                result_formats: vec![0],
            },
            FrontendMessage::Describe {
                target_type: b'P',
                name: String::new(),
            },
            FrontendMessage::Execute {
                portal: String::new(),
                max_rows: 0,
            },
            FrontendMessage::Sync,
        ];
        write_frontend_messages(&mut self.stream, &messages).await?;

        let response = read_extended_response(&mut self.stream).await?;
        self.transaction_status = response.transaction_status;
        for (name, value) in response.parameter_status {
            self.server_parameters.insert(name, value);
        }
        if let Some(tag) = response.command_tag {
            self.last_command_tag = Some(tag);
        }

        if let Some(err) = response.error {
            return Err(err);
        }

        let mut summary = response.summary;
        summary.execution_time_ms = start_time.elapsed().as_millis() as u64;

        let batch = rows_to_record_batch(&response.columns, &response.rows)?;
        let batches = if batch.num_rows() > 0 || !response.columns.is_empty() {
            vec![batch]
        } else {
            Vec::new()
        };

        Ok(QueryResult::new(response.columns, batches, summary))
    }

    /// Executes a prepared statement with the configured query timeout.
    async fn execute_extended(
        &mut self,
        prepared: &PreparedPostgresStatement,
    ) -> Result<QueryResult> {
        if let Some(to) = self.query_timeout {
            match tokio::time::timeout(to, self.execute_extended_inner(prepared)).await {
                Ok(res) => res,
                Err(_) => {
                    self.transaction_status = TransactionStatus::FailedTransaction;
                    Err(NetError::Timeout(format!(
                        "Query execution timed out after {:?}",
                        to
                    )))
                }
            }
        } else {
            self.execute_extended_inner(prepared).await
        }
    }

    /// Returns the active query timeout.
    pub fn query_timeout(&self) -> Option<std::time::Duration> {
        self.query_timeout
    }

    /// Sets the query timeout for this connection.
    pub fn set_query_timeout(&mut self, timeout: Option<std::time::Duration>) {
        self.query_timeout = timeout;
    }

    /// Returns the active transaction status reported by PostgreSQL.
    pub fn transaction_status(&self) -> TransactionStatus {
        self.transaction_status
    }

    /// Returns the command tag of the most recently completed query.
    pub fn last_command_tag(&self) -> Option<&str> {
        self.last_command_tag.as_deref()
    }

    /// Returns the server parameters collected during handshake.
    pub fn server_parameters(&self) -> &HashMap<String, String> {
        &self.server_parameters
    }

    /// Returns the backend process ID.
    pub fn backend_pid(&self) -> i32 {
        self.backend_pid
    }
}

#[async_trait]
impl AsyncConnection for PostgresConnection {
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        let prepared = prepare_postgres_statement(query, params)?;
        if !prepared.is_parameterized() {
            return self.execute_simple(&prepared.statement).await;
        }
        self.execute_extended(&prepared).await
    }

    async fn ping(&mut self) -> Result<()> {
        self.execute_simple("SELECT 1;").await?;
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        if self.transaction_status != TransactionStatus::Idle {
            let _ = self.execute_simple("ROLLBACK;").await;
        }
        Ok(())
    }

    fn is_valid(&self) -> bool {
        self.transaction_status != TransactionStatus::FailedTransaction
    }

    fn is_in_transaction(&self) -> bool {
        self.transaction_status == TransactionStatus::InTransaction
    }

    fn can_rollback(&self) -> bool {
        self.transaction_status != TransactionStatus::Idle
    }

    async fn close(&mut self) -> Result<()> {
        let _ = write_frontend_message(&mut self.stream, &FrontendMessage::Terminate).await;
        Ok(())
    }

    async fn begin_transaction(&mut self) -> Result<()> {
        // The AGE setup (`LOAD 'age'; SET search_path = ...`) is executed as a
        // separate simple query before this point, so it is committed and cannot
        // be undone by rolling back this explicit transaction.
        if self.is_age_initialized {
            self.init_age().await?;
        }
        self.execute_simple("BEGIN;").await?;
        Ok(())
    }

    async fn commit_transaction(&mut self) -> Result<()> {
        if self.transaction_status == TransactionStatus::FailedTransaction {
            return Err(NetError::TransactionError(
                "transaction is aborted; roll back instead of committing".to_string(),
            ));
        }
        self.last_command_tag = None;
        self.execute_simple("COMMIT;").await?;
        match self.last_command_tag.as_deref() {
            Some(tag) if tag.eq_ignore_ascii_case("COMMIT") => Ok(()),
            Some(tag) if tag.eq_ignore_ascii_case("ROLLBACK") => Err(NetError::TransactionError(
                "transaction is aborted; PostgreSQL converted COMMIT into ROLLBACK".to_string(),
            )),
            other => Err(NetError::ProtocolError(format!(
                "Unexpected command tag after COMMIT: {other:?}"
            ))),
        }
    }

    async fn rollback_transaction(&mut self) -> Result<()> {
        self.execute_simple("ROLLBACK;").await?;
        Ok(())
    }

    async fn create_savepoint(&mut self, name: &str) -> Result<()> {
        validate_savepoint_name(name)?;
        self.execute_simple(&format!("SAVEPOINT {};", name)).await?;
        Ok(())
    }

    async fn rollback_to_savepoint(&mut self, name: &str) -> Result<()> {
        validate_savepoint_name(name)?;
        self.execute_simple(&format!("ROLLBACK TO SAVEPOINT {};", name))
            .await?;
        Ok(())
    }

    async fn release_savepoint(&mut self, name: &str) -> Result<()> {
        validate_savepoint_name(name)?;
        self.execute_simple(&format!("RELEASE SAVEPOINT {};", name))
            .await?;
        Ok(())
    }

    fn supports_savepoints(&self) -> bool {
        true
    }
}

/// Validates that a savepoint name is a safe unquoted SQL identifier.
pub(crate) fn validate_savepoint_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '_')
        .unwrap_or(false);
    let valid_rest = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid_start && valid_rest {
        Ok(())
    } else {
        Err(NetError::TransactionError(format!(
            "Invalid savepoint name '{name}'; expected an unquoted SQL identifier"
        )))
    }
}

/// Fully drained response to an extended-query cycle.
struct ExtendedResponse {
    columns: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
    summary: QuerySummary,
    command_tag: Option<String>,
    error: Option<NetError>,
    transaction_status: TransactionStatus,
    parameter_status: Vec<(String, String)>,
}

/// Reads extended-query responses until `ReadyForQuery`, even after an error.
async fn read_extended_response<R>(reader: &mut R) -> Result<ExtendedResponse>
where
    R: AsyncRead + Unpin,
{
    let mut response = ExtendedResponse {
        columns: Vec::new(),
        rows: Vec::new(),
        summary: QuerySummary::empty(),
        command_tag: None,
        error: None,
        transaction_status: TransactionStatus::Idle,
        parameter_status: Vec::new(),
    };

    loop {
        let msg = read_backend_message(reader).await?;
        match msg {
            BackendMessage::ParseComplete
            | BackendMessage::BindComplete
            | BackendMessage::CloseComplete
            | BackendMessage::NoData
            | BackendMessage::EmptyQueryResponse => {}
            BackendMessage::RowDescription(fields) => {
                response.columns = fields.into_iter().map(|f| f.name).collect();
            }
            BackendMessage::DataRow(vals) => {
                let mut row = Vec::with_capacity(vals.len());
                for val in vals {
                    match val {
                        Some(bytes) => {
                            let s = String::from_utf8_lossy(&bytes).into_owned();
                            row.push(Some(s));
                        }
                        None => row.push(None),
                    }
                }
                response.rows.push(row);
            }
            BackendMessage::CommandComplete(tag) => {
                parse_command_tag(&tag, &mut response.summary);
                response.command_tag = Some(tag);
            }
            BackendMessage::ReadyForQuery(status) => {
                response.transaction_status = status;
                break;
            }
            BackendMessage::ErrorResponse(diag) => {
                // Keep draining until ReadyForQuery: the server discards the
                // rest of this extended-protocol cycle and reports the final
                // transaction state there.
                if response.error.is_none() {
                    response.error = Some(NetError::ProtocolError(diag.format_error()));
                }
            }
            BackendMessage::NoticeResponse(_) => {}
            BackendMessage::ParameterStatus { name, value } => {
                response.parameter_status.push((name, value));
            }
            other => {
                tracing::trace!("Ignored message during extended query: {:?}", other);
            }
        }
    }

    Ok(response)
}

/// Parses PostgreSQL command tag (e.g. `INSERT 0 1`, `DELETE 5`, `UPDATE 2`) into summary metrics.
fn parse_command_tag(tag: &str, summary: &mut QuerySummary) {
    let parts: Vec<&str> = tag.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    match parts[0] {
        "INSERT" => {
            if let Some(count_str) = parts.get(2)
                && let Ok(c) = count_str.parse::<u64>()
            {
                summary.nodes_created += c;
            }
        }
        "DELETE" => {
            if let Some(count_str) = parts.get(1)
                && let Ok(c) = count_str.parse::<u64>()
            {
                summary.nodes_deleted += c;
            }
        }
        "UPDATE" => {
            if let Some(count_str) = parts.get(1)
                && let Ok(c) = count_str.parse::<u64>()
            {
                summary.properties_set += c;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, duplex};

    fn frame(msg_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + payload.len());
        out.push(msg_type);
        out.extend_from_slice(&(4 + payload.len() as i32).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn error_frame(message: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"SERROR\0");
        payload.extend_from_slice(b"C42601\0");
        payload.push(b'M');
        payload.extend_from_slice(message.as_bytes());
        payload.push(0);
        payload.push(0);
        frame(b'E', &payload)
    }

    fn ready_frame(status: u8) -> Vec<u8> {
        frame(b'Z', &[status])
    }

    #[tokio::test]
    async fn extended_error_drains_to_ready_for_query_and_recovers() {
        let (mut client, mut server) = duplex(4096);

        let mut first_cycle = Vec::new();
        first_cycle.extend_from_slice(&frame(b'1', &[])); // ParseComplete
        first_cycle.extend_from_slice(&frame(b'2', &[])); // BindComplete
        first_cycle.extend_from_slice(&error_frame("syntax error at or near \"%\""));
        first_cycle.extend_from_slice(&ready_frame(b'E')); // failed transaction
        server.write_all(&first_cycle).await.unwrap();

        let first = read_extended_response(&mut client).await.unwrap();
        assert!(first.error.is_some());
        assert!(
            first
                .error
                .as_ref()
                .unwrap()
                .to_string()
                .contains("syntax error")
        );
        assert_eq!(
            first.transaction_status,
            TransactionStatus::FailedTransaction
        );
        assert!(first.columns.is_empty());

        let mut second_cycle = Vec::new();
        // RowDescription with one text column "n"
        let mut row_desc = Vec::new();
        row_desc.extend_from_slice(&1i16.to_be_bytes());
        row_desc.extend_from_slice(b"n\0");
        row_desc.extend_from_slice(&0i32.to_be_bytes()); // table oid
        row_desc.extend_from_slice(&0i16.to_be_bytes()); // column attr
        row_desc.extend_from_slice(&25i32.to_be_bytes()); // text
        row_desc.extend_from_slice(&(-1i16).to_be_bytes()); // varlena
        row_desc.extend_from_slice(&(-1i32).to_be_bytes()); // typmod
        row_desc.extend_from_slice(&0i16.to_be_bytes()); // text format
        second_cycle.extend_from_slice(&frame(b'T', &row_desc));

        let mut data_row = Vec::new();
        data_row.extend_from_slice(&1i16.to_be_bytes());
        data_row.extend_from_slice(&3i32.to_be_bytes());
        data_row.extend_from_slice(b"one");
        second_cycle.extend_from_slice(&frame(b'D', &data_row));
        second_cycle.extend_from_slice(&frame(b'C', b"SELECT 1\0"));
        second_cycle.extend_from_slice(&ready_frame(b'I'));
        server.write_all(&second_cycle).await.unwrap();

        let second = read_extended_response(&mut client).await.unwrap();
        assert!(second.error.is_none());
        assert_eq!(second.transaction_status, TransactionStatus::Idle);
        assert_eq!(second.columns, vec!["n".to_string()]);
        assert_eq!(second.rows, vec![vec![Some("one".to_string())]]);
        assert_eq!(second.command_tag.as_deref(), Some("SELECT 1"));
    }
}
