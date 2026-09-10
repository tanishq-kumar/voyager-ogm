//! Native asynchronous network runtime, connection pooling, and wire protocol engine for Voyager OGM.
//!
//! `voyager-net` aims to provides a high-throughput native Rust engine operating over raw TCP/TLS with
//! zero GIL contention, connection pooling, and direct wire-to-Arrow streaming.
//!
//! # Architecture
//! - [`ConnectionPool`]: Thread-safe, generic connection pool with RAII leasing guards and liveness probes.
//! - [`ParsedUri`]: Structured URI parser for Bolt, PostgreSQL, Redis/FalkorDB, and DuckDB endpoints.
//! - [`AsyncEngine`], [`AsyncConnection`], [`AsyncTransaction`]: Core asynchronous database abstractions.
//! - [`QueryResult`], [`QuerySummary`]: High-performance columnar result containers using Apache Arrow.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod bolt;
pub mod config;
pub mod engine;
pub mod error;
pub mod mock;
pub mod pool;
pub mod postgres;
pub mod redis;
pub mod uri;

// Re-exports
pub use bolt::{
    BoltConnection, BoltNode, BoltPath, BoltRelationship, BoltRequest, BoltResponse,
    BoltStubServer, BoltUnboundRelationship, BoltValue, BoltVersion, PackStream,
};
pub use config::{Auth, ConnectionConfig, PoolConfig, TlsMode};
pub use engine::{
    AsyncConnection, AsyncEngine, AsyncTransaction, ConnectionFactory, FnConnectionFactory,
    QueryResult, QuerySummary,
};
pub use error::{NetError, Result};
pub use mock::{MockConnection, MockConnectionFactory, MockEngine};
pub use pool::{ConnectionPool, PoolMetricsSnapshot, PooledConnection};
pub use postgres::{
    AgeEdge, AgePath, AgeValue, AgeVertex, AuthenticationRequest, BackendMessage, FieldDescription,
    FrontendMessage, PgDiagnostic, PostgresConnection, PostgresTransaction, ScramClient,
    StartupMessage, TransactionStatus, clean_agtype_string, compute_md5_password, parse_agtype,
    rows_to_record_batch,
};
pub use redis::{
    FalkorNode, FalkorQueryResult, FalkorRelationship, FalkorStatistics, RedisConnection,
    RedisTransaction, RespCodec, RespValue,
};
pub use uri::{DatabaseProtocol, ParsedUri};
