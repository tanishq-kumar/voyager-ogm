//! Network, protocol, and connection pool error types for Voyager OGM.

use thiserror::Error;

/// Result alias for network and connection operations.
pub type Result<T> = std::result::Result<T, NetError>;

/// Represents errors that can occur during network transport, connection pooling, and protocol parsing.
#[derive(Debug, Error)]
pub enum NetError {
    /// Failed to establish a physical socket connection to the target database host.
    #[error("Failed to connect to database host: {0}")]
    ConnectionFailed(String),

    /// Database rejected the authentication credentials.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Connection pool is exhausted and the acquire timeout elapsed.
    #[error("Connection pool exhausted: {0}")]
    PoolExhausted(String),

    /// An asynchronous network socket operation timed out.
    #[error("Network operation timed out: {0}")]
    Timeout(String),

    /// A protocol framing or serialization error occurred.
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// A TLS handshake or certificate validation error occurred.
    #[error("TLS negotiation failed: {0}")]
    TlsError(String),

    /// The connection or pool was closed while an operation was in progress.
    #[error("Connection closed: {0}")]
    Closed(String),

    /// Invalid or malformed connection URI string.
    #[error("Invalid connection URI: {0}")]
    InvalidUri(String),

    /// Transaction state error (e.g. committing an already committed transaction).
    #[error("Transaction error: {0}")]
    TransactionError(String),

    /// Underlying I/O error from network sockets.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Voyager Core AST compiler or emitter error.
    #[error("Core compiler error: {0}")]
    CoreError(#[from] voyager_core::Error),

    /// Apache Arrow record batch or schema serialization error.
    #[error("Arrow serialization error: {0}")]
    ArrowError(#[from] arrow::error::ArrowError),
}
