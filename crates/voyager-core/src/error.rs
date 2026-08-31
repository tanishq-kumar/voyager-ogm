//! Error types and diagnostic reporting for `voyager-core`.

use thiserror::Error;

/// Result alias for Voyager OGM core operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Primary error enum for AST construction, validation, and dialect emission.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid AST node handle index in the arena.
    #[error("Invalid AST node handle index: {0}")]
    InvalidNodeHandle(u32),

    /// Missing required AST field or child node.
    #[error("Missing required AST field: {0}")]
    MissingField(String),

    /// Unsupported dialect feature or capability.
    #[error("Dialect '{dialect}' does not support feature: {feature}")]
    UnsupportedFeature {
        /// Target dialect name (e.g. "cypher", "sql_pgq", "iso_gql")
        dialect: String,
        /// Description of the unsupported feature
        feature: String,
    },

    /// AST structural invariant violation or malformed graph pattern.
    #[error("AST invariant violation: {0}")]
    AstInvariantViolation(String),

    /// Query emission or translation error.
    #[error("Query emission error: {0}")]
    EmissionError(String),

    /// Arrow conversion or IPC error.
    #[error("Arrow bridge error: {0}")]
    ArrowError(String),

    /// Transaction lifecycle or rollback error.
    #[error("Transaction error: {0}")]
    TransactionError(String),

    /// I/O or serialization error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}
