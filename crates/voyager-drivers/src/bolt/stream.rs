//! Bolt chunk framing encoder and decoder for TCP streams.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{NetError, Result};

/// Maximum size of a single Bolt chunk payload in bytes (64 KB - 1).
pub const MAX_CHUNK_SIZE: usize = 65535;
/// Default chunk size used when splitting outbound messages (64 KB - 1 for max TCP throughput).
pub const DEFAULT_CHUNK_SIZE: usize = MAX_CHUNK_SIZE;
/// Maximum allowed Bolt message frame size (128 MB) to prevent unbounded memory allocation.
pub const MAX_BOLT_MESSAGE_SIZE: usize = 128 * 1024 * 1024;

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

/// Reads and reassembles a chunked Bolt message from an asynchronous reader using a persistent read buffer.
pub async fn read_message_frame_buffered<R: AsyncRead + Unpin>(
    reader: &mut R,
    read_buf: &mut BytesMut,
) -> Result<Bytes> {
    let mut message_buf = BytesMut::new();

    loop {
        // 1. Ensure at least 2 bytes are available in read_buf for chunk header
        while read_buf.len() < 2 {
            let bytes_read = reader.read_buf(read_buf).await.map_err(|e| {
                NetError::ConnectionFailed(format!("Failed to read chunk header: {}", e))
            })?;
            if bytes_read == 0 {
                return Err(NetError::ConnectionFailed(
                    "Connection closed by peer while waiting for chunk header".to_string(),
                ));
            }
        }

        let chunk_size = u16::from_be_bytes([read_buf[0], read_buf[1]]) as usize;
        read_buf.advance(2);

        if chunk_size == 0 {
            // End of message frame reached
            break;
        }

        if message_buf.len() + chunk_size > MAX_BOLT_MESSAGE_SIZE {
            return Err(NetError::ProtocolError(format!(
                "Bolt message frame exceeded maximum allowed size of {} bytes",
                MAX_BOLT_MESSAGE_SIZE
            )));
        }

        // 2. Ensure all chunk_size bytes for payload are present in read_buf
        while read_buf.len() < chunk_size {
            let bytes_read = reader.read_buf(read_buf).await.map_err(|e| {
                NetError::ConnectionFailed(format!("Failed to read chunk payload: {}", e))
            })?;
            if bytes_read == 0 {
                return Err(NetError::ConnectionFailed(
                    "Connection closed by peer mid-chunk payload".to_string(),
                ));
            }
        }

        message_buf.extend_from_slice(&read_buf[..chunk_size]);
        read_buf.advance(chunk_size);
    }

    Ok(message_buf.freeze())
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

        if message_buf.len() + chunk_size > MAX_BOLT_MESSAGE_SIZE {
            return Err(NetError::ProtocolError(format!(
                "Bolt message frame exceeded maximum allowed size of {} bytes",
                MAX_BOLT_MESSAGE_SIZE
            )));
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
