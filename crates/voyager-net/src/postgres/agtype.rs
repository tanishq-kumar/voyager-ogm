//! Apache AGE `agtype` parser, graph entity structures, and Apache Arrow RecordBatch deserialization.

use arrow_array::builder::{BooleanBuilder, Float64Builder, Int64Builder, StringBuilder};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::bolt::{BoltNode, BoltPath, BoltRelationship, BoltValue};
use crate::error::{NetError, Result};

/// Apache AGE Graph Vertex entity (`::vertex`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgeVertex {
    /// Graph internal 64-bit vertex identifier.
    pub id: i64,
    /// Vertex label / type tag.
    pub label: String,
    /// Key-value vertex property map.
    pub properties: HashMap<String, serde_json::Value>,
}

impl AgeVertex {
    /// Converts this `AgeVertex` into a Bolt-compatible [`BoltNode`].
    pub fn to_bolt_node(&self) -> BoltNode {
        let mut props = HashMap::new();
        for (k, v) in &self.properties {
            props.insert(k.clone(), from_json_to_bolt_value(v));
        }
        BoltNode {
            id: self.id,
            labels: vec![self.label.clone()],
            properties: props,
            element_id: Some(self.id.to_string()),
        }
    }
}

/// Apache AGE Graph Edge / Relationship entity (`::edge`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgeEdge {
    /// Graph internal 64-bit edge identifier.
    pub id: i64,
    /// Edge relationship label / type name.
    pub label: String,
    /// Start (source) vertex 64-bit identifier.
    pub start_id: i64,
    /// End (target) vertex 64-bit identifier.
    pub end_id: i64,
    /// Key-value edge property map.
    pub properties: HashMap<String, serde_json::Value>,
}

impl AgeEdge {
    /// Converts this `AgeEdge` into a Bolt-compatible [`BoltRelationship`].
    pub fn to_bolt_relationship(&self) -> BoltRelationship {
        let mut props = HashMap::new();
        for (k, v) in &self.properties {
            props.insert(k.clone(), from_json_to_bolt_value(v));
        }
        BoltRelationship {
            id: self.id,
            start_node_id: self.start_id,
            end_node_id: self.end_id,
            rel_type: self.label.clone(),
            properties: props,
            element_id: Some(self.id.to_string()),
            start_element_id: Some(self.start_id.to_string()),
            end_element_id: Some(self.end_id.to_string()),
        }
    }
}

/// Apache AGE Graph Path entity (`::path`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgePath {
    /// Ordered sequence of vertices in the path.
    pub vertices: Vec<AgeVertex>,
    /// Ordered sequence of edges connecting the vertices in the path.
    pub edges: Vec<AgeEdge>,
}

impl AgePath {
    /// Converts this `AgePath` into a Bolt-compatible [`BoltPath`].
    pub fn to_bolt_path(&self) -> BoltPath {
        let nodes: Vec<BoltNode> = self.vertices.iter().map(|v| v.to_bolt_node()).collect();
        let rels: Vec<crate::bolt::BoltUnboundRelationship> = self
            .edges
            .iter()
            .map(|e| {
                let mut props = HashMap::new();
                for (k, v) in &e.properties {
                    props.insert(k.clone(), from_json_to_bolt_value(v));
                }
                crate::bolt::BoltUnboundRelationship {
                    id: e.id,
                    rel_type: e.label.clone(),
                    properties: props,
                    element_id: Some(e.id.to_string()),
                }
            })
            .collect();

        let sequence: Vec<i64> = (1..=(self.edges.len() as i64)).collect();

        BoltPath {
            nodes,
            relationships: rels,
            sequence,
        }
    }
}

fn from_json_to_bolt_value(val: &serde_json::Value) -> BoltValue {
    match val {
        serde_json::Value::Null => BoltValue::Null,
        serde_json::Value::Bool(b) => BoltValue::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                BoltValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                BoltValue::Float(f)
            } else {
                BoltValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => BoltValue::String(s.clone()),
        serde_json::Value::Array(arr) => {
            BoltValue::List(arr.iter().map(from_json_to_bolt_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), from_json_to_bolt_value(v));
            }
            BoltValue::Map(map)
        }
    }
}

/// Parsed Apache AGE dynamic agtype value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgeValue {
    /// Null value.
    Null,
    /// Boolean scalar.
    Boolean(bool),
    /// 64-bit integer scalar.
    Integer(i64),
    /// 64-bit float scalar.
    Float(f64),
    /// String scalar.
    String(String),
    /// Graph vertex entity.
    Vertex(AgeVertex),
    /// Graph edge entity.
    Edge(AgeEdge),
    /// Graph path entity.
    Path(AgePath),
    /// Homogeneous or heterogeneous array list.
    List(Vec<AgeValue>),
    /// Dynamic property map.
    Map(HashMap<String, AgeValue>),
}

