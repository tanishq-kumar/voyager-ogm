//! PostgreSQL asynchronous message framing, encoder, and decoder.

use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{NetError, Result};
use crate::postgres::message::{BackendMessage, FrontendMessage, StartupMessage};

/// Maximum allowable single message size in bytes (128 MB safety guard).
pub const MAX_PG_MESSAGE_SIZE: usize = 128 * 1024 * 1024;

/// Default internal buffer capacity for socket writes.
pub const DEFAULT_BUFFER_CAPACITY: usize = 8 * 1024;

/// Asynchronously encodes and writes the initial PostgreSQL `StartupMessage`.
pub async fn write_startup_message<W>(writer: &mut W, msg: &StartupMessage) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut buf = BytesMut::with_capacity(DEFAULT_BUFFER_CAPACITY);
    msg.encode(&mut buf);
    writer.write_all(&buf).await.map_err(NetError::IoError)?;
    writer.flush().await.map_err(NetError::IoError)?;
    Ok(())
}

/// Asynchronously encodes and writes a single `FrontendMessage`.
pub async fn write_frontend_message<W>(writer: &mut W, msg: &FrontendMessage) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut buf = BytesMut::with_capacity(DEFAULT_BUFFER_CAPACITY);
    msg.encode(&mut buf);
    writer.write_all(&buf).await.map_err(NetError::IoError)?;
    writer.flush().await.map_err(NetError::IoError)?;
    Ok(())
}

/// Asynchronously encodes and writes multiple `FrontendMessage`s in a single network flush.
pub async fn write_frontend_messages<W>(writer: &mut W, msgs: &[FrontendMessage]) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut buf = BytesMut::with_capacity(DEFAULT_BUFFER_CAPACITY);
    for msg in msgs {
        msg.encode(&mut buf);
    }
    writer.write_all(&buf).await.map_err(NetError::IoError)?;
    writer.flush().await.map_err(NetError::IoError)?;
    Ok(())
}

/// Asynchronously reads and decodes the next `BackendMessage` from the network stream.
pub async fn read_backend_message<R>(reader: &mut R) -> Result<BackendMessage>
where
    R: AsyncRead + Unpin,
{
    // Read 1-byte message type identifier
    let msg_type = match reader.read_u8().await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(NetError::Closed(
                "PostgreSQL connection closed unexpectedly by remote host".to_string(),
            ));
        }
        Err(e) => return Err(NetError::IoError(e)),
    };

    // Read 4-byte big-endian message length (includes the 4 length bytes)
    let length = reader.read_i32().await.map_err(NetError::IoError)?;
    if length < 4 {
        return Err(NetError::ProtocolError(format!(
            "Invalid PostgreSQL message length: {}",
            length
        )));
    }

    let payload_len = (length - 4) as usize;
    if payload_len > MAX_PG_MESSAGE_SIZE {
        return Err(NetError::ProtocolError(format!(
            "PostgreSQL message size {} exceeds maximum allowed limit of {} bytes",
            payload_len, MAX_PG_MESSAGE_SIZE
        )));
    }

    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        reader
            .read_exact(&mut payload)
            .await
            .map_err(NetError::IoError)?;
    }

    let payload_bytes = Bytes::from(payload);
    BackendMessage::decode(msg_type, payload_bytes)
}
