//! # Voyager OGM Core Engine (`voyager-core`)
//!
//! Voyager OGM is a high-performance, vendor-neutral Object-Graph Mapper (OGM)
//! and AST query compiler implemented in pure safe Rust.
//!
//! ## Modules
//! - [`ast`]: 32-bit integer handle memory arena and AST node definitions.
//! - [`builder`]: Ergonomic fluent query builder API.
//! - [`emitters`]: Multi-dialect query string emitters (openCypher, SQL:2023 PGQ, ISO GQL).
//! - [`visitor`]: AST Visitor trait and compilation result containers.
//! - [`error`]: Error types and diagnostic reporting.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "arrow")]
pub mod arrow;
pub mod ast;
pub mod bridge;
pub mod builder;
pub mod bulk;
pub mod emitters;
pub mod error;
pub mod transaction;
pub mod visitor;

pub use ast::{
    AggregationFunc, AstNode, BinaryOp, Direction, LiteralValue, NodeHandle, ProjectionItem,
    QueryAstArena,
};
pub use bridge::{DatabaseBridge, MockDatabaseBridge, QueryResult, QuerySummary};
pub use builder::QueryBuilder;
pub use emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
pub use error::{Error, Result};
pub use transaction::{
    CheckpointState, EntityMutation, Savepoint, Transaction, TransactionState, UnitOfWork,
};
pub use visitor::{AstVisitor, CompiledQuery};

/// Voyager OGM engine version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_initialization() {
        assert!(!VERSION.is_empty());
    }
}
