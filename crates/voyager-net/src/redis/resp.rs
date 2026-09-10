//! Redis Serialization Protocol (RESP2 and RESP3) wire types, parser, and serializer.
//!
//! Implements the full RESP specification (RESP2 and RESP3) in pure safe Rust:
//! - Standard types: Simple Strings (`+`), Errors (`-`), Integers (`:`), Bulk Strings (`$`), Arrays (`*`)
//! - RESP3 types: Null (`_`), Double (`,`), Boolean (`#`), Blob Error (`!`), Verbatim String (`=`),
//!   Big Number (`(`), Map (`%`), Set (`~`), Push (`>`)

use bytes::BytesMut;
use std::collections::HashMap;
use std::fmt;

use crate::error::{NetError, Result};

/// Maximum recursion depth allowed when parsing nested aggregate structures (Arrays, Maps, Sets).
pub const MAX_RESP_DEPTH: usize = 128;

/// A parsed value according to the Redis Serialization Protocol (RESP2 & RESP3).
#[derive(Debug, Clone, PartialEq)]
pub enum RespValue {
    /// Space-efficient non-binary safe string (`+<string>\r\n`).
    SimpleString(String),
    /// Space-efficient error code and message (`-<error>\r\n`).
    Error(String),
    /// Signed 64-bit integer (`:<number>\r\n`).
    Integer(i64),
    /// Binary safe string (`$<len>\r\n<bytes>\r\n`). `None` represents null bulk string (`$-1\r\n`).
    BulkString(Option<Vec<u8>>),
    /// Ordered collection of other RESP values (`*<len>\r\n...`). `None` represents null array (`*-1\r\n`).
    Array(Option<Vec<RespValue>>),
    /// RESP3 Null value (`_\r\n`).
    Null,
    /// RESP3 64-bit floating point number (`,<float>\r\n`).
    Double(f64),
    /// RESP3 Boolean (`#t\r\n` or `#f\r\n`).
    Boolean(bool),
    /// RESP3 Binary-safe error code and message (`!<len>\r\n<bytes>\r\n`).
    BlobError(Vec<u8>),
    /// RESP3 Verbatim string with a 3-character format specifier (`=<len>\r\n<fmt>:<bytes>\r\n`).
    VerbatimString {
        /// 3-byte format specifier (e.g. `txt` or `markdown`).
        format: [u8; 3],
        /// Verbatim content payload.
        text: Vec<u8>,
    },
    /// RESP3 Arbitrary precision integer (`(<big_number>\r\n`).
    BigNumber(String),
    /// RESP3 Unordered collection of key-value pairs (`%<len>\r\n...`).
    Map(Vec<(RespValue, RespValue)>),
    /// RESP3 Unordered collection of unique items (`~<len>\r\n...`).
    Set(Vec<RespValue>),
    /// RESP3 Out-of-band asynchronous push notification (`><len>\r\n...`).
    Push(Vec<RespValue>),
}

impl RespValue {
    /// Attempts to parse a single `RespValue` from the provided buffer.
    ///
    /// If a complete frame is available, advances the buffer and returns `Ok(Some(value))`.
    /// If the buffer does not yet contain a complete frame, returns `Ok(None)` without consuming bytes.
    pub fn parse(src: &mut BytesMut) -> Result<Option<Self>> {
        match parse_resp_slice(src, 0)? {
            Some((val, consumed)) => {
                use bytes::Buf;
                src.advance(consumed);
                Ok(Some(val))
            }
            None => Ok(None),
        }
    }

