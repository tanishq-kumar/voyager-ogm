//! Native PostgreSQL Frontend/Backend Protocol v3.0 implementation and Apache AGE engine.

pub mod agtype;
pub mod auth;
pub mod codec;
pub mod connection;
pub mod message;
pub mod transaction;

pub use agtype::{
    AgeEdge, AgePath, AgeValue, AgeVertex, clean_agtype_string, parse_agtype, rows_to_record_batch,
};
pub use auth::{ScramClient, compute_md5_password, generate_client_nonce};
pub use codec::{
    DEFAULT_BUFFER_CAPACITY, MAX_PG_MESSAGE_SIZE, read_backend_message, write_frontend_message,
    write_frontend_messages, write_startup_message,
};
pub use connection::PostgresConnection;
pub use message::{
    AuthenticationRequest, BackendMessage, FieldDescription, FrontendMessage, PG_PROTOCOL_V3,
    PgDiagnostic, StartupMessage, TransactionStatus, oids,
};
pub use transaction::PostgresTransaction;
