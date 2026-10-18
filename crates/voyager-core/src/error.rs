//! Error types and diagnostic reporting for `voyager-core`.

use std::fmt;
use thiserror::Error;

/// Machine-readable diagnostic error code for Voyager OGM operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "SCREAMING_SNAKE_CASE"))]
pub enum ErrorCode {
    /// Target dialect does not support the requested operation or syntax.
    UnsupportedDialect,
    /// Target dialect does not support a specific feature.
    UnsupportedFeature,
    /// Specified node label is unknown or unregistered in the schema registry.
    UnknownNodeLabel,
    /// Specified relationship type is unknown or unregistered in the schema registry.
    UnknownRelationshipType,
    /// Property value does not conform to the expected schema field type.
    PropertyTypeMismatch,
    /// Property is not declared in the schema definition.
    UndeclaredProperty,
    /// Source or target node labels are incompatible with relationship schema definition.
    IncompatibleEndpoints,
    /// Graph topology schema version is unsupported.
    UnsupportedTopologyVersion,
    /// Query clause syntax or structure is invalid.
    InvalidClause,
    /// Query specification structure or interchange format is invalid.
    InvalidSpec,
    /// AST node handle index is out of bounds in the arena.
    InvalidNodeHandle,
    /// Required AST field or parameter is missing.
    MissingField,
    /// AST structural invariant was violated during construction or transformation.
    AstInvariantViolation,
    /// Query emission failed during SQL/Cypher/GQL generation.
    EmissionError,
    /// Decoded element size exceeds available buffer capacity (allocation bomb protection).
    ClaimedSizeExceedsBuffer,
    /// Spatial Point missing required coordinate components (x/y or longitude/latitude).
    MissingCoordinates,
    /// Failed to connect to database host.
    ConnectionFailed,
    /// Database rejected authentication credentials.
    AuthenticationFailed,
    /// Connection pool exhausted and acquire timeout elapsed.
    PoolExhausted,
    /// Network socket or database operation timed out.
    Timeout,
    /// Protocol framing or serialization error.
    ProtocolError,
    /// TLS negotiation or handshake failed.
    TlsError,
    /// Connection was closed during operation.
    Closed,
    /// Connection URI string is malformed or invalid.
    InvalidUri,
    /// Query or command execution error returned by database.
    ExecutionError,
    /// Apache Arrow conversion or IPC error.
    ArrowError,
    /// Transaction lifecycle, commit, or rollback error.
    TransactionError,
    /// Schema validation, registry lock, or constraint error.
    SchemaError,
    /// Underlying I/O error.
    IoError,
    /// Unspecified internal runtime error.
    InternalError,
}

impl ErrorCode {
    /// Returns the static screaming-snake-case representation of this error code.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnsupportedDialect => "UNSUPPORTED_DIALECT",
            Self::UnsupportedFeature => "UNSUPPORTED_FEATURE",
            Self::UnknownNodeLabel => "UNKNOWN_NODE_LABEL",
            Self::UnknownRelationshipType => "UNKNOWN_RELATIONSHIP_TYPE",
            Self::PropertyTypeMismatch => "PROPERTY_TYPE_MISMATCH",
            Self::UndeclaredProperty => "UNDECLARED_PROPERTY",
            Self::IncompatibleEndpoints => "INCOMPATIBLE_ENDPOINTS",
            Self::UnsupportedTopologyVersion => "UNSUPPORTED_TOPOLOGY_VERSION",
            Self::InvalidClause => "INVALID_CLAUSE",
            Self::InvalidSpec => "INVALID_SPEC",
            Self::InvalidNodeHandle => "INVALID_NODE_HANDLE",
            Self::MissingField => "MISSING_FIELD",
            Self::AstInvariantViolation => "AST_INVARIANT_VIOLATION",
            Self::EmissionError => "EMISSION_ERROR",
            Self::ClaimedSizeExceedsBuffer => "CLAIMED_SIZE_EXCEEDS_BUFFER",
            Self::MissingCoordinates => "MISSING_COORDINATES",
            Self::ConnectionFailed => "CONNECTION_FAILED",
            Self::AuthenticationFailed => "AUTHENTICATION_FAILED",
            Self::PoolExhausted => "POOL_EXHAUSTED",
            Self::Timeout => "TIMEOUT",
            Self::ProtocolError => "PROTOCOL_ERROR",
            Self::TlsError => "TLS_ERROR",
            Self::Closed => "CLOSED",
            Self::InvalidUri => "INVALID_URI",
            Self::ExecutionError => "EXECUTION_ERROR",
            Self::ArrowError => "ARROW_ERROR",
            Self::TransactionError => "TRANSACTION_ERROR",
            Self::SchemaError => "SCHEMA_ERROR",
            Self::IoError => "IO_ERROR",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    /// Returns `true` if this error represents a semantic, syntax, or schema issue that should not trip circuit breakers.
    pub fn is_semantic(&self) -> bool {
        matches!(
            self,
            Self::UnsupportedDialect
                | Self::UnsupportedFeature
                | Self::UnknownNodeLabel
                | Self::UnknownRelationshipType
                | Self::PropertyTypeMismatch
                | Self::UndeclaredProperty
                | Self::IncompatibleEndpoints
                | Self::UnsupportedTopologyVersion
                | Self::InvalidClause
                | Self::InvalidSpec
                | Self::InvalidNodeHandle
                | Self::MissingField
                | Self::AstInvariantViolation
                | Self::EmissionError
                | Self::ClaimedSizeExceedsBuffer
                | Self::MissingCoordinates
                | Self::InvalidUri
                | Self::SchemaError
        )
    }

