//! PostgreSQL Frontend and Backend Protocol v3.0 message definitions and wire serialization.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::collections::HashMap;

use crate::error::{NetError, Result};

/// Protocol v3.0 version integer (`0x0003_0000` = 196608).
pub const PG_PROTOCOL_V3: i32 = 196608;

/// PostgreSQL data type OIDs commonly used in queries.
pub mod oids {
    /// Boolean data type OID.
    pub const BOOL: i32 = 16;
    /// Bytea binary data type OID.
    pub const BYTEA: i32 = 17;
    /// Int8 / BigInt 64-bit integer OID.
    pub const INT8: i32 = 20;
    /// Int2 / SmallInt 16-bit integer OID.
    pub const INT2: i32 = 21;
    /// Int4 / Integer 32-bit integer OID.
    pub const INT4: i32 = 23;
    /// Text string data type OID.
    pub const TEXT: i32 = 25;
    /// JSON data type OID.
    pub const JSON: i32 = 114;
    /// Float4 / Real 32-bit float OID.
    pub const FLOAT4: i32 = 700;
    /// Float8 / Double Precision 64-bit float OID.
    pub const FLOAT8: i32 = 701;
    /// Varchar variable length character string OID.
    pub const VARCHAR: i32 = 1043;
    /// JSONB binary JSON data type OID.
    pub const JSONB: i32 = 3802;
    /// Unspecified / untyped OID (server infers type).
    pub const UNSPECIFIED: i32 = 0;
}

/// Transaction state indicator received in `ReadyForQuery` (`'Z'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    /// Idle, not in a transaction block (`'I'`).
    Idle,
    /// Currently inside an active transaction block (`'T'`).
    InTransaction,
    /// In a failed transaction block; queries will be rejected until rollback (`'E'`).
    FailedTransaction,
}

impl TransactionStatus {
    /// Converts raw byte from PostgreSQL `'Z'` message into `TransactionStatus`.
    pub fn from_byte(b: u8) -> Result<Self> {
        match b {
            b'I' => Ok(Self::Idle),
            b'T' => Ok(Self::InTransaction),
            b'E' => Ok(Self::FailedTransaction),
            other => Err(NetError::ProtocolError(format!(
                "Unknown transaction status byte: {}",
                other as char
            ))),
        }
    }

    /// Converts status to raw ASCII byte.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Idle => b'I',
            Self::InTransaction => b'T',
            Self::FailedTransaction => b'E',
        }
    }
}

/// PostgreSQL Frontend message types sent from client to server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendMessage {
    /// Startup message sent immediately after TCP connection establishment.
    Startup(StartupMessage),
    /// Password message or SASL challenge response (`'p'`).
    Password(Vec<u8>),
    /// SASL initial response message (`'p'`).
    SASLInitialResponse {
        /// Chosen authentication mechanism name (e.g. `"SCRAM-SHA-256"`).
        mechanism: String,
        /// Optional initial client response data.
        data: Option<Vec<u8>>,
    },
    /// SASL response message (`'p'`).
    SASLResponse(Vec<u8>),
    /// Simple Query protocol string (`'Q'`).
    Query(String),
    /// Extended Query protocol: Parse prepared statement (`'P'`).
    Parse {
        /// Prepared statement name (empty for unnamed).
        name: String,
        /// SQL query string.
        query: String,
        /// List of parameter data type OIDs.
        param_types: Vec<i32>,
    },
    /// Extended Query protocol: Bind parameters to portal (`'B'`).
    Bind {
        /// Portal name (empty for unnamed).
        portal: String,
        /// Prepared statement name (empty for unnamed).
        statement: String,
        /// Parameter format codes (0 = text, 1 = binary).
        param_formats: Vec<i16>,
        /// Parameter values (`None` for NULL, `Some(bytes)` for non-null).
        params: Vec<Option<Vec<u8>>>,
        /// Result column format codes (0 = text, 1 = binary).
        result_formats: Vec<i16>,
    },
    /// Extended Query protocol: Execute portal (`'E'`).
    Execute {
        /// Portal name (empty for unnamed).
        portal: String,
        /// Maximum number of rows to return (0 = all rows).
        max_rows: i32,
    },
    /// Extended Query protocol: Describe statement or portal (`'D'`).
    Describe {
        /// `'S'` for prepared statement, `'P'` for portal.
        target_type: u8,
        /// Statement or portal name.
        name: String,
    },
    /// Extended Query protocol: Close statement or portal (`'C'`).
    Close {
        /// `'S'` for prepared statement, `'P'` for portal.
        target_type: u8,
        /// Statement or portal name.
        name: String,
    },
    /// Extended Query protocol: Synchronize and commit/complete transaction cycle (`'S'`).
    Sync,
    /// Extended Query protocol: Flush pending messages to client (`'H'`).
    Flush,
    /// Graceful connection termination (`'X'`).
    Terminate,
}

