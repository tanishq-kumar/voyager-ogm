//! PostgreSQL physical asynchronous connection engine implementing [`AsyncConnection`].

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;
use tokio::net::TcpStream;

use crate::config::Auth;
use crate::engine::{AsyncConnection, QueryResult, QuerySummary};
use crate::error::{NetError, Result};
use crate::postgres::agtype::rows_to_record_batch;
use crate::postgres::auth::{ScramClient, compute_md5_password};
use crate::postgres::codec::{read_backend_message, write_frontend_message, write_startup_message};
use crate::postgres::message::{
    AuthenticationRequest, BackendMessage, FrontendMessage, StartupMessage, TransactionStatus,
};
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
}

impl PostgresConnection {
    /// Asynchronously establishes a new connection to PostgreSQL / Apache AGE based on the parsed URI.
    pub async fn connect(uri: &ParsedUri) -> Result<Self> {
        let socket_addr = uri.socket_addr();
        let stream = TcpStream::connect(&socket_addr).await.map_err(|e| {
            NetError::ConnectionFailed(format!(
                "Failed to connect to PostgreSQL host at {}: {}",
                socket_addr, e
            ))
        })?;

        // Optimize latency: disable Nagle's algorithm
        let _ = stream.set_nodelay(true);

        let mut conn = Self {
            stream,
            uri: uri.clone(),
            server_parameters: HashMap::new(),
            backend_pid: 0,
            secret_key: 0,
            transaction_status: TransactionStatus::Idle,
            is_age_initialized: false,
        };

        // Perform protocol handshake & authentication
        conn.handshake().await?;

        // If target protocol or parameters indicate Apache AGE, initialize AGE extension
        if uri.raw.starts_with("age:")
            || uri.params.contains_key("graph")
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

    /// Returns the active transaction status reported by PostgreSQL.
    pub fn transaction_status(&self) -> TransactionStatus {
        self.transaction_status
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
        // If params are provided and query uses parameters, format or substitute parameters
        let formatted_query = if !params.is_empty() {
            let mut q = query.to_string();
            // Handle $param_name or %s substitutions if present
            for (k, v) in params {
                let json_repr = match v {
                    serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
                    serde_json::Value::Null => "NULL".to_string(),
                    other => other.to_string(),
                };
                let placeholder = format!("${}", k);
                q = q.replace(&placeholder, &json_repr);
            }
            q
        } else {
            query.to_string()
        };

        self.execute_simple(&formatted_query).await
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

    async fn close(&mut self) -> Result<()> {
        let _ = write_frontend_message(&mut self.stream, &FrontendMessage::Terminate).await;
        Ok(())
    }
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
