//! Native asynchronous network runtime, connection pooling, and wire protocol engine for Voyager OGM.
//!
//! `voyager-drivers` aims to provides a high-throughput native Rust engine operating over raw TCP/TLS with
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
pub mod circuit;
pub mod client;
pub mod config;
pub mod engine;
pub mod error;
#[cfg(feature = "testkit")]
pub mod mock;
pub mod native_transaction;
pub mod pool;
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "redis")]
pub mod redis;
pub mod uri;

// Re-exports
#[cfg(feature = "testkit")]
pub use bolt::BoltStubServer;
#[cfg(feature = "bolt")]
pub use bolt::{BoltConnection, BoltRequest, BoltResponse, BoltVersion};
pub use bolt::{
    BoltNode, BoltPath, BoltRelationship, BoltUnboundRelationship, BoltValue, PackStream,
};
pub use circuit::{
    BackendRoute, CircuitConfig, CircuitRouter, CircuitSnapshot, CircuitState, ErrorClassification,
    classify_error, classify_net_error, is_semantic_error,
};
pub use client::{NativeClient, connect_any};
pub use config::{Auth, ConnectionConfig, PoolConfig, TlsMode};
pub use engine::{
    AsyncConnection, AsyncEngine, AsyncTransaction, ConnectionFactory, FnConnectionFactory,
    QueryResult, QuerySummary,
};
pub use error::{NetError, Result};
#[cfg(feature = "testkit")]
pub use mock::{MockConnection, MockConnectionFactory, MockEngine};
pub use native_transaction::{NativeTransaction, NativeTransactionState};
pub use pool::{ConnectionPool, PoolMetricsSnapshot, PooledConnection};
#[cfg(feature = "postgres")]
pub use postgres::{
    AgeEdge, AgePath, AgeValue, AgeVertex, AuthenticationRequest, BackendMessage, FieldDescription,
    FrontendMessage, PgDiagnostic, PostgresConnection, PostgresTransaction, ScramClient,
    StartupMessage, TransactionStatus, clean_agtype_string, compute_md5_password, parse_agtype,
    rows_to_record_batch,
};
#[cfg(feature = "redis")]
pub use redis::{
    FalkorNode, FalkorQueryResult, FalkorRelationship, FalkorStatistics, RedisConnection,
    RedisTransaction, RespCodec, RespValue,
};
pub use uri::{DatabaseProtocol, ParsedUri};