    /// Serializes this `RespValue` into the destination buffer.
    pub fn encode(&self, dst: &mut BytesMut) {
        match self {
            Self::SimpleString(s) => {
                dst.extend_from_slice(b"+");
                dst.extend_from_slice(s.as_bytes());
                dst.extend_from_slice(b"\r\n");
            }
            Self::Error(e) => {
                dst.extend_from_slice(b"-");
                dst.extend_from_slice(e.as_bytes());
                dst.extend_from_slice(b"\r\n");
            }
            Self::Integer(i) => {
                let s = format!(":{}\r\n", i);
                dst.extend_from_slice(s.as_bytes());
            }
            Self::BulkString(None) => {
                dst.extend_from_slice(b"$-1\r\n");
            }
            Self::BulkString(Some(b)) => {
                let header = format!("${}\r\n", b.len());
                dst.extend_from_slice(header.as_bytes());
                dst.extend_from_slice(b);
                dst.extend_from_slice(b"\r\n");
            }
            Self::Array(None) => {
                dst.extend_from_slice(b"*-1\r\n");
            }
            Self::Array(Some(items)) => {
                let header = format!("*{}\r\n", items.len());
                dst.extend_from_slice(header.as_bytes());
                for item in items {
                    item.encode(dst);
                }
            }
            Self::Null => {
                dst.extend_from_slice(b"_\r\n");
            }
            Self::Double(f) => {
                if f.is_nan() {
                    dst.extend_from_slice(b",nan\r\n");
                } else if f.is_infinite() {
                    if *f > 0.0 {
                        dst.extend_from_slice(b",inf\r\n");
                    } else {
                        dst.extend_from_slice(b",-inf\r\n");
                    }
                } else {
                    let s = format!(",{}\r\n", f);
                    dst.extend_from_slice(s.as_bytes());
                }
            }
            Self::Boolean(b) => {
                if *b {
                    dst.extend_from_slice(b"#t\r\n");
                } else {
                    dst.extend_from_slice(b"#f\r\n");
                }
            }
            Self::BlobError(b) => {
                let header = format!("!{}\r\n", b.len());
                dst.extend_from_slice(header.as_bytes());
                dst.extend_from_slice(b);
                dst.extend_from_slice(b"\r\n");
            }
            Self::VerbatimString { format, text } => {
                let total_len = 4 + text.len();
                let header = format!("={}\r\n", total_len);
                dst.extend_from_slice(header.as_bytes());
                dst.extend_from_slice(format);
                dst.extend_from_slice(b":");
                dst.extend_from_slice(text);
                dst.extend_from_slice(b"\r\n");
            }
            Self::BigNumber(num) => {
                let s = format!("({}\r\n", num);
                dst.extend_from_slice(s.as_bytes());
            }
            Self::Map(pairs) => {
                let header = format!("%{}\r\n", pairs.len());
                dst.extend_from_slice(header.as_bytes());
                for (k, v) in pairs {
                    k.encode(dst);
                    v.encode(dst);
                }
            }
            Self::Set(items) => {
                let header = format!("~{}\r\n", items.len());
                dst.extend_from_slice(header.as_bytes());
                for item in items {
                    item.encode(dst);
                }
            }
            Self::Push(items) => {
                let header = format!(">{}\r\n", items.len());
                dst.extend_from_slice(header.as_bytes());
                for item in items {
                    item.encode(dst);
                }
            }
        }
    }

