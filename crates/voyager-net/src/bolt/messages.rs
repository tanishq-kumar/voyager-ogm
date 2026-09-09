//! Bolt message definitions, framing, and request/response serialization.

use bytes::{Bytes, BytesMut};
use std::collections::HashMap;

use super::packstream::{BoltValue, PackStream};
use crate::error::{NetError, Result};

/// Bolt protocol request messages sent from client to database server.
#[derive(Debug, Clone, PartialEq)]
pub enum BoltRequest {
    /// Handshake authentication and metadata initialization (`0x01`).
    Hello {
        /// Client metadata and authentication credentials.
        extra: HashMap<String, BoltValue>,
    },
    /// Bolt v5.1+ separate authentication message (`0x6A`).
    Logon {
        /// Authentication credentials dictionary.
        auth: HashMap<String, BoltValue>,
    },
    /// Bolt v5.1+ de-authentication message (`0x6B`).
    Logoff,
    /// Executes a parameterized query statement (`0x10`).
    Run {
        /// Parameterized Cypher or GQL query string.
        query: String,
        /// Named parameter map.
        params: HashMap<String, BoltValue>,
        /// Extra query configuration (e.g. `db`, `tx_timeout`, `mode`).
        extra: HashMap<String, BoltValue>,
    },
    /// Pulls streamed records resulting from a previously executed query (`0x3F`).
    Pull {
        /// Pull configuration (e.g. `{"n": -1}` for all records, or batch size).
        extra: HashMap<String, BoltValue>,
    },
    /// Discards remaining records from an active query stream (`0x2F`).
    Discard {
        /// Discard configuration.
        extra: HashMap<String, BoltValue>,
    },
    /// Begins an explicit transaction block (`0x11`).
    Begin {
        /// Transaction configuration (e.g. bookmarks, timeout, database).
        extra: HashMap<String, BoltValue>,
    },
    /// Commits an open explicit transaction (`0x12`).
    Commit,
    /// Aborts and rolls back an open explicit transaction (`0x13`).
    Rollback,
    /// Resets the session, interrupting any active query and returning to READY (`0x0F`).
    Reset,
    /// Gracefully closes the connection socket (`0x02`).
    Goodbye,
}

impl BoltRequest {
    /// Creates a Bolt v5.1+ `HELLO` message (without inline authentication).
    pub fn hello_v51(user_agent: &str, routing: Option<HashMap<String, BoltValue>>) -> Self {
        let mut extra = HashMap::new();
        extra.insert(
            "user_agent".to_string(),
            BoltValue::String(user_agent.to_string()),
        );

        let mut bolt_agent = HashMap::new();
        bolt_agent.insert(
            "product".to_string(),
            BoltValue::String("voyager-net/0.4.6".to_string()),
        );
        bolt_agent.insert(
            "platform".to_string(),
            BoltValue::String(std::env::consts::OS.to_string()),
        );
        bolt_agent.insert(
            "language".to_string(),
            BoltValue::String("Rust".to_string()),
        );
        extra.insert("bolt_agent".to_string(), BoltValue::Map(bolt_agent));

        if let Some(r) = routing {
            extra.insert("routing".to_string(), BoltValue::Map(r));
        }

        Self::Hello { extra }
    }

    /// Creates a legacy Bolt (<= v5.0) `HELLO` message (with inline authentication).
    pub fn hello_legacy(
        user_agent: &str,
        auth_scheme: Option<(&str, &str)>,
        database: Option<&str>,
    ) -> Self {
        let mut extra = HashMap::new();
        extra.insert(
            "user_agent".to_string(),
            BoltValue::String(user_agent.to_string()),
        );

        let mut bolt_agent = HashMap::new();
        bolt_agent.insert(
            "product".to_string(),
            BoltValue::String("voyager-net/0.4.6".to_string()),
        );
        bolt_agent.insert(
            "platform".to_string(),
            BoltValue::String(std::env::consts::OS.to_string()),
        );
        bolt_agent.insert(
            "language".to_string(),
            BoltValue::String("Rust".to_string()),
        );
        extra.insert("bolt_agent".to_string(), BoltValue::Map(bolt_agent));

        if let Some((username, password)) = auth_scheme {
            extra.insert("scheme".to_string(), BoltValue::String("basic".to_string()));
            extra.insert(
                "principal".to_string(),
                BoltValue::String(username.to_string()),
            );
            extra.insert(
                "credentials".to_string(),
                BoltValue::String(password.to_string()),
            );
        } else {
            extra.insert("scheme".to_string(), BoltValue::String("none".to_string()));
        }

        if let Some(db) = database {
            extra.insert("db".to_string(), BoltValue::String(db.to_string()));
        }

        Self::Hello { extra }
    }

    /// Creates a Bolt v5.1+ `LOGON` request message.
    pub fn logon(auth_scheme: Option<(&str, &str)>) -> Self {
        let mut auth = HashMap::new();
        if let Some((username, password)) = auth_scheme {
            auth.insert("scheme".to_string(), BoltValue::String("basic".to_string()));
            auth.insert(
                "principal".to_string(),
                BoltValue::String(username.to_string()),
            );
            auth.insert(
                "credentials".to_string(),
                BoltValue::String(password.to_string()),
            );
        } else {
            auth.insert("scheme".to_string(), BoltValue::String("none".to_string()));
        }
        Self::Logon { auth }
    }

    /// Creates a standard `RUN` request message.
    pub fn run(
        query: impl Into<String>,
        params: HashMap<String, BoltValue>,
        database: Option<&str>,
    ) -> Self {
        let mut extra = HashMap::new();
        if let Some(db) = database {
            extra.insert("db".to_string(), BoltValue::String(db.to_string()));
        }

        Self::Run {
            query: query.into(),
            params,
            extra,
        }
    }

