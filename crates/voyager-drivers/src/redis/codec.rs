//! Asynchronous RESP stream codec and frame I/O for Tokio TCP streams.
//!
//! Provides [`RespCodec`], implementing [`tokio_util::codec::Decoder`] and
//! [`tokio_util::codec::Encoder`] with buffer reuse, max frame limit protection (128 MB),
//! and pipelined batch execution over raw TCP.

use bytes::{BufMut, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::codec::{Decoder, Encoder};

use crate::error::{NetError, Result};
use crate::redis::resp::RespValue;

/// Maximum payload size allowed for a single RESP frame (128 MB), matching Bolt and PostgreSQL safety guards.
pub const MAX_RESP_FRAME_SIZE: usize = 128 * 1024 * 1024;

/// Tokio codec for framed RESP2/RESP3 value streaming over asynchronous sockets.
#[derive(Debug, Default, Clone)]
pub struct RespCodec {
    max_frame_size: usize,
}

impl RespCodec {
    /// Creates a new `RespCodec` with the default maximum frame size of 128 MB.
    pub fn new() -> Self {
        Self {
            max_frame_size: MAX_RESP_FRAME_SIZE,
        }
    }

    /// Sets a custom maximum frame size limit in bytes.
    pub fn with_max_frame_size(mut self, size: usize) -> Self {
        self.max_frame_size = size;
        self
    }
}

impl Decoder for RespCodec {
    type Item = RespValue;
    type Error = NetError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>> {
        if src.is_empty() {
            return Ok(None);
        }

        if src.len() > self.max_frame_size {
            return Err(NetError::ProtocolError(format!(
                "RESP frame exceeded maximum safety limit of {} bytes (current: {})",
                self.max_frame_size,
                src.len()
            )));
        }

        RespValue::parse(src)
    }
}

impl Encoder<RespValue> for RespCodec {
    type Error = NetError;

    fn encode(&mut self, item: RespValue, dst: &mut BytesMut) -> Result<()> {
        item.encode(dst);
        Ok(())
    }
}

/// Asynchronously reads a single `RespValue` from a raw TCP stream, accumulating into `buffer`.
pub async fn read_resp_value(stream: &mut TcpStream, buffer: &mut BytesMut) -> Result<RespValue> {
    loop {
        // Attempt to parse existing buffered data
        if !buffer.is_empty()
            && let Some(val) = RespValue::parse(buffer)?
        {
            return Ok(val);
        }

        // Safety limit check
        if buffer.len() > MAX_RESP_FRAME_SIZE {
            return Err(NetError::ProtocolError(format!(
                "RESP buffer exceeded maximum limit of {} bytes",
                MAX_RESP_FRAME_SIZE
            )));
        }

        // Reserve space and read more data from the network socket
        buffer.reserve(4096);
        let bytes_read = stream.read_buf(buffer).await.map_err(|e| {
            NetError::ConnectionFailed(format!("Failed to read RESP frame from socket: {}", e))
        })?;

        if bytes_read == 0 {
            if buffer.is_empty() {
                return Err(NetError::ConnectionFailed(
                    "Connection closed by server while waiting for RESP reply".to_string(),
                ));
            } else {
                return Err(NetError::ProtocolError(
                    "Unexpected EOF in incomplete RESP frame".to_string(),
                ));
            }
        }
    }
}

/// Asynchronously writes a Redis command array of string arguments over the TCP stream.
pub async fn write_command(stream: &mut TcpStream, args: &[&str]) -> Result<()> {
    let payload = RespValue::encode_command(args);
    stream.write_all(&payload).await.map_err(|e| {
        NetError::ConnectionFailed(format!("Failed to write RESP command to socket: {}", e))
    })?;
    stream
        .flush()
        .await
        .map_err(|e| NetError::ConnectionFailed(format!("Failed to flush RESP stream: {}", e)))?;
    Ok(())
}

/// Asynchronously writes a command with raw byte arguments over the TCP stream.
pub async fn write_command_raw(stream: &mut TcpStream, args: &[&[u8]]) -> Result<()> {
    let mut buf = BytesMut::new();
    let header = format!("*{}\r\n", args.len());
    buf.put_slice(header.as_bytes());
    for arg in args {
        let item_hdr = format!("${}\r\n", arg.len());
        buf.put_slice(item_hdr.as_bytes());
        buf.put_slice(arg);
        buf.put_slice(b"\r\n");
    }

    stream.write_all(&buf).await.map_err(|e| {
        NetError::ConnectionFailed(format!("Failed to write raw RESP command to socket: {}", e))
    })?;
    stream
        .flush()
        .await
        .map_err(|e| NetError::ConnectionFailed(format!("Failed to flush RESP stream: {}", e)))?;
    Ok(())
}
