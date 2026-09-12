//! PackStream binary serialization and deserialization for Bolt wire protocols.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{NetError, Result};

/// Represents a graph Node decoded from a Bolt PackStream structure (`tag = 0x4E`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltNode {
    /// Internal integer ID (Bolt v1-v4).
    pub id: i64,
    /// List of node labels (e.g. `["Person", "Developer"]`).
    pub labels: Vec<String>,
    /// Property key-value map.
    pub properties: HashMap<String, BoltValue>,
    /// String element ID (Bolt v5+).
    pub element_id: Option<String>,
}

/// Represents a graph Relationship decoded from a Bolt PackStream structure (`tag = 0x52`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltRelationship {
    /// Internal integer ID (Bolt v1-v4).
    pub id: i64,
    /// Start node internal integer ID.
    pub start_node_id: i64,
    /// End node internal integer ID.
    pub end_node_id: i64,
    /// Relationship type label (e.g. `"FOLLOWS"`).
    pub rel_type: String,
    /// Property key-value map.
    pub properties: HashMap<String, BoltValue>,
    /// String element ID (Bolt v5+).
    pub element_id: Option<String>,
    /// Start node element ID (Bolt v5+).
    pub start_element_id: Option<String>,
    /// End node element ID (Bolt v5+).
    pub end_element_id: Option<String>,
}

/// Represents an unbound relationship inside a graph Path (`tag = 0x72`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltUnboundRelationship {
    /// Internal integer ID.
    pub id: i64,
    /// Relationship type label.
    pub rel_type: String,
    /// Property key-value map.
    pub properties: HashMap<String, BoltValue>,
    /// String element ID.
    pub element_id: Option<String>,
}

/// Represents a graph Path decoded from a Bolt PackStream structure (`tag = 0x50`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoltPath {
    /// Ordered list of unique nodes in the path.
    pub nodes: Vec<BoltNode>,
    /// Ordered list of unique unbound relationships in the path.
    pub relationships: Vec<BoltUnboundRelationship>,
    /// Traversal sequence indices connecting nodes and relationships.
    pub sequence: Vec<i64>,
}

/// Represents any dynamically typed value supported by Bolt PackStream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BoltValue {
    /// Null / empty value (`0xC0`).
    Null,
    /// Boolean flag (`0xC2` false, `0xC3` true).
    Boolean(bool),
    /// 64-bit signed integer.
    Integer(i64),
    /// 64-bit IEEE-754 floating point number (`0xC1`).
    Float(f64),
    /// Raw binary byte buffer (`0xCC`..`0xCE`).
    Bytes(Vec<u8>),
    /// UTF-8 encoded string.
    String(String),
    /// Ordered list of PackStream values.
    List(Vec<BoltValue>),
    /// Key-value dictionary.
    Map(HashMap<String, BoltValue>),
    /// Generic PackStream structure with 1-byte signature tag.
    Structure {
        /// 1-byte structure signature tag.
        tag: u8,
        /// Ordered field elements.
        fields: Vec<BoltValue>,
    },
    /// Graph Node structure (`0x4E`).
    Node(BoltNode),
    /// Graph Relationship structure (`0x52`).
    Relationship(BoltRelationship),
    /// Graph Path structure (`0x50`).
    Path(BoltPath),
}

impl From<bool> for BoltValue {
    fn from(b: bool) -> Self {
        Self::Boolean(b)
    }
}

impl From<i64> for BoltValue {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}

impl From<i32> for BoltValue {
    fn from(i: i32) -> Self {
        Self::Integer(i as i64)
    }
}

impl From<f64> for BoltValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<String> for BoltValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for BoltValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<Vec<BoltValue>> for BoltValue {
    fn from(list: Vec<BoltValue>) -> Self {
        Self::List(list)
    }
}

impl From<HashMap<String, BoltValue>> for BoltValue {
    fn from(map: HashMap<String, BoltValue>) -> Self {
        Self::Map(map)
    }
}