/// Strips Apache AGE type annotations (such as `::vertex`, `::edge`, `::path`, `::numeric`) from a raw string while preserving content inside quotes.
pub fn clean_agtype_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_quote = false;
    let mut escaped = false;
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if in_quote {
            out.push(b as char);
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_quote = false;
            }
            i += 1;
        } else if b == b'"' {
            in_quote = true;
            out.push('"');
            i += 1;
        } else if s[i..].starts_with("::vertex") {
            i += "::vertex".len();
        } else if s[i..].starts_with("::edge") {
            i += "::edge".len();
        } else if s[i..].starts_with("::path") {
            i += "::path".len();
        } else if s[i..].starts_with("::numeric") {
            i += "::numeric".len();
        } else {
            out.push(b as char);
            i += 1;
        }
    }
    out.trim().to_string()
}

/// Parses an Apache AGE `agtype` text string into an [`AgeValue`].
pub fn parse_agtype(raw: &str) -> Result<AgeValue> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return Ok(AgeValue::Null);
    }

    let is_vertex = trimmed.ends_with("::vertex");
    let is_edge = trimmed.ends_with("::edge");
    let is_path = trimmed.ends_with("::path");

    let cleaned = clean_agtype_string(trimmed);

    if is_vertex {
        let vertex: AgeVertex = serde_json::from_str(&cleaned).map_err(|e| {
            NetError::ProtocolError(format!("Failed to parse AGE vertex: {}: {}", e, trimmed))
        })?;
        return Ok(AgeValue::Vertex(vertex));
    }

    if is_edge {
        let edge: AgeEdge = serde_json::from_str(&cleaned).map_err(|e| {
            NetError::ProtocolError(format!("Failed to parse AGE edge: {}: {}", e, trimmed))
        })?;
        return Ok(AgeValue::Edge(edge));
    }

    if is_path {
        let raw_val: serde_json::Value = serde_json::from_str(&cleaned).map_err(|e| {
            NetError::ProtocolError(format!("Failed to parse AGE path: {}: {}", e, trimmed))
        })?;

        if let serde_json::Value::Array(arr) = raw_val {
            let mut vertices = Vec::new();
            let mut edges = Vec::new();
            for (idx, item) in arr.into_iter().enumerate() {
                if idx % 2 == 0 {
                    let v: AgeVertex = serde_json::from_value(item).map_err(|e| {
                        NetError::ProtocolError(format!("Invalid vertex in path at {}: {}", idx, e))
                    })?;
                    vertices.push(v);
                } else {
                    let e: AgeEdge = serde_json::from_value(item).map_err(|e| {
                        NetError::ProtocolError(format!("Invalid edge in path at {}: {}", idx, e))
                    })?;
                    edges.push(e);
                }
            }
            return Ok(AgeValue::Path(AgePath { vertices, edges }));
        }
    }

    // Try parsing as standard JSON
    match serde_json::from_str::<serde_json::Value>(&cleaned) {
        Ok(serde_json::Value::Null) => Ok(AgeValue::Null),
        Ok(serde_json::Value::Bool(b)) => Ok(AgeValue::Boolean(b)),
        Ok(serde_json::Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Ok(AgeValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(AgeValue::Float(f))
            } else {
                Ok(AgeValue::String(n.to_string()))
            }
        }
        Ok(serde_json::Value::String(s)) => Ok(AgeValue::String(s)),
        Ok(serde_json::Value::Object(obj)) => {
            // Check if object has vertex or edge fields
            if obj.contains_key("id") && obj.contains_key("label") && obj.contains_key("properties")
            {
                if obj.contains_key("start_id") && obj.contains_key("end_id") {
                    if let Ok(edge) =
                        serde_json::from_value::<AgeEdge>(serde_json::Value::Object(obj.clone()))
                    {
                        return Ok(AgeValue::Edge(edge));
                    }
                } else if let Ok(vertex) =
                    serde_json::from_value::<AgeVertex>(serde_json::Value::Object(obj.clone()))
                {
                    return Ok(AgeValue::Vertex(vertex));
                }
            }
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k, parse_json_value(v));
            }
            Ok(AgeValue::Map(map))
        }
        Ok(serde_json::Value::Array(arr)) => {
            let list = arr.into_iter().map(parse_json_value).collect();
            Ok(AgeValue::List(list))
        }
        Err(_) => Ok(AgeValue::String(trimmed.to_string())),
    }
}