/// Initial startup parameters sent to PostgreSQL server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupMessage {
    /// Protocol version (default `PG_PROTOCOL_V3`).
    pub protocol_version: i32,
    /// Parameter key-value map (e.g. `user`, `database`, `client_encoding`, `application_name`).
    pub parameters: HashMap<String, String>,
}

impl StartupMessage {
    /// Creates a new startup message with standard parameters.
    pub fn new(user: &str, database: Option<&str>) -> Self {
        let mut params = HashMap::new();
        params.insert("user".to_string(), user.to_string());
        if let Some(db) = database {
            params.insert("database".to_string(), db.to_string());
        }
        params.insert("client_encoding".to_string(), "UTF8".to_string());
        params.insert("application_name".to_string(), "voyager-net".to_string());

        Self {
            protocol_version: PG_PROTOCOL_V3,
            parameters: params,
        }
    }

    /// Serializes startup message into wire bytes.
    /// Note: StartupMessage does not have a 1-byte message type identifier prefix.
    pub fn encode(&self, dst: &mut BytesMut) {
        let mut body = BytesMut::new();
        body.put_i32(self.protocol_version);

        for (k, v) in &self.parameters {
            body.put_slice(k.as_bytes());
            body.put_u8(0);
            body.put_slice(v.as_bytes());
            body.put_u8(0);
        }
        body.put_u8(0); // Terminating null byte

        let total_len = 4 + body.len() as i32;
        dst.put_i32(total_len);
        dst.put_slice(&body);
    }
}