    /// Serializes this `RespValue` to a new `Vec<u8>`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        self.encode(&mut buf);
        buf.to_vec()
    }

    /// Serializes a Redis command array of bulk strings (e.g. `["GRAPH.QUERY", "db", "RETURN 1"]`).
    pub fn encode_command(args: &[&str]) -> Vec<u8> {
        let mut buf = BytesMut::new();
        let header = format!("*{}\r\n", args.len());
        buf.extend_from_slice(header.as_bytes());
        for arg in args {
            let item_hdr = format!("${}\r\n", arg.len());
            buf.extend_from_slice(item_hdr.as_bytes());
            buf.extend_from_slice(arg.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        buf.to_vec()
    }

    /// Returns `true` if this value represents a Null (RESP3 `_`, or null bulk string `$-1`, or null array `*-1`).
    pub fn is_null(&self) -> bool {
        matches!(
            self,
            Self::Null | Self::BulkString(None) | Self::Array(None)
        )
    }

    /// Borrows this value as a string slice if it is a `SimpleString` or a UTF-8 `BulkString`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::SimpleString(s) => Some(s.as_str()),
            Self::BulkString(Some(b)) => std::str::from_utf8(b).ok(),
            _ => None,
        }
    }

    /// Converts this value into a String if string-like.
    pub fn to_string_lossy(&self) -> String {
        match self {
            Self::SimpleString(s) => s.clone(),
            Self::Error(e) => format!("ERROR: {}", e),
            Self::Integer(i) => i.to_string(),
            Self::BulkString(Some(b)) => String::from_utf8_lossy(b).into_owned(),
            Self::BulkString(None) | Self::Null => "null".to_string(),
            Self::Double(f) => f.to_string(),
            Self::Boolean(b) => b.to_string(),
            Self::BigNumber(n) => n.clone(),
            Self::Array(Some(arr)) => {
                let items: Vec<String> = arr.iter().map(|x| x.to_string_lossy()).collect();
                format!("[{}]", items.join(", "))
            }
            Self::Array(None) => "null".to_string(),
            Self::Map(pairs) => {
                let kvs: Vec<String> = pairs
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.to_string_lossy(), v.to_string_lossy()))
                    .collect();
                format!("{{{}}}", kvs.join(", "))
            }
            Self::Set(items) => {
                let s: Vec<String> = items.iter().map(|x| x.to_string_lossy()).collect();
                format!("{{{}}}", s.join(", "))
            }
            _ => format!("{:?}", self),
        }
    }

    /// Extracts an integer value if this is an `Integer` or integer-formatted `BulkString`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            Self::BulkString(Some(b)) => std::str::from_utf8(b).ok()?.parse::<i64>().ok(),
            Self::SimpleString(s) => s.parse::<i64>().ok(),
            _ => None,
        }
    }

    /// Extracts a floating point value if this is a `Double`, `Integer`, or float-formatted `BulkString`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(f) => Some(*f),
            Self::Integer(i) => Some(*i as f64),
            Self::BulkString(Some(b)) => std::str::from_utf8(b).ok()?.parse::<f64>().ok(),
            Self::SimpleString(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    /// Extracts a boolean value if this is a `Boolean` or boolean-formatted string.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            Self::SimpleString(s) => match s.to_lowercase().as_str() {
                "true" | "t" | "1" => Some(true),
                "false" | "f" | "0" => Some(false),
                _ => None,
            },
            Self::BulkString(Some(b)) => match std::str::from_utf8(b).ok()?.to_lowercase().as_str()
            {
                "true" | "t" | "1" => Some(true),
                "false" | "f" | "0" => Some(false),
                _ => None,
            },
            Self::Integer(i) => Some(*i != 0),
            _ => None,
        }
    }

    /// Borrows the raw bytes if this is a `BulkString` or `BlobError`.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::BulkString(Some(b)) => Some(b.as_slice()),
            Self::BlobError(b) => Some(b.as_slice()),
            _ => None,
        }
    }

    /// Borrows elements if this is an `Array`.
    pub fn as_array(&self) -> Option<&[RespValue]> {
        match self {
            Self::Array(Some(arr)) => Some(arr.as_slice()),
            _ => None,
        }
    }

    /// Borrows pairs if this is a `Map`.
    pub fn as_map(&self) -> Option<&[(RespValue, RespValue)]> {
        match self {
            Self::Map(pairs) => Some(pairs.as_slice()),
            _ => None,
        }
    }

    /// Converts a RESP Map or 2-element array pairs into a string-keyed HashMap.
    pub fn to_hashmap(&self) -> HashMap<String, RespValue> {
        let mut map = HashMap::new();
        if let Some(pairs) = self.as_map() {
            for (k, v) in pairs {
                map.insert(k.to_string_lossy(), v.clone());
            }
        } else if let Some(items) = self.as_array() {
            // Check if array is structured as [[k1, v1], [k2, v2]] (FalkorDB properties format)
            for item in items {
                if let Some(pair) = item.as_array()
                    && pair.len() == 2
                {
                    map.insert(pair[0].to_string_lossy(), pair[1].clone());
                }
            }
        }
        map
    }
}

impl fmt::Display for RespValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_lossy())
    }
}

/// Finds the first `\r\n` in slice, returning the index of `\r`.
fn find_crlf(src: &[u8]) -> Option<usize> {
    (0..src.len().saturating_sub(1)).find(|&i| src[i] == b'\r' && src[i + 1] == b'\n')
}

/// Parses a line up to `\r\n`, returning the line slice and total consumed bytes (including `\r\n`).
fn parse_line(src: &[u8]) -> Option<(&[u8], usize)> {
    let crlf_idx = find_crlf(src)?;
    Some((&src[..crlf_idx], crlf_idx + 2))
}