impl From<&serde_json::Value> for BoltValue {
    fn from(json: &serde_json::Value) -> Self {
        match json {
            serde_json::Value::Null => BoltValue::Null,
            serde_json::Value::Bool(b) => BoltValue::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    BoltValue::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    BoltValue::Float(f)
                } else {
                    BoltValue::Null
                }
            }
            serde_json::Value::String(s) => BoltValue::String(s.clone()),
            serde_json::Value::Array(arr) => {
                BoltValue::List(arr.iter().map(BoltValue::from).collect())
            }
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::with_capacity(obj.len());
                for (k, v) in obj {
                    map.insert(k.clone(), BoltValue::from(v));
                }
                BoltValue::Map(map)
            }
        }
    }
}

/// PackStream encoder and decoder implementation.
pub struct PackStream;

impl PackStream {
    /// Encodes a `BoltValue` into a mutable byte buffer.
    pub fn encode(value: &BoltValue, buf: &mut BytesMut) {
        match value {
            BoltValue::Null => buf.put_u8(0xC0),
            BoltValue::Boolean(false) => buf.put_u8(0xC2),
            BoltValue::Boolean(true) => buf.put_u8(0xC3),
            BoltValue::Integer(i) => Self::encode_integer(*i, buf),
            BoltValue::Float(f) => {
                buf.put_u8(0xC1);
                buf.put_f64(*f);
            }
            BoltValue::Bytes(bytes) => Self::encode_bytes(bytes, buf),
            BoltValue::String(s) => Self::encode_string(s, buf),
            BoltValue::List(list) => Self::encode_list(list, buf),
            BoltValue::Map(map) => Self::encode_map(map, buf),
            BoltValue::Structure { tag, fields } => Self::encode_structure(*tag, fields, buf),
            BoltValue::Node(node) => Self::encode_node(node, buf),
            BoltValue::Relationship(rel) => Self::encode_relationship(rel, buf),
            BoltValue::Path(path) => Self::encode_path(path, buf),
        }
    }

