//! Native Redis Serialization Protocol (RESP2 / RESP3) and FalkorDB graph engine.
//!
//! Provides pure safe Rust implementations of:
//! - [`RespValue`]: Full RESP2 and RESP3 wire data types, slice parser, and serializer.
//! - [`RespCodec`]: Asynchronous Tokio codec with 128 MB safety protection and chunk pipelining.
//! - [`RedisConnection`]: Asynchronous physical connection implementing [`crate::engine::AsyncConnection`].
//! - [`RedisTransaction`]: Transaction boundary utilizing Redis `MULTI`, `EXEC`, and `DISCARD`.
//! - [`FalkorQueryResult`]: Tabular Cypher result parser with direct Apache Arrow [`arrow_array::RecordBatch`] builder.
//! - [`FalkorNode`], [`FalkorRelationship`], [`FalkorStatistics`]: Unified graph models compatible with Bolt.

pub mod codec;
pub mod connection;
pub mod graph;
pub mod resp;
pub mod transaction;

pub use codec::{
    MAX_RESP_FRAME_SIZE, RespCodec, read_resp_value, write_command, write_command_raw,
};
pub use connection::RedisConnection;
pub use graph::{
    FalkorNode, FalkorQueryResult, FalkorRelationship, FalkorStatistics, resp_to_bolt_value,
};
pub use resp::{MAX_RESP_DEPTH, RespValue};
pub use transaction::{RedisTransaction, format_cypher_with_params};
