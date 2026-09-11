//! High-level unified asynchronous client with connection pooling across Bolt, PostgreSQL, and Redis/FalkorDB.

use std::collections::HashMap;

use crate::bolt::BoltConnection;
use crate::config::{ConnectionConfig, PoolConfig};
use crate::engine::{AsyncConnection, QueryResult};
use crate::error::{NetError, Result};
use crate::pool::ConnectionPool;
use crate::postgres::PostgresConnection;
use crate::redis::RedisConnection;
use crate::uri::{DatabaseProtocol, ParsedUri};

/// Asynchronously connects to any supported database protocol based on the URI scheme.
pub async fn connect_any(uri: &str) -> Result<Box<dyn AsyncConnection>> {
    let parsed = ParsedUri::parse(uri)?;
    if parsed.tls != crate::config::TlsMode::Disabled {
        return Err(NetError::TlsError(format!(
            "TLS encryption requested for '{}' ({:?}), but native TLS socket negotiation is not yet configured on this endpoint. Plaintext downgrade rejected for security.",
            parsed.host, parsed.tls
        )));
    }
    match parsed.protocol {
        DatabaseProtocol::Bolt => {
            let config = ConnectionConfig::from_uri(uri);
            let conn = BoltConnection::connect(&config).await?;
            Ok(Box::new(conn))
        }
        DatabaseProtocol::Postgres => {
            let conn = PostgresConnection::connect(&parsed).await?;
            Ok(Box::new(conn))
        }
        DatabaseProtocol::Redis => {
            let conn = RedisConnection::connect(&parsed).await?;
            Ok(Box::new(conn))
        }
        DatabaseProtocol::DuckDb => Err(NetError::ProtocolError(
            "Embedded DuckDB does not use TCP network connections; use voyager's DuckDbBridge instead"
                .to_string(),
        )),
    }
}

/// Unified high-performance database client managing a pooled connection.
#[derive(Clone)]
pub struct NativeClient {
    pool: ConnectionPool<Box<dyn AsyncConnection>>,
    uri: String,
    protocol: DatabaseProtocol,
}

impl NativeClient {
    /// Creates a new `NativeClient` connecting to the given URI.
    pub fn new(uri: &str, pool_config: Option<PoolConfig>) -> Result<Self> {
        let parsed = ParsedUri::parse(uri)?;
        let protocol = parsed.protocol;
        let uri_owned = uri.to_string();
        let uri_factory = uri_owned.clone();
        let config = pool_config.unwrap_or_default();

        let pool = ConnectionPool::from_fn(config, move || {
            let u = uri_factory.clone();
            async move { connect_any(&u).await }
        });

        Ok(Self {
            pool,
            uri: uri_owned,
            protocol,
        })
    }

    /// Returns the target connection URI.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Returns the active database protocol.
    pub fn protocol(&self) -> DatabaseProtocol {
        self.protocol
    }

    /// Returns a reference to the underlying connection pool.
    pub fn pool(&self) -> &ConnectionPool<Box<dyn AsyncConnection>> {
        &self.pool
    }

    /// Executes a parameterized query using an automatically leased connection from the pool.
    pub async fn execute(
        &self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        let mut conn = self.pool.acquire().await?;
        conn.execute(query, params).await
    }

    /// Sends a lightweight liveness probe ping to the database.
    pub async fn ping(&self) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        conn.ping().await
    }

    /// Closes the connection pool and evicts all active connections.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}
