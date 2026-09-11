//! Configuration options for connection pools, TLS, authentication, and database engines.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::uri::ParsedUri;

/// Authentication schemes supported by database wire protocols.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Auth {
    /// No authentication credentials.
    #[default]
    None,
    /// Standard username and password authentication with optional realm/database.
    Basic {
        /// Username identifier.
        username: String,
        /// Password secret.
        password: String,
        /// Optional authentication realm (e.g. for Neo4j multi-realm setups).
        realm: Option<String>,
    },
    /// Bearer token / JWT authentication.
    Bearer {
        /// Secret token string.
        token: String,
    },
}

/// Transport Layer Security (TLS) configuration modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TlsMode {
    /// Plaintext TCP connection without TLS encryption.
    #[default]
    Disabled,
    /// TLS encryption with strict system certificate authority verification.
    Required,
    /// TLS encryption with self-signed certificate acceptance (useful for local development).
    SelfSigned,
}

/// Asynchronous connection pool configuration parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Minimum number of idle connections maintained in the pool. Default is 1.
    pub min_idle: usize,
    /// Maximum number of active and idle connections allowed in the pool. Default is 10.
    pub max_size: usize,
    /// Maximum time to wait when acquiring a connection before returning a `PoolExhausted` error. Default is 30s.
    pub acquire_timeout: Duration,
    /// Maximum time a connection can remain idle in the pool before being evicted. Default is 10 minutes.
    pub idle_timeout: Option<Duration>,
    /// Maximum total lifetime of a connection from creation to retirement. Default is 30 minutes.
    pub max_lifetime: Option<Duration>,
    /// Interval between background health-check and idle-eviction sweeps. Default is 30 seconds.
    pub health_check_interval: Duration,
    /// Whether to perform a liveness probe (ping/RESET) on a connection before leasing. Default is true.
    pub test_on_acquire: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_idle: 1,
            max_size: 10,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Some(Duration::from_secs(600)),
            max_lifetime: Some(Duration::from_secs(1800)),
            health_check_interval: Duration::from_secs(30),
            test_on_acquire: true,
        }
    }
}

impl PoolConfig {
    /// Creates a new `PoolConfig` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum connection pool capacity.
    pub fn with_max_size(mut self, size: usize) -> Self {
        self.max_size = size;
        self
    }

    /// Sets the minimum number of idle connections.
    pub fn with_min_idle(mut self, min: usize) -> Self {
        self.min_idle = min;
        self
    }

    /// Sets the connection acquisition timeout.
    pub fn with_acquire_timeout(mut self, timeout: Duration) -> Self {
        self.acquire_timeout = timeout;
        self
    }

    /// Sets the idle connection eviction timeout.
    pub fn with_idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Sets the maximum connection lifetime before recycling.
    pub fn with_max_lifetime(mut self, lifetime: Option<Duration>) -> Self {
        self.max_lifetime = lifetime;
        self
    }
}

/// Comprehensive connection options for an engine instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Target connection URI (e.g. `bolt://localhost:7687` or `postgresql://localhost:5455/postgres`).
    pub uri: String,
    /// Authentication credentials.
    pub auth: Auth,
    /// Target database catalog or graph name.
    pub database: Option<String>,
    /// Transport security mode.
    pub tls: TlsMode,
    /// Physical TCP socket connection timeout. Default is 10s.
    pub connect_timeout: Duration,
    /// Maximum execution timeout for any single query on the wire. Default is 30s.
    pub query_timeout: Option<Duration>,
    /// Asynchronous connection pool settings.
    pub pool: PoolConfig,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            uri: "bolt://localhost:7687".to_string(),
            auth: Auth::None,
            database: None,
            tls: TlsMode::Disabled,
            connect_timeout: Duration::from_secs(10),
            query_timeout: Some(Duration::from_secs(30)),
            pool: PoolConfig::default(),
        }
    }
}

impl ConnectionConfig {
    /// Creates a connection config from a URI string, extracting credentials, database, and timeout parameters.
    pub fn from_uri(uri: impl Into<String>) -> Self {
        let uri_str = uri.into();
        let mut config = Self {
            uri: uri_str.clone(),
            ..Default::default()
        };
        if let Ok(parsed) = ParsedUri::parse(&uri_str) {
            config.auth = parsed.auth;
            config.database = parsed.database;
            config.tls = parsed.tls;
            if let Some(t) = parsed
                .params
                .get("connect_timeout")
                .and_then(|s| s.parse::<u64>().ok())
            {
                config.connect_timeout = Duration::from_secs(t);
            }
            if let Some(t) = parsed
                .params
                .get("query_timeout")
                .and_then(|s| s.parse::<u64>().ok())
            {
                config.query_timeout = Some(Duration::from_secs(t));
            }
        }
        config
    }

    /// Sets basic username and password authentication.
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.auth = Auth::Basic {
            username: username.into(),
            password: password.into(),
            realm: None,
        };
        self
    }

    /// Sets the target database or graph name.
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Sets the query execution timeout.
    pub fn with_query_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.query_timeout = timeout;
        self
    }

    /// Sets the pool configuration.
    pub fn with_pool(mut self, pool: PoolConfig) -> Self {
        self.pool = pool;
        self
    }
}