fn parse_json_value(val: serde_json::Value) -> AgeValue {
    match val {
        serde_json::Value::Null => AgeValue::Null,
        serde_json::Value::Bool(b) => AgeValue::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                AgeValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                AgeValue::Float(f)
            } else {
                AgeValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => AgeValue::String(s),
        serde_json::Value::Array(arr) => {
            AgeValue::List(arr.into_iter().map(parse_json_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k, parse_json_value(v));
            }
            AgeValue::Map(map)
        }
    }
}

/// Dynamically transforms tabular row values into an Apache Arrow [`RecordBatch`].
pub fn rows_to_record_batch(
    column_names: &[String],
    rows: &[Vec<Option<String>>],
) -> Result<RecordBatch> {
    if column_names.is_empty() {
        return Ok(RecordBatch::new_empty(Arc::new(Schema::empty())));
    }

    let num_rows = rows.len();
    let num_cols = column_names.len();

    let mut inferred_types = vec![DataType::Int64; num_cols];

    // First pass: type inference per column
    for (col_idx, inferred_type) in inferred_types.iter_mut().enumerate() {
        let mut has_non_null = false;
        let mut can_be_int = true;
        let mut can_be_float = true;
        let mut can_be_bool = true;

        for row in rows {
            if let Some(Some(val)) = row.get(col_idx) {
                has_non_null = true;
                let trimmed = clean_agtype_string(val);

                if can_be_bool
                    && trimmed != "t"
                    && trimmed != "f"
                    && trimmed != "true"
                    && trimmed != "false"
                {
                    can_be_bool = false;
                }
                if can_be_int && trimmed.parse::<i64>().is_err() {
                    can_be_int = false;
                }
                if can_be_float && trimmed.parse::<f64>().is_err() {
                    can_be_float = false;
                }
            }
        }

        if !has_non_null {
            *inferred_type = DataType::Utf8;
        } else if can_be_int {
            *inferred_type = DataType::Int64;
        } else if can_be_float {
            *inferred_type = DataType::Float64;
        } else if can_be_bool {
            *inferred_type = DataType::Boolean;
        } else {
            *inferred_type = DataType::Utf8;
        }
    }

    // Build Arrow arrays based on inferred types
    let mut fields = Vec::with_capacity(num_cols);
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(num_cols);

    for (col_idx, name) in column_names.iter().enumerate() {
        let data_type = &inferred_types[col_idx];
        fields.push(Field::new(name, data_type.clone(), true));

        match data_type {
            DataType::Int64 => {
                let mut builder = Int64Builder::with_capacity(num_rows);
                for row in rows {
                    match row.get(col_idx).and_then(|v| v.as_deref()) {
                        Some(val) => {
                            let cleaned = clean_agtype_string(val);
                            if let Ok(i) = cleaned.parse::<i64>() {
                                builder.append_value(i);
                            } else {
                                builder.append_null();
                            }
                        }
                        None => builder.append_null(),
                    }
                }
                columns.push(Arc::new(builder.finish()));
            }
            DataType::Float64 => {
                let mut builder = Float64Builder::with_capacity(num_rows);
                for row in rows {
                    match row.get(col_idx).and_then(|v| v.as_deref()) {
                        Some(val) => {
                            let cleaned = clean_agtype_string(val);
                            if let Ok(f) = cleaned.parse::<f64>() {
                                builder.append_value(f);
                            } else {
                                builder.append_null();
                            }
                        }
                        None => builder.append_null(),
                    }
                }
                columns.push(Arc::new(builder.finish()));
            }
            DataType::Boolean => {
                let mut builder = BooleanBuilder::with_capacity(num_rows);
                for row in rows {
                    match row.get(col_idx).and_then(|v| v.as_deref()) {
                        Some(val) => {
                            let cleaned = clean_agtype_string(val);
                            let b = cleaned == "t" || cleaned == "true";
                            builder.append_value(b);
                        }
                        None => builder.append_null(),
                    }
                }
                columns.push(Arc::new(builder.finish()));
            }
            _ => {
                let mut builder = StringBuilder::with_capacity(num_rows, num_rows * 32);
                for row in rows {
                    match row.get(col_idx).and_then(|v| v.as_deref()) {
                        Some(val) => builder.append_value(val),
                        None => builder.append_null(),
                    }
                }
                columns.push(Arc::new(builder.finish()));
            }
        }
    }

    let schema = Arc::new(Schema::new(fields));
    RecordBatch::try_new(schema, columns).map_err(NetError::ArrowError)
}