impl FrontendMessage {
    /// Serializes frontend message into wire buffer.
    pub fn encode(&self, dst: &mut BytesMut) {
        match self {
            Self::Startup(msg) => {
                msg.encode(dst);
            }
            Self::Password(pwd) => {
                dst.put_u8(b'p');
                let len = 4 + pwd.len() as i32;
                dst.put_i32(len);
                dst.put_slice(pwd);
            }
            Self::SASLInitialResponse { mechanism, data } => {
                dst.put_u8(b'p');
                let mech_bytes = mechanism.as_bytes();
                let mut body = BytesMut::new();
                body.put_slice(mech_bytes);
                body.put_u8(0);
                match data {
                    Some(d) => {
                        body.put_i32(d.len() as i32);
                        body.put_slice(d);
                    }
                    None => {
                        body.put_i32(-1);
                    }
                }
                let len = 4 + body.len() as i32;
                dst.put_i32(len);
                dst.put_slice(&body);
            }
            Self::SASLResponse(data) => {
                dst.put_u8(b'p');
                let len = 4 + data.len() as i32;
                dst.put_i32(len);
                dst.put_slice(data);
            }
            Self::Query(query) => {
                dst.put_u8(b'Q');
                let q_bytes = query.as_bytes();
                let len = 4 + q_bytes.len() as i32 + 1; // +1 for null terminator
                dst.put_i32(len);
                dst.put_slice(q_bytes);
                dst.put_u8(0);
            }
            Self::Parse {
                name,
                query,
                param_types,
            } => {
                dst.put_u8(b'P');
                let mut body = BytesMut::new();
                body.put_slice(name.as_bytes());
                body.put_u8(0);
                body.put_slice(query.as_bytes());
                body.put_u8(0);
                body.put_i16(param_types.len() as i16);
                for &oid in param_types {
                    body.put_i32(oid);
                }
                let len = 4 + body.len() as i32;
                dst.put_i32(len);
                dst.put_slice(&body);
            }
            Self::Bind {
                portal,
                statement,
                param_formats,
                params,
                result_formats,
            } => {
                dst.put_u8(b'B');
                let mut body = BytesMut::new();
                body.put_slice(portal.as_bytes());
                body.put_u8(0);
                body.put_slice(statement.as_bytes());
                body.put_u8(0);

                // Param formats
                body.put_i16(param_formats.len() as i16);
                for &fmt in param_formats {
                    body.put_i16(fmt);
                }

                // Param values
                body.put_i16(params.len() as i16);
                for param in params {
                    match param {
                        Some(val) => {
                            body.put_i32(val.len() as i32);
                            body.put_slice(val);
                        }
                        None => {
                            body.put_i32(-1);
                        }
                    }
                }

                // Result formats
                body.put_i16(result_formats.len() as i16);
                for &fmt in result_formats {
                    body.put_i16(fmt);
                }

                let len = 4 + body.len() as i32;
                dst.put_i32(len);
                dst.put_slice(&body);
            }
            Self::Execute { portal, max_rows } => {
                dst.put_u8(b'E');
                let mut body = BytesMut::new();
                body.put_slice(portal.as_bytes());
                body.put_u8(0);
                body.put_i32(*max_rows);
                let len = 4 + body.len() as i32;
                dst.put_i32(len);
                dst.put_slice(&body);
            }
            Self::Describe { target_type, name } => {
                dst.put_u8(b'D');
                let mut body = BytesMut::new();
                body.put_u8(*target_type);
                body.put_slice(name.as_bytes());
                body.put_u8(0);
                let len = 4 + body.len() as i32;
                dst.put_i32(len);
                dst.put_slice(&body);
            }
            Self::Close { target_type, name } => {
                dst.put_u8(b'C');
                let mut body = BytesMut::new();
                body.put_u8(*target_type);
                body.put_slice(name.as_bytes());
                body.put_u8(0);
                let len = 4 + body.len() as i32;
                dst.put_i32(len);
                dst.put_slice(&body);
            }
            Self::Sync => {
                dst.put_u8(b'S');
                dst.put_i32(4);
            }
            Self::Flush => {
                dst.put_u8(b'H');
                dst.put_i32(4);
            }
            Self::Terminate => {
                dst.put_u8(b'X');
                dst.put_i32(4);
            }
        }
    }
}

/// PostgreSQL Authentication challenge types received from server (`'R'`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticationRequest {
    /// Authentication successful (`0`).
    Ok,
    /// Kerberos V5 authentication requested (`2`).
    KerberosV5,
    /// Plain cleartext password requested (`3`).
    CleartextPassword,
    /// MD5 password challenge requested (`5`) with 4-byte salt.
    MD5Password {
        /// 4-byte cryptographic salt provided by server.
        salt: [u8; 4],
    },
    /// SCM credential authentication (`6`).
    SCMCredential,
    /// GSSAPI authentication (`7`).
    GSS,
    /// GSSAPI continuation data (`8`).
    GSSContinue(Vec<u8>),
    /// SSPI authentication (`9`).
    SSPI,
    /// SASL authentication mechanisms offered by server (`10`).
    SASL {
        /// Supported SASL mechanisms (e.g. `["SCRAM-SHA-256"]`).
        mechanisms: Vec<String>,
    },
    /// SASL server-first-message challenge data (`11`).
    SASLContinue(Vec<u8>),
    /// SASL server-final-message validation data (`12`).
    SASLFinal(Vec<u8>),
}

