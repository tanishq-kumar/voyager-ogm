//! Native Bolt wire protocol implementation (PackStream framing, state machines, and connection).

pub mod connection;
pub mod messages;
pub mod packstream;
pub mod stream;
pub mod stub_server;
pub mod testkit;

pub use connection::{BOLT_MAGIC_PREAMBLE, BOLT_PROPOSED_VERSIONS, BoltConnection, BoltVersion};
pub use messages::{BoltRequest, BoltResponse};
pub use packstream::{
    BoltNode, BoltPath, BoltRelationship, BoltUnboundRelationship, BoltValue, PackStream,
};
pub use stream::{
    DEFAULT_CHUNK_SIZE, MAX_CHUNK_SIZE, encode_chunks, read_message_frame, write_message_frame,
};
pub use stub_server::BoltStubServer;
pub use testkit::TestkitBackend;