    /// Creates a `PULL` message to fetch all remaining records (`n = -1`).
    pub fn pull_all() -> Self {
        let mut extra = HashMap::new();
        extra.insert("n".to_string(), BoltValue::Integer(-1));
        Self::Pull { extra }
    }

    /// Creates a `PULL` message with a specific batch limit.
    pub fn pull_batch(batch_size: i64) -> Self {
        let mut extra = HashMap::new();
        extra.insert("n".to_string(), BoltValue::Integer(batch_size));
        Self::Pull { extra }
    }

    /// Serializes the request into a PackStream binary byte buffer.
    pub fn encode(&self, buf: &mut BytesMut) {
        match self {
            Self::Hello { extra } => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x01,
                        fields: vec![BoltValue::Map(extra.clone())],
                    },
                    buf,
                );
            }
            Self::Logon { auth } => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x6A,
                        fields: vec![BoltValue::Map(auth.clone())],
                    },
                    buf,
                );
            }
            Self::Logoff => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x6B,
                        fields: Vec::new(),
                    },
                    buf,
                );
            }
            Self::Run {
                query,
                params,
                extra,
            } => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x10,
                        fields: vec![
                            BoltValue::String(query.clone()),
                            BoltValue::Map(params.clone()),
                            BoltValue::Map(extra.clone()),
                        ],
                    },
                    buf,
                );
            }
            Self::Pull { extra } => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x3F,
                        fields: vec![BoltValue::Map(extra.clone())],
                    },
                    buf,
                );
            }
            Self::Discard { extra } => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x2F,
                        fields: vec![BoltValue::Map(extra.clone())],
                    },
                    buf,
                );
            }
            Self::Begin { extra } => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x11,
                        fields: vec![BoltValue::Map(extra.clone())],
                    },
                    buf,
                );
            }
            Self::Commit => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x12,
                        fields: Vec::new(),
                    },
                    buf,
                );
            }
            Self::Rollback => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x13,
                        fields: Vec::new(),
                    },
                    buf,
                );
            }
            Self::Reset => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x0F,
                        fields: Vec::new(),
                    },
                    buf,
                );
            }
            Self::Goodbye => {
                PackStream::encode(
                    &BoltValue::Structure {
                        tag: 0x02,
                        fields: Vec::new(),
                    },
                    buf,
                );
            }
        }
    }
}

/// Bolt protocol response messages returned by the database server.
#[derive(Debug, Clone, PartialEq)]
pub enum BoltResponse {
    /// Operation succeeded with metadata (`0x70`).
    Success {
        /// Metadata map (e.g. column `fields`, execution `stats`, server version).
        metadata: HashMap<String, BoltValue>,
    },
    /// A single record row returned by `PULL` (`0x71`).
    Record {
        /// Ordered field values matching columns declared in `SUCCESS.fields`.
        fields: Vec<BoltValue>,
    },
    /// Operation failed with error code and description (`0x7F`).
    Failure {
        /// Error metadata map (contains `"code"` and `"message"`).
        metadata: HashMap<String, BoltValue>,
    },
    /// Pipelined message was ignored due to an earlier error in the batch (`0x7E`).
    Ignored {
        /// Ignored metadata.
        metadata: HashMap<String, BoltValue>,
    },
}

impl BoltResponse {
    /// Decodes a `BoltResponse` from a PackStream byte buffer.
    pub fn decode(buf: &mut Bytes) -> Result<Self> {
        let value = PackStream::decode(buf)?;
        match value {
            BoltValue::Structure { tag, mut fields } => match tag {
                0x70 => {
                    let metadata = if let Some(BoltValue::Map(m)) = fields.pop() {
                        m
                    } else {
                        HashMap::new()
                    };
                    Ok(Self::Success { metadata })
                }
                0x71 => {
                    let record_fields = if let Some(BoltValue::List(l)) = fields.pop() {
                        l
                    } else {
                        fields
                    };
                    Ok(Self::Record {
                        fields: record_fields,
                    })
                }
                0x7F => {
                    let metadata = if let Some(BoltValue::Map(m)) = fields.pop() {
                        m
                    } else {
                        HashMap::new()
                    };
                    Ok(Self::Failure { metadata })
                }
                0x7E => {
                    let metadata = if let Some(BoltValue::Map(m)) = fields.pop() {
                        m
                    } else {
                        HashMap::new()
                    };
                    Ok(Self::Ignored { metadata })
                }
                unknown => Err(NetError::ProtocolError(format!(
                    "Unexpected Bolt response message structure tag 0x{:02X}",
                    unknown
                ))),
            },
            other => Err(NetError::ProtocolError(format!(
                "Expected Bolt response structure, got: {:?}",
                other
            ))),
        }
    }

    /// Returns the error message string if this is a `Failure` response.
    pub fn failure_message(&self) -> Option<(String, String)> {
        match self {
            Self::Failure { metadata } => {
                let code = metadata
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UnknownError")
                    .to_string();
                let message = metadata
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("An unknown error occurred")
                    .to_string();
                Some((code, message))
            }
            _ => None,
        }
    }

    /// Returns the projected column names if this is a `SUCCESS` response declaring fields.
    pub fn fields(&self) -> Option<Vec<String>> {
        match self {
            Self::Success { metadata } => {
                metadata.get("fields").and_then(|v| v.as_list()).map(|l| {
                    l.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
            }
            _ => None,
        }
    }
}