/// Column/Field descriptor in a `RowDescription` (`'T'`) message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescription {
    /// Field name / column alias.
    pub name: String,
    /// Table OID (if field is from a table).
    pub table_oid: i32,
    /// Column attribute number in table.
    pub column_attr_num: i16,
    /// PostgreSQL data type OID.
    pub type_oid: i32,
    /// Data type size in bytes (-1 for variable length).
    pub type_size: i16,
    /// Data type modifier.
    pub type_modifier: i32,
    /// Format code: 0 = text format, 1 = binary format.
    pub format_code: i16,
}

/// Server error or notice field dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PgDiagnostic {
    /// Severity level (e.g. `ERROR`, `FATAL`, `PANIC`, `WARNING`, `NOTICE`).
    pub severity: String,
    /// SQLSTATE 5-character error code (e.g. `42P01`, `28P01`).
    pub code: String,
    /// Primary human-readable error message.
    pub message: String,
    /// Optional error detail.
    pub detail: Option<String>,
    /// Optional hint for fixing the error.
    pub hint: Option<String>,
    /// Position indicator inside query string.
    pub position: Option<String>,
    /// Server source code file name.
    pub file: Option<String>,
    /// Server source code line number.
    pub line: Option<String>,
    /// Server source code routine name.
    pub routine: Option<String>,
}

impl PgDiagnostic {
    /// Formats the diagnostic into a concise error string.
    pub fn format_error(&self) -> String {
        let mut out = format!(
            "{}: {} (SQLSTATE {})",
            self.severity, self.message, self.code
        );
        if let Some(detail) = &self.detail {
            out.push_str(&format!("\nDetail: {}", detail));
        }
        if let Some(hint) = &self.hint {
            out.push_str(&format!("\nHint: {}", hint));
        }
        out
    }
}

/// Backend messages sent from PostgreSQL server to client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendMessage {
    /// Authentication challenge or success message (`'R'`).
    Authentication(AuthenticationRequest),
    /// Backend cancellation key data (`'K'`).
    BackendKeyData {
        /// Process ID of this backend server.
        process_id: i32,
        /// Secret cancellation key for canceling running queries.
        secret_key: i32,
    },
    /// Server parameter status notification (`'S'`).
    ParameterStatus {
        /// Parameter name (e.g. `server_version`, `client_encoding`, `TimeZone`).
        name: String,
        /// Current parameter value.
        value: String,
    },
    /// Ready for query / transaction status update (`'Z'`).
    ReadyForQuery(TransactionStatus),
    /// Row description metadata for query result columns (`'T'`).
    RowDescription(Vec<FieldDescription>),
    /// Single row data tuple (`'D'`).
    DataRow(Vec<Option<Bytes>>),
    /// SQL command execution complete (`'C'`).
    CommandComplete(String),
    /// Server error response (`'E'`).
    ErrorResponse(PgDiagnostic),
    /// Server notice / warning response (`'N'`).
    NoticeResponse(PgDiagnostic),
    /// Prepared statement parse complete (`'1'`).
    ParseComplete,
    /// Portal parameter bind complete (`'2'`).
    BindComplete,
    /// Statement or portal close complete (`'3'`).
    CloseComplete,
    /// Statement returned no rows/data (`'n'`).
    NoData,
    /// Executed query string was empty (`'I'`).
    EmptyQueryResponse,
    /// Asynchronous server notification (`'A'`).
    NotificationResponse {
        /// Process ID of notifying backend.
        process_id: i32,
        /// Channel name.
        channel: String,
        /// Notification payload string.
        payload: String,
    },
    /// CopyInResponse (`'G'`).
    CopyInResponse,
    /// CopyOutResponse (`'H'`).
    CopyOutResponse,
    /// CopyData (`'d'`).
    CopyData(Bytes),
    /// CopyDone (`'c'`).
    CopyDone,
}

