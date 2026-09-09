//! Bolt chunk framing encoder and decoder for TCP streams.

use bytes::{BufMut, Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{NetError, Result};

/// Maximum size of a single Bolt chunk payload in bytes (64 KB - 1).
pub const MAX_CHUNK_SIZE: usize = 65535;
/// Default chunk size used when splitting outbound messages (64 KB - 1 for max TCP throughput).
pub const DEFAULT_CHUNK_SIZE: usize = MAX_CHUNK_SIZE;

/// Formats a raw PackStream message payload into Bolt chunk frames with `0x00 0x00` terminator.
pub fn encode_chunks(payload: &[u8], buf: &mut BytesMut) {
    let mut offset = 0;
    while offset < payload.len() {
        let chunk_len = (payload.len() - offset).min(MAX_CHUNK_SIZE);
        buf.put_u16(chunk_len as u16);
        buf.put_slice(&payload[offset..offset + chunk_len]);
        offset += chunk_len;
    }
    // Terminating empty chunk
    buf.put_u16(0);
}

/// Reads and reassembles a chunked Bolt message from an asynchronous reader until `0x00 0x00` is encountered.
pub async fn read_message_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Bytes> {
    let mut message_buf = BytesMut::new();

    loop {
        let mut header = [0u8; 2];
        reader.read_exact(&mut header).await.map_err(|e| {
            NetError::ConnectionFailed(format!("Failed to read chunk header: {}", e))
        })?;

        let chunk_size = u16::from_be_bytes(header) as usize;
        if chunk_size == 0 {
            // End of message frame reached
            break;
        }

        let start_len = message_buf.len();
        message_buf.resize(start_len + chunk_size, 0);
        reader
            .read_exact(&mut message_buf[start_len..start_len + chunk_size])
            .await
            .map_err(|e| {
                NetError::ConnectionFailed(format!("Failed to read chunk payload: {}", e))
            })?;
    }

    Ok(message_buf.freeze())
}

/// Writes an encoded chunk buffer to an asynchronous writer and flushes the socket.
pub async fn write_message_frame<W: AsyncWrite + Unpin>(writer: &mut W, buf: &[u8]) -> Result<()> {
    writer
        .write_all(buf)
        .await
        .map_err(|e| NetError::ConnectionFailed(format!("Failed to write chunk frame: {}", e)))?;
    writer
        .flush()
        .await
        .map_err(|e| NetError::ConnectionFailed(format!("Failed to flush socket: {}", e)))?;
    Ok(())
}
