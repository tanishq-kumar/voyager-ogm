//! # Voyager OGM Core Engine (`voyager-core`)
//!
//! Voyager OGM is a vendor-neutral Object-Graph Mapper (OGM)
//! and AST query compiler implemented in pure safe Rust.
//!
//! ## Modules
//! - [`ast`]: AST node definitions and handle-based memory management.
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
pub mod cache;
pub mod emitters;
pub mod error;
pub mod optimizer;
pub mod schema;
pub mod transaction;
pub mod visitor;

pub use ast::{
    AggregationFunc, AstNode, BinaryOp, Direction, ExecutionMode, LiteralValue, NodeHandle,
    ProjectionItem, QueryAstArena,
};
pub use bridge::{DatabaseBridge, MockDatabaseBridge, QueryResult, QuerySummary};
pub use builder::QueryBuilder;
pub use cache::{CacheMetrics, CompiledQueryCache, global_query_cache};
pub use emitters::{
    AgeEmitter, CypherEmitter, IsoGqlEmitter, SqlPgqEmitter, emit_cypher_drop_node_ddl,
    emit_cypher_drop_rel_ddl, emit_cypher_node_ddl, emit_cypher_rel_ddl, emit_gql_alter_node_ddl,
    emit_gql_alter_rel_ddl, emit_gql_drop_graph_type_ddl, emit_gql_graph_type_ddl,
    emit_pgq_drop_property_graph_ddl, emit_pgq_property_graph_ddl,
};
pub use error::{Error, Result};
pub use optimizer::{AstOptimizer, OptimizationLevel};
pub use schema::{
    ConstraintType, FieldDescriptor, FieldType, IndexType, NodeSchema, RelationshipSchema,
    SchemaRegistry, SchemaSnapshot, global_schema_registry,
};
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