impl BackendMessage {
    /// Decodes a BackendMessage from raw frame byte payload (excluding the 1-byte type and 4-byte length).
    pub fn decode(msg_type: u8, mut payload: Bytes) -> Result<Self> {
        match msg_type {
            b'R' => {
                if payload.len() < 4 {
                    return Err(NetError::ProtocolError(
                        "Authentication message too short".to_string(),
                    ));
                }
                let auth_type = payload.get_i32();
                match auth_type {
                    0 => Ok(Self::Authentication(AuthenticationRequest::Ok)),
                    2 => Ok(Self::Authentication(AuthenticationRequest::KerberosV5)),
                    3 => Ok(Self::Authentication(
                        AuthenticationRequest::CleartextPassword,
                    )),
                    5 => {
                        if payload.len() < 4 {
                            return Err(NetError::ProtocolError("MD5 salt truncated".to_string()));
                        }
                        let mut salt = [0u8; 4];
                        payload.copy_to_slice(&mut salt);
                        Ok(Self::Authentication(AuthenticationRequest::MD5Password {
                            salt,
                        }))
                    }
                    6 => Ok(Self::Authentication(AuthenticationRequest::SCMCredential)),
                    7 => Ok(Self::Authentication(AuthenticationRequest::GSS)),
                    8 => Ok(Self::Authentication(AuthenticationRequest::GSSContinue(
                        payload.to_vec(),
                    ))),
                    9 => Ok(Self::Authentication(AuthenticationRequest::SSPI)),
                    10 => {
                        let mut mechanisms = Vec::new();
                        while payload.has_remaining() {
                            let mech = read_cstring(&mut payload)?;
                            if mech.is_empty() {
                                break;
                            }
                            mechanisms.push(mech);
                        }
                        Ok(Self::Authentication(AuthenticationRequest::SASL {
                            mechanisms,
                        }))
                    }
                    11 => Ok(Self::Authentication(AuthenticationRequest::SASLContinue(
                        payload.to_vec(),
                    ))),
                    12 => Ok(Self::Authentication(AuthenticationRequest::SASLFinal(
                        payload.to_vec(),
                    ))),
                    other => Err(NetError::AuthenticationFailed(format!(
                        "Unsupported PostgreSQL authentication method type: {}",
                        other
                    ))),
                }
            }
            b'K' => {
                if payload.len() < 8 {
                    return Err(NetError::ProtocolError(
                        "BackendKeyData truncated".to_string(),
                    ));
                }
                let process_id = payload.get_i32();
                let secret_key = payload.get_i32();
                Ok(Self::BackendKeyData {
                    process_id,
                    secret_key,
                })
            }
            b'S' => {
                let name = read_cstring(&mut payload)?;
                let value = read_cstring(&mut payload)?;
                Ok(Self::ParameterStatus { name, value })
            }
            b'Z' => {
                if !payload.has_remaining() {
                    return Err(NetError::ProtocolError(
                        "ReadyForQuery truncated".to_string(),
                    ));
                }
                let status_byte = payload.get_u8();
                let status = TransactionStatus::from_byte(status_byte)?;
                Ok(Self::ReadyForQuery(status))
            }
            b'T' => {
                if payload.len() < 2 {
                    return Err(NetError::ProtocolError(
                        "RowDescription truncated".to_string(),
                    ));
                }
                let field_count = payload.get_i16() as usize;
                let mut fields = Vec::with_capacity(field_count);
                for _ in 0..field_count {
                    let name = read_cstring(&mut payload)?;
                    if payload.len() < 18 {
                        return Err(NetError::ProtocolError(
                            "RowDescription field metadata truncated".to_string(),
                        ));
                    }
                    let table_oid = payload.get_i32();
                    let column_attr_num = payload.get_i16();
                    let type_oid = payload.get_i32();
                    let type_size = payload.get_i16();
                    let type_modifier = payload.get_i32();
                    let format_code = payload.get_i16();

                    fields.push(FieldDescription {
                        name,
                        table_oid,
                        column_attr_num,
                        type_oid,
                        type_size,
                        type_modifier,
                        format_code,
                    });
                }
                Ok(Self::RowDescription(fields))
            }
            b'D' => {
                if payload.len() < 2 {
                    return Err(NetError::ProtocolError("DataRow truncated".to_string()));
                }
                let field_count = payload.get_i16() as usize;
                let mut values = Vec::with_capacity(field_count);
                for _ in 0..field_count {
                    if payload.len() < 4 {
                        return Err(NetError::ProtocolError(
                            "DataRow column length truncated".to_string(),
                        ));
                    }
                    let col_len = payload.get_i32();
                    if col_len == -1 {
                        values.push(None);
                    } else if col_len >= 0 {
                        let len = col_len as usize;
                        if payload.len() < len {
                            return Err(NetError::ProtocolError(
                                "DataRow column payload truncated".to_string(),
                            ));
                        }
                        let val_bytes = payload.split_to(len);
                        values.push(Some(val_bytes));
                    } else {
                        return Err(NetError::ProtocolError(format!(
                            "Invalid column length: {}",
                            col_len
                        )));
                    }
                }
                Ok(Self::DataRow(values))
            }
            b'C' => {
                let tag = read_cstring(&mut payload)?;
                Ok(Self::CommandComplete(tag))
            }
            b'E' => {
                let diagnostic = decode_diagnostic(&mut payload)?;
                Ok(Self::ErrorResponse(diagnostic))
            }
            b'N' => {
                let diagnostic = decode_diagnostic(&mut payload)?;
                Ok(Self::NoticeResponse(diagnostic))
            }
            b'1' => Ok(Self::ParseComplete),
            b'2' => Ok(Self::BindComplete),
            b'3' => Ok(Self::CloseComplete),
            b'n' => Ok(Self::NoData),
            b'I' => Ok(Self::EmptyQueryResponse),
            b'A' => {
                if payload.len() < 4 {
                    return Err(NetError::ProtocolError(
                        "NotificationResponse truncated".to_string(),
                    ));
                }
                let process_id = payload.get_i32();
                let channel = read_cstring(&mut payload)?;
                let pl = read_cstring(&mut payload)?;
                Ok(Self::NotificationResponse {
                    process_id,
                    channel,
                    payload: pl,
                })
            }
            b'G' => Ok(Self::CopyInResponse),
            b'H' => Ok(Self::CopyOutResponse),
            b'd' => Ok(Self::CopyData(payload)),
            b'c' => Ok(Self::CopyDone),
            other => Err(NetError::ProtocolError(format!(
                "Unknown backend message type: '{}' (0x{:02x})",
                other as char, other
            ))),
        }
    }
}