/// Internal slice-based parser returning `Ok(Some((value, total_bytes_consumed)))`.
fn parse_resp_slice(src: &[u8], depth: usize) -> Result<Option<(RespValue, usize)>> {
    if depth > MAX_RESP_DEPTH {
        return Err(NetError::ProtocolError(format!(
            "RESP structure exceeded maximum nesting depth of {}",
            MAX_RESP_DEPTH
        )));
    }
    if src.is_empty() {
        return Ok(None);
    }

    let type_marker = src[0];
    let rest = &src[1..];

    match type_marker {
        // Simple String: +<str>\r\n
        b'+' => match parse_line(rest) {
            Some((line, consumed)) => {
                let s = std::str::from_utf8(line).map_err(|e| {
                    NetError::ProtocolError(format!("Invalid UTF-8 simple string: {}", e))
                })?;
                Ok(Some((RespValue::SimpleString(s.to_string()), consumed + 1)))
            }
            None => Ok(None),
        },

        // Simple Error: -<str>\r\n
        b'-' => match parse_line(rest) {
            Some((line, consumed)) => {
                let s = std::str::from_utf8(line).map_err(|e| {
                    NetError::ProtocolError(format!("Invalid UTF-8 error string: {}", e))
                })?;
                Ok(Some((RespValue::Error(s.to_string()), consumed + 1)))
            }
            None => Ok(None),
        },

        // Integer: :<num>\r\n
        b':' => match parse_line(rest) {
            Some((line, consumed)) => {
                let line_str = std::str::from_utf8(line).map_err(|e| {
                    NetError::ProtocolError(format!("Invalid UTF-8 integer string: {}", e))
                })?;
                let num = line_str.parse::<i64>().map_err(|e| {
                    NetError::ProtocolError(format!("Malformed RESP integer '{}': {}", line_str, e))
                })?;
                Ok(Some((RespValue::Integer(num), consumed + 1)))
            }
            None => Ok(None),
        },

        // Bulk String: $<len>\r\n<bytes>\r\n
        b'$' => match parse_line(rest) {
            Some((len_line, hdr_consumed)) => {
                let len_str = std::str::from_utf8(len_line).map_err(|e| {
                    NetError::ProtocolError(format!("Invalid bulk string length: {}", e))
                })?;
                let len = len_str.parse::<i64>().map_err(|e| {
                    NetError::ProtocolError(format!(
                        "Malformed bulk string length '{}': {}",
                        len_str, e
                    ))
                })?;

                if len < -1 {
                    return Err(NetError::ProtocolError(format!(
                        "Negative bulk string length: {}",
                        len
                    )));
                }

                if len == -1 {
                    // Null bulk string ($-1\r\n)
                    return Ok(Some((RespValue::BulkString(None), hdr_consumed + 1)));
                }

                let byte_len = len as usize;
                let payload_start = hdr_consumed;
                let payload_end = payload_start + byte_len;
                let total_consumed = payload_end + 2; // +2 for trailing \r\n

                if rest.len() < total_consumed {
                    // Incomplete payload
                    return Ok(None);
                }

                // Verify trailing \r\n
                if rest[payload_end] != b'\r' || rest[payload_end + 1] != b'\n' {
                    return Err(NetError::ProtocolError(
                        "Bulk string payload missing trailing CRLF".to_string(),
                    ));
                }

                let bytes = rest[payload_start..payload_end].to_vec();
                Ok(Some((
                    RespValue::BulkString(Some(bytes)),
                    total_consumed + 1,
                )))
            }
            None => Ok(None),
        },

        // Array: *<len>\r\n...
        b'*' => match parse_line(rest) {
            Some((len_line, hdr_consumed)) => {
                let len_str = std::str::from_utf8(len_line)
                    .map_err(|e| NetError::ProtocolError(format!("Invalid array length: {}", e)))?;
                let len = len_str.parse::<i64>().map_err(|e| {
                    NetError::ProtocolError(format!("Malformed array length '{}': {}", len_str, e))
                })?;

                if len < -1 {
                    return Err(NetError::ProtocolError(format!(
                        "Negative array length: {}",
                        len
                    )));
                }

                if len == -1 {
                    // Null array (*-1\r\n)
                    return Ok(Some((RespValue::Array(None), hdr_consumed + 1)));
                }

                let num_elements = len as usize;
                let mut elements = Vec::with_capacity(num_elements);
                let mut offset = hdr_consumed;

                for _ in 0..num_elements {
                    if offset >= rest.len() {
                        return Ok(None);
                    }
                    match parse_resp_slice(&rest[offset..], depth + 1)? {
                        Some((item, item_consumed)) => {
                            elements.push(item);
                            offset += item_consumed;
                        }
                        None => return Ok(None),
                    }
                }

                Ok(Some((RespValue::Array(Some(elements)), offset + 1)))
            }
            None => Ok(None),
        },

        // RESP3 Null: _\r\n
        b'_' => {
            if rest.len() < 2 {
                return Ok(None);
            }
            if rest[0] == b'\r' && rest[1] == b'\n' {
                Ok(Some((RespValue::Null, 3)))
            } else {
                Err(NetError::ProtocolError(
                    "Malformed RESP3 Null: expected CRLF after '_'".to_string(),
                ))
            }
        }

        // RESP3 Double: ,<float>\r\n
        b',' => match parse_line(rest) {
            Some((line, consumed)) => {
                let s = std::str::from_utf8(line).map_err(|e| {
                    NetError::ProtocolError(format!("Invalid double string: {}", e))
                })?;
                let val = match s {
                    "inf" | "+inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    "nan" => f64::NAN,
                    other => other.parse::<f64>().map_err(|e| {
                        NetError::ProtocolError(format!(
                            "Malformed RESP3 double '{}': {}",
                            other, e
                        ))
                    })?,
                };
                Ok(Some((RespValue::Double(val), consumed + 1)))
            }
            None => Ok(None),
        },

        // RESP3 Boolean: #t\r\n or #f\r\n
        b'#' => {
            if rest.len() < 3 {
                return Ok(None);
            }
            if rest[1] == b'\r' && rest[2] == b'\n' {
                let b = match rest[0] {
                    b't' => true,
                    b'f' => false,
                    other => {
                        return Err(NetError::ProtocolError(format!(
                            "Invalid RESP3 boolean character: '{}'",
                            other as char
                        )));
                    }
                };
                Ok(Some((RespValue::Boolean(b), 4)))
            } else {
                Err(NetError::ProtocolError(
                    "Malformed RESP3 boolean missing CRLF".to_string(),
                ))
            }
        }

        // RESP3 Blob Error: !<len>\r\n<bytes>\r\n
        b'!' => match parse_line(rest) {
            Some((len_line, hdr_consumed)) => {
                let len_str = std::str::from_utf8(len_line).map_err(|e| {
                    NetError::ProtocolError(format!("Invalid blob error length: {}", e))
                })?;
                let len = len_str.parse::<usize>().map_err(|e| {
                    NetError::ProtocolError(format!(
                        "Malformed blob error length '{}': {}",
                        len_str, e
                    ))
                })?;

                let payload_start = hdr_consumed;
                let payload_end = payload_start + len;
                let total_consumed = payload_end + 2;

                if rest.len() < total_consumed {
                    return Ok(None);
                }

                if rest[payload_end] != b'\r' || rest[payload_end + 1] != b'\n' {
                    return Err(NetError::ProtocolError(
                        "Blob error payload missing trailing CRLF".to_string(),
                    ));
                }

                let bytes = rest[payload_start..payload_end].to_vec();
                Ok(Some((RespValue::BlobError(bytes), total_consumed + 1)))
            }
            None => Ok(None),
        },

        // RESP3 Verbatim String: =<len>\r\n<fmt>:<bytes>\r\n
        b'=' => match parse_line(rest) {
            Some((len_line, hdr_consumed)) => {
                let len_str = std::str::from_utf8(len_line).map_err(|e| {
                    NetError::ProtocolError(format!("Invalid verbatim length: {}", e))
                })?;
                let len = len_str.parse::<usize>().map_err(|e| {
                    NetError::ProtocolError(format!(
                        "Malformed verbatim length '{}': {}",
                        len_str, e
                    ))
                })?;

                if len < 4 {
                    return Err(NetError::ProtocolError(
                        "Verbatim string length must be at least 4 bytes (format + ':')"
                            .to_string(),
                    ));
                }

                let payload_start = hdr_consumed;
                let payload_end = payload_start + len;
                let total_consumed = payload_end + 2;

                if rest.len() < total_consumed {
                    return Ok(None);
                }

                if rest[payload_end] != b'\r' || rest[payload_end + 1] != b'\n' {
                    return Err(NetError::ProtocolError(
                        "Verbatim string payload missing trailing CRLF".to_string(),
                    ));
                }

                let raw = &rest[payload_start..payload_end];
                if raw[3] != b':' {
                    return Err(NetError::ProtocolError(
                        "Verbatim string missing ':' after 3-byte format".to_string(),
                    ));
                }

                let mut fmt_bytes = [0u8; 3];
                fmt_bytes.copy_from_slice(&raw[0..3]);
                let text = raw[4..].to_vec();

                Ok(Some((
                    RespValue::VerbatimString {
                        format: fmt_bytes,
                        text,
                    },
                    total_consumed + 1,
                )))
            }
            None => Ok(None),
        },

        // RESP3 Big Number: (<num>\r\n
        b'(' => match parse_line(rest) {
            Some((line, consumed)) => {
                let s = std::str::from_utf8(line).map_err(|e| {
                    NetError::ProtocolError(format!("Invalid big number string: {}", e))
                })?;
                Ok(Some((RespValue::BigNumber(s.to_string()), consumed + 1)))
            }
            None => Ok(None),
        },

        // RESP3 Map: %<len>\r\n... (each pair has key and value)
        b'%' => match parse_line(rest) {
            Some((len_line, hdr_consumed)) => {
                let len_str = std::str::from_utf8(len_line)
                    .map_err(|e| NetError::ProtocolError(format!("Invalid map length: {}", e)))?;
                let num_pairs = len_str.parse::<usize>().map_err(|e| {
                    NetError::ProtocolError(format!("Malformed map length '{}': {}", len_str, e))
                })?;

                let mut pairs = Vec::with_capacity(num_pairs);
                let mut offset = hdr_consumed;

                for _ in 0..num_pairs {
                    // Key
                    if offset >= rest.len() {
                        return Ok(None);
                    }
                    let key = match parse_resp_slice(&rest[offset..], depth + 1)? {
                        Some((k, k_consumed)) => {
                            offset += k_consumed;
                            k
                        }
                        None => return Ok(None),
                    };

                    // Value
                    if offset >= rest.len() {
                        return Ok(None);
                    }
                    let val = match parse_resp_slice(&rest[offset..], depth + 1)? {
                        Some((v, v_consumed)) => {
                            offset += v_consumed;
                            v
                        }
                        None => return Ok(None),
                    };

                    pairs.push((key, val));
                }

                Ok(Some((RespValue::Map(pairs), offset + 1)))
            }
            None => Ok(None),
        },

        // RESP3 Set: ~<len>\r\n...
        b'~' => match parse_line(rest) {
            Some((len_line, hdr_consumed)) => {
                let len_str = std::str::from_utf8(len_line)
                    .map_err(|e| NetError::ProtocolError(format!("Invalid set length: {}", e)))?;
                let num_items = len_str.parse::<usize>().map_err(|e| {
                    NetError::ProtocolError(format!("Malformed set length '{}': {}", len_str, e))
                })?;

                let mut items = Vec::with_capacity(num_items);
                let mut offset = hdr_consumed;

                for _ in 0..num_items {
                    if offset >= rest.len() {
                        return Ok(None);
                    }
                    match parse_resp_slice(&rest[offset..], depth + 1)? {
                        Some((item, item_consumed)) => {
                            items.push(item);
                            offset += item_consumed;
                        }
                        None => return Ok(None),
                    }
                }

                Ok(Some((RespValue::Set(items), offset + 1)))
            }
            None => Ok(None),
        },

        // RESP3 Push: ><len>\r\n...
        b'>' => match parse_line(rest) {
            Some((len_line, hdr_consumed)) => {
                let len_str = std::str::from_utf8(len_line)
                    .map_err(|e| NetError::ProtocolError(format!("Invalid push length: {}", e)))?;
                let num_items = len_str.parse::<usize>().map_err(|e| {
                    NetError::ProtocolError(format!("Malformed push length '{}': {}", len_str, e))
                })?;

                let mut items = Vec::with_capacity(num_items);
                let mut offset = hdr_consumed;

                for _ in 0..num_items {
                    if offset >= rest.len() {
                        return Ok(None);
                    }
                    match parse_resp_slice(&rest[offset..], depth + 1)? {
                        Some((item, item_consumed)) => {
                            items.push(item);
                            offset += item_consumed;
                        }
                        None => return Ok(None),
                    }
                }

                Ok(Some((RespValue::Push(items), offset + 1)))
            }
            None => Ok(None),
        },

        other => Err(NetError::ProtocolError(format!(
            "Unrecognized RESP protocol type marker: '{}' (0x{:02x})",
            other as char, other
        ))),
    }
}
