//! Database connection URI parsing and protocol resolution for Voyager OGM.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use url::Url;

use crate::config::{Auth, TlsMode};
use crate::error::{NetError, Result};

/// Supported graph and relational wire database protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseProtocol {
    /// Neo4j / Memgraph Bolt binary wire protocol (`bolt://`, `neo4j://`).
    Bolt,
    /// PostgreSQL Frontend/Backend v3.0 wire protocol (`postgresql://`, `postgres://`).
    Postgres,
    /// Redis RESP2/RESP3 wire protocol for FalkorDB (`redis://`, `rediss://`, `falkordb://`).
    Redis,
    /// In-memory / embedded C-ABI DuckDB engine (`duckdb://`).
    DuckDb,
}

impl DatabaseProtocol {
    /// Returns the standard default TCP port for the protocol if none is specified in the URI.
    pub fn default_port(&self) -> Option<u16> {
        match self {
            Self::Bolt => Some(7687),
            Self::Postgres => Some(5432),
            Self::Redis => Some(6379),
            Self::DuckDb => None,
        }
    }

    /// Returns whether this protocol requires a network socket connection.
    pub fn is_network(&self) -> bool {
        match self {
            Self::Bolt | Self::Postgres | Self::Redis => true,
            Self::DuckDb => false,
        }
    }
}

impl fmt::Display for DatabaseProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bolt => write!(f, "Bolt"),
            Self::Postgres => write!(f, "PostgreSQL"),
            Self::Redis => write!(f, "Redis/FalkorDB"),
            Self::DuckDb => write!(f, "DuckDB"),
        }
    }
}

/// Fully parsed and normalized connection URI descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUri {
    /// Resolved database protocol.
    pub protocol: DatabaseProtocol,
    /// Hostname or IP address (e.g. `localhost`, `127.0.0.1`, `db.example.com`).
    pub host: String,
    /// Resolved TCP port.
    pub port: Option<u16>,
    /// Extracted authentication credentials if embedded in the URI.
    pub auth: Auth,
    /// Target database catalog or graph name (e.g. `neo4j`, `postgres`, `social_graph`).
    pub database: Option<String>,
    /// Resolved TLS security mode.
    pub tls: TlsMode,
    /// Extra query parameters provided in the URI (e.g. `?routing=true&timeout=5s`).
    pub params: HashMap<String, String>,
    /// Optional backend preference extracted from prefix (e.g. `voyager+native:` or `voyager+bridge:`).
    pub backend_hint: Option<String>,
    /// Raw unparsed connection string.
    pub raw: String,
}

