//! In-memory mock TCP stub server for Bolt protocol compliance and TestKit simulation.

use bytes::BytesMut;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::connection::BOLT_MAGIC_PREAMBLE;
use super::messages::BoltResponse;
use super::packstream::{BoltValue, PackStream};
use super::stream::{encode_chunks, read_message_frame, write_message_frame};
use crate::error::{NetError, Result};

/// Scripted mock Bolt server simulating database responses and error injection over raw TCP.
pub struct BoltStubServer {
    addr: SocketAddr,
    server_task: JoinHandle<()>,
}

impl BoltStubServer {
    /// Starts a mock Bolt server on an ephemeral local port.
    pub async fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
            NetError::ConnectionFailed(format!("Failed to bind stub server: {}", e))
        })?;
        let addr = listener
            .local_addr()
            .map_err(|e| NetError::ConnectionFailed(format!("Failed to get local addr: {}", e)))?;

        let server_task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let _ = Self::handle_connection(&mut socket).await;
                });
            }
        });

        Ok(Self { addr, server_task })
    }

    /// Returns the socket address `127.0.0.1:port` of the running stub server.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Returns the Bolt URI for this stub server (`bolt://127.0.0.1:port`).
    pub fn uri(&self) -> String {
        format!("bolt://{}", self.addr)
    }

    async fn handle_connection(socket: &mut TcpStream) -> Result<()> {
        // 1. Read 20-byte Handshake
        let mut handshake = [0u8; 20];
        socket
            .read_exact(&mut handshake)
            .await
            .map_err(|e| NetError::ConnectionFailed(e.to_string()))?;

        if handshake[0..4] != BOLT_MAGIC_PREAMBLE {
            return Err(NetError::ProtocolError(
                "Invalid magic preamble".to_string(),
            ));
        }

        // Respond with agreed version Bolt v5.4 [0x00, 0x00, 0x04, 0x05]
        socket
            .write_all(&[0x00, 0x00, 0x04, 0x05])
            .await
            .map_err(|e| NetError::ConnectionFailed(e.to_string()))?;
        socket
            .flush()
            .await
            .map_err(|e| NetError::ConnectionFailed(e.to_string()))?;

        // 2. Read HELLO
        let mut frame = read_message_frame(socket).await?;
        let _hello_val = PackStream::decode(&mut frame)?;

        // Reply SUCCESS to HELLO
        let mut hello_meta = HashMap::new();
        hello_meta.insert(
            "server".to_string(),
            BoltValue::String("Neo4j/5.15.0".to_string()),
        );
        hello_meta.insert(
            "connection_id".to_string(),
            BoltValue::String("bolt-1".to_string()),
        );
        Self::send_response(
            socket,
            &BoltResponse::Success {
                metadata: hello_meta,
            },
        )
        .await?;

        let mut in_failed_state = false;

        // 3. Message loop
        loop {
            let mut msg_frame = match read_message_frame(socket).await {
                Ok(f) => f,
                Err(_) => break, // Client disconnected
            };

            let req_val = match PackStream::decode(&mut msg_frame) {
                Ok(v) => v,
                Err(_) => break,
            };

            if let BoltValue::Structure { tag, fields } = req_val {
                if in_failed_state && tag != 0x0F && tag != 0x02 {
                    // When in failed state, all subsequent pipelined requests (e.g. PULL) receive IGNORED
                    Self::send_response(
                        socket,
                        &BoltResponse::Ignored {
                            metadata: HashMap::new(),
                        },
                    )
                    .await?;
                    continue;
                }

                match tag {
                    0x10 => {
                        // RUN request
                        let query = fields.first().and_then(|v| v.as_str()).unwrap_or("");
                        let mut run_meta = HashMap::new();

                        if query.contains("FAIL") {
                            in_failed_state = true;
                            let mut fail_meta = HashMap::new();
                            fail_meta.insert(
                                "code".to_string(),
                                BoltValue::String(
                                    "Neo.ClientError.Statement.SyntaxError".to_string(),
                                ),
                            );
                            fail_meta.insert(
                                "message".to_string(),
                                BoltValue::String("Simulated query execution failure".to_string()),
                            );
                            Self::send_response(
                                socket,
                                &BoltResponse::Failure {
                                    metadata: fail_meta,
                                },
                            )
                            .await?;
                        } else {
                            // Column fields
                            let fields_list = vec![
                                BoltValue::String("name".to_string()),
                                BoltValue::String("age".to_string()),
                            ];
                            run_meta.insert("fields".to_string(), BoltValue::List(fields_list));
                            Self::send_response(
                                socket,
                                &BoltResponse::Success { metadata: run_meta },
                            )
                            .await?;
                        }
                    }
                    0x3F => {
                        // PULL request -> return 2 mock records + SUCCESS
                        let rec1 = vec![
                            BoltValue::String("Alice".to_string()),
                            BoltValue::Integer(30),
                        ];
                        let rec2 =
                            vec![BoltValue::String("Bob".to_string()), BoltValue::Integer(25)];

                        Self::send_response(socket, &BoltResponse::Record { fields: rec1 }).await?;
                        Self::send_response(socket, &BoltResponse::Record { fields: rec2 }).await?;

                        let mut pull_meta = HashMap::new();
                        let mut stats_map = HashMap::new();
                        stats_map.insert("nodes-created".to_string(), BoltValue::Integer(0));
                        pull_meta.insert("stats".to_string(), BoltValue::Map(stats_map));
                        pull_meta.insert("t_last".to_string(), BoltValue::Integer(2));
                        Self::send_response(
                            socket,
                            &BoltResponse::Success {
                                metadata: pull_meta,
                            },
                        )
                        .await?;
                    }
                    0x0F => {
                        // RESET request -> clear failed state and reply SUCCESS
                        in_failed_state = false;
                        Self::send_response(
                            socket,
                            &BoltResponse::Success {
                                metadata: HashMap::new(),
                            },
                        )
                        .await?;
                    }
                    0x02 => {
                        // GOODBYE -> terminate connection
                        break;
                    }
                    _ => {
                        // Other messages -> reply SUCCESS
                        Self::send_response(
                            socket,
                            &BoltResponse::Success {
                                metadata: HashMap::new(),
                            },
                        )
                        .await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn send_response(socket: &mut TcpStream, resp: &BoltResponse) -> Result<()> {
        let val = match resp {
            BoltResponse::Success { metadata } => BoltValue::Structure {
                tag: 0x70,
                fields: vec![BoltValue::Map(metadata.clone())],
            },
            BoltResponse::Record { fields } => BoltValue::Structure {
                tag: 0x71,
                fields: vec![BoltValue::List(fields.clone())],
            },
            BoltResponse::Failure { metadata } => BoltValue::Structure {
                tag: 0x7F,
                fields: vec![BoltValue::Map(metadata.clone())],
            },
            BoltResponse::Ignored { metadata } => BoltValue::Structure {
                tag: 0x7E,
                fields: vec![BoltValue::Map(metadata.clone())],
            },
        };

        let mut payload = BytesMut::new();
        PackStream::encode(&val, &mut payload);

        let mut chunk_buf = BytesMut::new();
        encode_chunks(&payload, &mut chunk_buf);

        write_message_frame(socket, &chunk_buf).await
    }
}

impl Drop for BoltStubServer {
    fn drop(&mut self) {
        self.server_task.abort();
    }
}