    /// Returns `true` if this error represents a transient network, timeout, or pool error.
    pub fn is_transient(&self) -> bool {
        !self.is_semantic()
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

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

    /// Unsupported dialect or capability.
    #[error("{dialect} does not support {feature}")]
    UnsupportedDialect {
        /// Target dialect name (e.g. "Apache AGE", "FalkorDB")
        dialect: String,
        /// Description of the unsupported feature or operation
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

    /// Schema validation or registry error.
    #[error("Schema error: {0}")]
    SchemaError(String),

    /// Invalid query specification dictionary or protocol structure.
    #[error("Invalid query specification: {0}")]
    InvalidSpec(String),

    /// Invalid query clause structure or unsupported operation.
    #[error("Invalid query clause: {0}")]
    InvalidClause(String),

    /// Missing spatial coordinates for point construction.
    #[error("Missing coordinates: {0}")]
    MissingCoordinates(String),

    /// Claimed element size exceeds available buffer capacity.
    #[error("Claimed size exceeds buffer: {0}")]
    ClaimedSizeExceedsBuffer(String),

    /// Unknown or unregistered node label in schema.
    #[error("Unknown node label: {0}")]
    UnknownNodeLabel(String),

    /// Unknown or unregistered relationship type in schema.
    #[error("Unknown relationship type: {0}")]
    UnknownRelationshipType(String),

    /// Property value does not match expected field type.
    #[error("Property type mismatch: {0}")]
    PropertyTypeMismatch(String),

    /// I/O or serialization error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

impl Error {
    /// Returns the machine-readable error code associated with this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidNodeHandle(_) => ErrorCode::InvalidNodeHandle,
            Self::MissingField(_) => ErrorCode::MissingField,
            Self::UnsupportedFeature { .. } => ErrorCode::UnsupportedFeature,
            Self::UnsupportedDialect { .. } => ErrorCode::UnsupportedDialect,
            Self::AstInvariantViolation(_) => ErrorCode::AstInvariantViolation,
            Self::EmissionError(_) => ErrorCode::EmissionError,
            Self::ArrowError(_) => ErrorCode::ArrowError,
            Self::TransactionError(_) => ErrorCode::TransactionError,
            Self::SchemaError(_) => ErrorCode::SchemaError,
            Self::InvalidSpec(_) => ErrorCode::InvalidSpec,
            Self::InvalidClause(_) => ErrorCode::InvalidClause,
            Self::MissingCoordinates(_) => ErrorCode::MissingCoordinates,
            Self::ClaimedSizeExceedsBuffer(_) => ErrorCode::ClaimedSizeExceedsBuffer,
            Self::UnknownNodeLabel(_) => ErrorCode::UnknownNodeLabel,
            Self::UnknownRelationshipType(_) => ErrorCode::UnknownRelationshipType,
            Self::PropertyTypeMismatch(_) => ErrorCode::PropertyTypeMismatch,
            Self::IoError(_) => ErrorCode::IoError,
        }
    }
}