impl ParsedUri {
    /// Parses a raw connection URI string into a structured `ParsedUri`.
    ///
    /// # Supported Schemes & Optional Prefixes
    /// - Standard: `bolt://`, `neo4j://`, `memgraph://`, `postgresql://`, `age://`, `redis://`, `falkordb://`, `duckdb://`
    /// - Optional JDBC/SQLAlchemy-style namespace prefixes:
    ///   - `voyager:bolt://...`
    ///   - `voyager+native:bolt://...` (explicit native Rust engine hint)
    ///   - `voyager+bridge:bolt://...` (explicit Python driver bridge hint)
    ///   - `vn:bolt://...` (shorthand for voyager-net)
    pub fn parse(raw_uri: &str) -> Result<Self> {
        let mut trimmed = raw_uri.trim();
        if trimmed.is_empty() {
            return Err(NetError::InvalidUri(
                "Connection URI cannot be empty".to_string(),
            ));
        }

        // Handle optional JDBC/SQLAlchemy-style namespace prefix
        let mut backend_hint = None;
        if let Some(rest) = trimmed.strip_prefix("voyager+native:") {
            backend_hint = Some("native".to_string());
            trimmed = rest.trim();
        } else if let Some(rest) = trimmed.strip_prefix("voyager+bridge:") {
            backend_hint = Some("bridge".to_string());
            trimmed = rest.trim();
        } else if let Some(rest) = trimmed.strip_prefix("voyager:") {
            trimmed = rest.trim();
        } else if let Some(rest) = trimmed.strip_prefix("vn:") {
            backend_hint = Some("native".to_string());
            trimmed = rest.trim();
        } else if let Some(rest) = trimmed.strip_prefix("voyager-net:") {
            backend_hint = Some("native".to_string());
            trimmed = rest.trim();
        }

        // Handle duckdb in-memory path or file path
        if trimmed.starts_with("duckdb://") || trimmed == "duckdb" {
            let path = trimmed.strip_prefix("duckdb://").unwrap_or("");
            let db_name = if path.is_empty() || path == ":memory:" {
                None
            } else {
                Some(path.to_string())
            };
            return Ok(Self {
                protocol: DatabaseProtocol::DuckDb,
                host: "localhost".to_string(),
                port: None,
                auth: Auth::None,
                database: db_name,
                tls: TlsMode::Disabled,
                params: HashMap::new(),
                backend_hint,
                raw: raw_uri.trim().to_string(),
            });
        }

        let parsed = Url::parse(trimmed)
            .map_err(|e| NetError::InvalidUri(format!("Malformed URI '{}': {}", trimmed, e)))?;

        let scheme = parsed.scheme().to_lowercase();
        let (protocol, default_tls) = match scheme.as_str() {
            "bolt" | "neo4j" | "memgraph" => (DatabaseProtocol::Bolt, TlsMode::Disabled),
            "bolt+s" | "neo4j+s" | "memgraph+s" => (DatabaseProtocol::Bolt, TlsMode::Required),
            "bolt+ssc" | "neo4j+ssc" | "memgraph+ssc" => {
                (DatabaseProtocol::Bolt, TlsMode::SelfSigned)
            }
            "postgresql" | "postgres" | "age" => (DatabaseProtocol::Postgres, TlsMode::Disabled),
            "postgresql+s" | "postgres+s" | "age+s" => {
                (DatabaseProtocol::Postgres, TlsMode::Required)
            }
            "redis" | "falkordb" | "valkey" => (DatabaseProtocol::Redis, TlsMode::Disabled),
            "rediss" | "falkordbs" | "valkeys" => (DatabaseProtocol::Redis, TlsMode::Required),
            "duckdb" => (DatabaseProtocol::DuckDb, TlsMode::Disabled),
            other => {
                return Err(NetError::InvalidUri(format!(
                    "Unsupported database URI scheme '{}'. Supported schemes: bolt://, neo4j://, memgraph://, postgresql://, age://, redis://, falkordb://, valkey://, duckdb://",
                    other
                )));
            }
        };

        let host = parsed.host_str().unwrap_or("localhost").to_string();
        let port = parsed.port().or_else(|| protocol.default_port());

        // Extract authentication from URI userInfo
        let username = parsed.username();
        let password = parsed.password();
        let auth = if !username.is_empty() || password.is_some() {
            Auth::Basic {
                username: username.to_string(),
                password: password.unwrap_or_default().to_string(),
                realm: None,
            }
        } else {
            Auth::None
        };

        // Extract query parameters
        let mut params: HashMap<String, String> = HashMap::new();
        for (k, v) in parsed.query_pairs() {
            params.insert(k.to_string(), v.to_string());
        }

        // Database catalog / graph extraction
        let path = parsed.path().trim_start_matches('/');
        let mut database = if !path.is_empty() {
            Some(path.to_string())
        } else {
            None
        };

        // Override database if ?database= or ?graph= is in query parameters
        if let Some(db_param) = params.get("database").or_else(|| params.get("graph"))
            && !db_param.is_empty()
        {
            database = Some(db_param.clone());
        }

        // Resolve TLS mode from query parameters if present (e.g. ?ssl=true or ?sslmode=require)
        let mut tls = default_tls;
        if let Some(ssl) = params.get("ssl").or_else(|| params.get("tls")) {
            match ssl.to_lowercase().as_str() {
                "true" | "1" | "require" | "required" => tls = TlsMode::Required,
                "self-signed" | "ssc" | "allow_invalid" | "insecure" => tls = TlsMode::SelfSigned,
                "false" | "0" | "disable" | "disabled" => tls = TlsMode::Disabled,
                _ => {}
            }
        } else if let Some(sslmode) = params.get("sslmode") {
            match sslmode.to_lowercase().as_str() {
                "require" | "verify-ca" | "verify-full" => tls = TlsMode::Required,
                "prefer" | "allow" => tls = TlsMode::Required,
                "disable" => tls = TlsMode::Disabled,
                _ => {}
            }
        }

        Ok(Self {
            protocol,
            host,
            port,
            auth,
            database,
            tls,
            params,
            backend_hint,
            raw: raw_uri.trim().to_string(),
        })
    }

    /// Returns the socket address format `host:port` (or `[host]:port` for IPv6).
    pub fn socket_addr(&self) -> String {
        match self.port {
            Some(p) => {
                if self.host.contains(':') && !self.host.starts_with('[') {
                    format!("[{}]:{}", self.host, p)
                } else {
                    format!("{}:{}", self.host, p)
                }
            }
            None => self.host.clone(),
        }
    }
}

impl FromStr for ParsedUri {
    type Err = NetError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for ParsedUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) [db: {:?}, tls: {:?}]",
            self.raw, self.protocol, self.database, self.tls
        )
    }
}