    /// Decodes a `BoltValue` from a byte buffer slice.
    pub fn decode(buf: &mut Bytes) -> Result<BoltValue> {
        if !buf.has_remaining() {
            return Err(NetError::ProtocolError(
                "Unexpected EOF while decoding PackStream value".to_string(),
            ));
        }

        let marker = buf.get_u8();

        // 1. TinyInt (0x00..0x7F positive, 0xF0..0xFF negative)
        if marker <= 0x7F {
            return Ok(BoltValue::Integer(marker as i64));
        }
        if marker >= 0xF0 {
            return Ok(BoltValue::Integer((marker as i8) as i64));
        }

        // 2. TinyString (0x80..0x8F)
        if (0x80..=0x8F).contains(&marker) {
            let len = (marker & 0x0F) as usize;
            return Self::decode_string_payload(len, buf);
        }

        // 3. TinyList (0x90..0x9F)
        if (0x90..=0x9F).contains(&marker) {
            let len = (marker & 0x0F) as usize;
            return Self::decode_list_payload(len, buf);
        }

        // 4. TinyMap (0xA0..0xAF)
        if (0xA0..=0xAF).contains(&marker) {
            let size = (marker & 0x0F) as usize;
            return Self::decode_map_payload(size, buf);
        }

        // 5. TinyStruct (0xB0..0xBF)
        if (0xB0..=0xBF).contains(&marker) {
            let num_fields = (marker & 0x0F) as usize;
            return Self::decode_structure_payload(num_fields, buf);
        }

        // 6. Explicit Marker Bytes
        match marker {
            0xC0 => Ok(BoltValue::Null),
            0xC1 => {
                if buf.remaining() < 8 {
                    return Err(NetError::ProtocolError("Incomplete Float64".to_string()));
                }
                Ok(BoltValue::Float(buf.get_f64()))
            }
            0xC2 => Ok(BoltValue::Boolean(false)),
            0xC3 => Ok(BoltValue::Boolean(true)),
            0xC8 => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError("Incomplete Int8".to_string()));
                }
                Ok(BoltValue::Integer(buf.get_i8() as i64))
            }
            0xC9 => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError("Incomplete Int16".to_string()));
                }
                Ok(BoltValue::Integer(buf.get_i16() as i64))
            }
            0xCA => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError("Incomplete Int32".to_string()));
                }
                Ok(BoltValue::Integer(buf.get_i32() as i64))
            }
            0xCB => {
                if buf.remaining() < 8 {
                    return Err(NetError::ProtocolError("Incomplete Int64".to_string()));
                }
                Ok(BoltValue::Integer(buf.get_i64()))
            }
            0xCC => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Bytes8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_bytes_payload(len, buf)
            }
            0xCD => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Bytes16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_bytes_payload(len, buf)
            }
            0xCE => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Bytes32 length".to_string(),
                    ));
                }
                let len = buf.get_u32() as usize;
                Self::decode_bytes_payload(len, buf)
            }
            0xD0 => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete String8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_string_payload(len, buf)
            }
            0xD1 => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete String16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_string_payload(len, buf)
            }
            0xD2 => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError(
                        "Incomplete String32 length".to_string(),
                    ));
                }
                let len = buf.get_u32() as usize;
                Self::decode_string_payload(len, buf)
            }
            0xD4 => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete List8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_list_payload(len, buf)
            }
            0xD5 => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete List16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_list_payload(len, buf)
            }
            0xD6 => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError(
                        "Incomplete List32 length".to_string(),
                    ));
                }
                let len = buf.get_u32() as usize;
                Self::decode_list_payload(len, buf)
            }
            0xD8 => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Map8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_map_payload(len, buf)
            }
            0xD9 => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Map16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_map_payload(len, buf)
            }
            0xDA => {
                if buf.remaining() < 4 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Map32 length".to_string(),
                    ));
                }
                let len = buf.get_u32() as usize;
                Self::decode_map_payload(len, buf)
            }
            0xDC => {
                if buf.remaining() < 1 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Struct8 length".to_string(),
                    ));
                }
                let len = buf.get_u8() as usize;
                Self::decode_structure_payload(len, buf)
            }
            0xDD => {
                if buf.remaining() < 2 {
                    return Err(NetError::ProtocolError(
                        "Incomplete Struct16 length".to_string(),
                    ));
                }
                let len = buf.get_u16() as usize;
                Self::decode_structure_payload(len, buf)
            }
            unknown => Err(NetError::ProtocolError(format!(
                "Unknown PackStream marker byte 0x{:02X}",
                unknown
            ))),
        }
    }

    // --- Encoders ---

    fn encode_integer(val: i64, buf: &mut BytesMut) {
        if (-16..=127).contains(&val) {
            buf.put_u8((val as i8) as u8);
        } else if (i8::MIN as i64..=i8::MAX as i64).contains(&val) {
            buf.put_u8(0xC8);
            buf.put_i8(val as i8);
        } else if (i16::MIN as i64..=i16::MAX as i64).contains(&val) {
            buf.put_u8(0xC9);
            buf.put_i16(val as i16);
        } else if (i32::MIN as i64..=i32::MAX as i64).contains(&val) {
            buf.put_u8(0xCA);
            buf.put_i32(val as i32);
        } else {
            buf.put_u8(0xCB);
            buf.put_i64(val);
        }
    }

    fn encode_bytes(bytes: &[u8], buf: &mut BytesMut) {
        let len = bytes.len();
        if len <= u8::MAX as usize {
            buf.put_u8(0xCC);
            buf.put_u8(len as u8);
        } else if len <= u16::MAX as usize {
            buf.put_u8(0xCD);
            buf.put_u16(len as u16);
        } else {
            buf.put_u8(0xCE);
            buf.put_u32(len as u32);
        }
        buf.put_slice(bytes);
    }

    fn encode_string(s: &str, buf: &mut BytesMut) {
        let len = s.len();
        if len <= 15 {
            buf.put_u8(0x80 | (len as u8));
        } else if len <= u8::MAX as usize {
            buf.put_u8(0xD0);
            buf.put_u8(len as u8);
        } else if len <= u16::MAX as usize {
            buf.put_u8(0xD1);
            buf.put_u16(len as u16);
        } else {
            buf.put_u8(0xD2);
            buf.put_u32(len as u32);
        }
        buf.put_slice(s.as_bytes());
    }

    fn encode_list(list: &[BoltValue], buf: &mut BytesMut) {
        let len = list.len();
        if len <= 15 {
            buf.put_u8(0x90 | (len as u8));
        } else if len <= u8::MAX as usize {
            buf.put_u8(0xD4);
            buf.put_u8(len as u8);
        } else if len <= u16::MAX as usize {
            buf.put_u8(0xD5);
            buf.put_u16(len as u16);
        } else {
            buf.put_u8(0xD6);
            buf.put_u32(len as u32);
        }
        for item in list {
            Self::encode(item, buf);
        }
    }

    fn encode_map(map: &HashMap<String, BoltValue>, buf: &mut BytesMut) {
        let len = map.len();
        if len <= 15 {
            buf.put_u8(0xA0 | (len as u8));
        } else if len <= u8::MAX as usize {
            buf.put_u8(0xD8);
            buf.put_u8(len as u8);
        } else if len <= u16::MAX as usize {
            buf.put_u8(0xD9);
            buf.put_u16(len as u16);
        } else {
            buf.put_u8(0xDA);
            buf.put_u32(len as u32);
        }
        for (k, v) in map {
            Self::encode_string(k, buf);
            Self::encode(v, buf);
        }
    }

    fn encode_structure(tag: u8, fields: &[BoltValue], buf: &mut BytesMut) {
        let len = fields.len();
        if len <= 15 {
            buf.put_u8(0xB0 | (len as u8));
        } else if len <= u8::MAX as usize {
            buf.put_u8(0xDC);
            buf.put_u8(len as u8);
        } else {
            buf.put_u8(0xDD);
            buf.put_u16(len as u16);
        }
        buf.put_u8(tag);
        for field in fields {
            Self::encode(field, buf);
        }
    }

    fn encode_node(node: &BoltNode, buf: &mut BytesMut) {
        let mut fields = vec![
            BoltValue::Integer(node.id),
            BoltValue::List(node.labels.iter().cloned().map(BoltValue::String).collect()),
            BoltValue::Map(node.properties.clone()),
        ];
        if let Some(ref elem_id) = node.element_id {
            fields.push(BoltValue::String(elem_id.clone()));
        }
        Self::encode_structure(0x4E, &fields, buf);
    }

    fn encode_relationship(rel: &BoltRelationship, buf: &mut BytesMut) {
        let mut fields = vec![
            BoltValue::Integer(rel.id),
            BoltValue::Integer(rel.start_node_id),
            BoltValue::Integer(rel.end_node_id),
            BoltValue::String(rel.rel_type.clone()),
            BoltValue::Map(rel.properties.clone()),
        ];
        if let Some(ref elem_id) = rel.element_id {
            fields.push(BoltValue::String(elem_id.clone()));
            fields.push(BoltValue::String(
                rel.start_element_id.clone().unwrap_or_default(),
            ));
            fields.push(BoltValue::String(
                rel.end_element_id.clone().unwrap_or_default(),
            ));
        }
        Self::encode_structure(0x52, &fields, buf);
    }

    fn encode_path(path: &BoltPath, buf: &mut BytesMut) {
        let nodes_val = BoltValue::List(path.nodes.iter().cloned().map(BoltValue::Node).collect());
        let rels_val = BoltValue::List(
            path.relationships
                .iter()
                .cloned()
                .map(|r| BoltValue::Structure {
                    tag: 0x72,
                    fields: {
                        let mut f = vec![
                            BoltValue::Integer(r.id),
                            BoltValue::String(r.rel_type),
                            BoltValue::Map(r.properties),
                        ];
                        if let Some(eid) = r.element_id {
                            f.push(BoltValue::String(eid));
                        }
                        f
                    },
                })
                .collect(),
        );
        let seq_val = BoltValue::List(
            path.sequence
                .iter()
                .map(|&i| BoltValue::Integer(i))
                .collect(),
        );
        Self::encode_structure(0x50, &[nodes_val, rels_val, seq_val], buf);
    }

    // --- Decoders ---

    fn decode_string_payload(len: usize, buf: &mut Bytes) -> Result<BoltValue> {
        if buf.remaining() < len {
            return Err(NetError::ProtocolError(format!(
                "Unexpected EOF reading String of length {}",
                len
            )));
        }
        let bytes = buf.split_to(len);
        let s = std::str::from_utf8(&bytes)
            .map_err(|e| NetError::ProtocolError(format!("Invalid UTF-8 string: {}", e)))?
            .to_string();
        Ok(BoltValue::String(s))
    }

    fn decode_bytes_payload(len: usize, buf: &mut Bytes) -> Result<BoltValue> {
        if buf.remaining() < len {
            return Err(NetError::ProtocolError(format!(
                "Unexpected EOF reading byte buffer of length {}",
                len
            )));
        }
        let bytes = buf.split_to(len).to_vec();
        Ok(BoltValue::Bytes(bytes))
    }

    fn decode_list_payload(len: usize, buf: &mut Bytes) -> Result<BoltValue> {
        if len > buf.remaining() {
            return Err(NetError::ProtocolError(format!(
                "List length {} exceeds available buffer bytes {}",
                len,
                buf.remaining()
            )));
        }
        let mut list = Vec::with_capacity(len.min(1024));
        for _ in 0..len {
            list.push(Self::decode(buf)?);
        }
        Ok(BoltValue::List(list))
    }

    fn decode_map_payload(size: usize, buf: &mut Bytes) -> Result<BoltValue> {
        if size.saturating_mul(2) > buf.remaining() {
            return Err(NetError::ProtocolError(format!(
                "Map size {} exceeds available buffer bytes {}",
                size,
                buf.remaining()
            )));
        }
        let mut map = HashMap::with_capacity(size.min(1024));
        for _ in 0..size {
            let key = match Self::decode(buf)? {
                BoltValue::String(s) => s,
                other => {
                    return Err(NetError::ProtocolError(format!(
                        "Map key must be a String, got: {:?}",
                        other
                    )));
                }
            };
            let value = Self::decode(buf)?;
            map.insert(key, value);
        }
        Ok(BoltValue::Map(map))
    }

    fn decode_structure_payload(num_fields: usize, buf: &mut Bytes) -> Result<BoltValue> {
        if !buf.has_remaining() {
            return Err(NetError::ProtocolError(
                "Unexpected EOF reading structure signature tag".to_string(),
            ));
        }
        let tag = buf.get_u8();
        if num_fields > buf.remaining() {
            return Err(NetError::ProtocolError(format!(
                "Structure field count {} exceeds available buffer bytes {}",
                num_fields,
                buf.remaining()
            )));
        }
        let mut fields = Vec::with_capacity(num_fields.min(1024));
        for _ in 0..num_fields {
            fields.push(Self::decode(buf)?);
        }

        // Try decoding into high-level graph structures if tag matches
        match tag {
            0x4E => Self::decode_node_structure(fields),
            0x52 => Self::decode_relationship_structure(fields),
            0x50 => Self::decode_path_structure(fields),
            _ => Ok(BoltValue::Structure { tag, fields }),
        }
    }

    fn decode_node_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 3 {
            return Ok(BoltValue::Structure { tag: 0x4E, fields });
        }
        let id = match fields[0] {
            BoltValue::Integer(i) => i,
            _ => return Ok(BoltValue::Structure { tag: 0x4E, fields }),
        };
        let labels = match &fields[1] {
            BoltValue::List(l) => l
                .iter()
                .filter_map(|v| match v {
                    BoltValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let properties = match &fields[2] {
            BoltValue::Map(m) => m.clone(),
            _ => HashMap::new(),
        };
        let element_id = if fields.len() >= 4 {
            match &fields[3] {
                BoltValue::String(s) => Some(s.clone()),
                _ => None,
            }
        } else {
            None
        };

        Ok(BoltValue::Node(BoltNode {
            id,
            labels,
            properties,
            element_id,
        }))
    }

    fn decode_relationship_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 5 {
            return Ok(BoltValue::Structure { tag: 0x52, fields });
        }
        let id = match fields[0] {
            BoltValue::Integer(i) => i,
            _ => return Ok(BoltValue::Structure { tag: 0x52, fields }),
        };
        let start_node_id = match fields[1] {
            BoltValue::Integer(i) => i,
            _ => 0,
        };
        let end_node_id = match fields[2] {
            BoltValue::Integer(i) => i,
            _ => 0,
        };
        let rel_type = match &fields[3] {
            BoltValue::String(s) => s.clone(),
            _ => String::new(),
        };
        let properties = match &fields[4] {
            BoltValue::Map(m) => m.clone(),
            _ => HashMap::new(),
        };
        let (element_id, start_element_id, end_element_id) = if fields.len() >= 8 {
            (
                fields[5].as_str().map(|s| s.to_string()),
                fields[6].as_str().map(|s| s.to_string()),
                fields[7].as_str().map(|s| s.to_string()),
            )
        } else {
            (None, None, None)
        };

        Ok(BoltValue::Relationship(BoltRelationship {
            id,
            start_node_id,
            end_node_id,
            rel_type,
            properties,
            element_id,
            start_element_id,
            end_element_id,
        }))
    }

    fn decode_path_structure(fields: Vec<BoltValue>) -> Result<BoltValue> {
        if fields.len() < 3 {
            return Ok(BoltValue::Structure { tag: 0x50, fields });
        }
        let nodes = match &fields[0] {
            BoltValue::List(l) => l
                .iter()
                .filter_map(|v| match v {
                    BoltValue::Node(n) => Some(n.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let relationships = match &fields[1] {
            BoltValue::List(l) => l
                .iter()
                .filter_map(|v| match v {
                    BoltValue::Structure { tag: 0x72, fields } => {
                        if fields.len() >= 3 {
                            let id = match fields[0] {
                                BoltValue::Integer(i) => i,
                                _ => 0,
                            };
                            let rel_type = match &fields[1] {
                                BoltValue::String(s) => s.clone(),
                                _ => String::new(),
                            };
                            let properties = match &fields[2] {
                                BoltValue::Map(m) => m.clone(),
                                _ => HashMap::new(),
                            };
                            let element_id = fields
                                .get(3)
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            Some(BoltUnboundRelationship {
                                id,
                                rel_type,
                                properties,
                                element_id,
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let sequence = match &fields[2] {
            BoltValue::List(l) => l
                .iter()
                .filter_map(|v| match v {
                    BoltValue::Integer(i) => Some(*i),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };

        Ok(BoltValue::Path(BoltPath {
            nodes,
            relationships,
            sequence,
        }))
    }
}

impl BoltValue {
    /// Returns the string slice if this is a `BoltValue::String`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns the integer if this is a `BoltValue::Integer`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Returns the boolean if this is a `BoltValue::Boolean`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the map if this is a `BoltValue::Map`.
    pub fn as_map(&self) -> Option<&HashMap<String, BoltValue>> {
        match self {
            Self::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the list if this is a `BoltValue::List`.
    pub fn as_list(&self) -> Option<&[BoltValue]> {
        match self {
            Self::List(l) => Some(l.as_slice()),
            _ => None,
        }
    }
}