/// Reads a null-terminated UTF-8 string from a byte buffer.
pub fn read_cstring(buf: &mut Bytes) -> Result<String> {
    if let Some(pos) = buf.iter().position(|&b| b == 0) {
        let str_bytes = buf.split_to(pos);
        buf.advance(1); // Advance past the '\0' byte
        let s = String::from_utf8(str_bytes.to_vec())
            .map_err(|e| NetError::ProtocolError(format!("Invalid UTF-8 string: {}", e)))?;
        Ok(s)
    } else {
        Err(NetError::ProtocolError(
            "Null terminator not found in string".to_string(),
        ))
    }
}

/// Decodes diagnostic fields from ErrorResponse (`'E'`) or NoticeResponse (`'N'`).
fn decode_diagnostic(buf: &mut Bytes) -> Result<PgDiagnostic> {
    let mut diag = PgDiagnostic::default();
    while buf.has_remaining() {
        let field_type = buf.get_u8();
        if field_type == 0 {
            break;
        }
        let val = read_cstring(buf)?;
        match field_type {
            b'S' | b'V' => diag.severity = val,
            b'C' => diag.code = val,
            b'M' => diag.message = val,
            b'D' => diag.detail = Some(val),
            b'H' => diag.hint = Some(val),
            b'P' => diag.position = Some(val),
            b'F' => diag.file = Some(val),
            b'L' => diag.line = Some(val),
            b'R' => diag.routine = Some(val),
            _ => {} // Ignore unhandled diagnostic field codes
        }
    }
    Ok(diag)
}
