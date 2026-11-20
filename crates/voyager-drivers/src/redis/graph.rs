//! FalkorDB Graph Result parser, entity conversion, and Apache Arrow RecordBatch builder.
//!
//! Parses tabular execution results from FalkorDB's `GRAPH.QUERY` and `GRAPH.RO_QUERY`
//! commands (in both RESP2 and RESP3 formats), converting Graph entities (nodes,
//! relationships, paths) into unified [`BoltNode`], [`BoltRelationship`], and columnar
//! Apache Arrow [`RecordBatch`] structures.

use arrow_array::builder::{BooleanBuilder, Float64Builder, Int64Builder, StringBuilder};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use std::collections::HashMap;
use std::sync::Arc;

use crate::bolt::{BoltNode, BoltRelationship, BoltValue};
use crate::engine::{QueryResult, QuerySummary};
use crate::error::{NetError, Result};
use crate::redis::resp::RespValue;

/// Parsed execution statistics from FalkorDB query execution.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FalkorStatistics {
    /// Number of graph nodes created.
    pub nodes_created: u64,
    /// Number of graph nodes deleted.
    pub nodes_deleted: u64,
    /// Number of graph relationships created.
    pub relationships_created: u64,
    /// Number of graph relationships deleted.
    pub relationships_deleted: u64,
    /// Number of properties set or updated on nodes/relationships.
    pub properties_set: u64,
    /// Number of node labels added.
    pub labels_added: u64,
    /// Number of node labels removed.
    pub labels_removed: u64,
    /// Number of schema indexes created.
    pub indices_created: u64,
    /// Number of schema indexes deleted.
    pub indices_deleted: u64,
    /// Whether this execution was cached by FalkorDB plan cache.
    pub cached_execution: bool,
    /// Internal execution time in milliseconds as reported by FalkorDB.
    pub internal_execution_time_ms: f64,
    /// Raw unparsed statistics strings returned by server.
    pub raw_stats: Vec<String>,
}

impl FalkorStatistics {
    /// Parses execution statistics from a RESP array of strings.
    pub fn parse(stats_array: &[RespValue]) -> Self {
        let mut stats = Self::default();

        for item in stats_array {
            let line = item.to_string_lossy();
            stats.raw_stats.push(line.clone());

            if let Some((key, val)) = line.split_once(':') {
                let key_trimmed = key.trim();
                let val_trimmed = val.trim();

                match key_trimmed {
                    "Nodes created" => {
                        stats.nodes_created = val_trimmed.parse().unwrap_or(0);
                    }
                    "Nodes deleted" => {
                        stats.nodes_deleted = val_trimmed.parse().unwrap_or(0);
                    }
                    "Relationships created" => {
                        stats.relationships_created = val_trimmed.parse().unwrap_or(0);
                    }
                    "Relationships deleted" => {
                        stats.relationships_deleted = val_trimmed.parse().unwrap_or(0);
                    }
                    "Properties set" => {
                        stats.properties_set = val_trimmed.parse().unwrap_or(0);
                    }
                    "Labels added" => {
                        stats.labels_added = val_trimmed.parse().unwrap_or(0);
                    }
                    "Labels removed" => {
                        stats.labels_removed = val_trimmed.parse().unwrap_or(0);
                    }
                    "Indices created" | "Indexes added" => {
                        stats.indices_created = val_trimmed.parse().unwrap_or(0);
                    }
                    "Indices deleted" | "Indexes removed" => {
                        stats.indices_deleted = val_trimmed.parse().unwrap_or(0);
                    }
                    "Cached execution" => {
                        stats.cached_execution = val_trimmed == "1" || val_trimmed == "true";
                    }
                    "Query internal execution time" => {
                        // Format e.g.: "24.918253 milliseconds"
                        if let Some(num_part) = val_trimmed.split_whitespace().next() {
                            stats.internal_execution_time_ms = num_part.parse().unwrap_or(0.0);
                        }
                    }
                    _ => {}
                }
            }
        }

        stats
    }

    /// Converts into the unified Voyager [`QuerySummary`].
    pub fn to_query_summary(&self) -> QuerySummary {
        QuerySummary {
            nodes_created: self.nodes_created,
            nodes_deleted: self.nodes_deleted,
            relationships_created: self.relationships_created,
            relationships_deleted: self.relationships_deleted,
            properties_set: self.properties_set,
            labels_added: self.labels_added,
            labels_removed: self.labels_removed,
            indexes_added: self.indices_created,
            constraints_added: 0,
            execution_time_ms: self.internal_execution_time_ms.round() as u64,
        }
    }
}

/// Represents a graph Node returned by FalkorDB.
#[derive(Debug, Clone, PartialEq)]
pub struct FalkorNode {
    /// Internal entity integer identifier.
    pub id: i64,
    /// Set of string labels attached to this node.
    pub labels: Vec<String>,
    /// Node properties map.
    pub properties: HashMap<String, RespValue>,
}

impl FalkorNode {
    /// Parses a `FalkorNode` from a RESP array or map representation.
    ///
    /// FalkorDB encodes nodes in RESP2 as:
    /// `[["id", <id>], ["labels", [<label1>, ...]], ["properties", [[k1, v1], [k2, v2]]]]`
    /// or in RESP3 as a map `%{"id": ..., "labels": ..., "properties": ...}`.
    pub fn parse(val: &RespValue) -> Result<Self> {
        let mut id = 0i64;
        let mut labels = Vec::new();
        let mut properties = HashMap::new();

        if let Some(pairs) = val.as_map() {
            for (k, v) in pairs {
                match k.as_str().unwrap_or("") {
                    "id" => id = v.as_i64().unwrap_or(0),
                    "labels" => {
                        if let Some(arr) = v.as_array() {
                            labels = arr.iter().map(|item| item.to_string_lossy()).collect();
                        }
                    }
                    "properties" => {
                        properties = v.to_hashmap();
                    }
                    _ => {}
                }
            }
        } else if let Some(items) = val.as_array() {
            for item in items {
                if let Some(pair) = item.as_array()
                    && pair.len() == 2
                {
                    let key_str = pair[0].to_string_lossy();
                    match key_str.as_str() {
                        "id" => id = pair[1].as_i64().unwrap_or(0),
                        "labels" => {
                            if let Some(arr) = pair[1].as_array() {
                                labels = arr.iter().map(|lbl| lbl.to_string_lossy()).collect();
                            }
                        }
                        "properties" => {
                            properties = pair[1].to_hashmap();
                        }
                        _ => {}
                    }
                }
            }
        } else {
            return Err(NetError::ProtocolError(format!(
                "Expected FalkorDB Node structure, got {:?}",
                val
            )));
        }

        Ok(Self {
            id,
            labels,
            properties,
        })
    }

    /// Converts this FalkorDB node into the unified [`BoltNode`].
    pub fn to_bolt_node(&self) -> BoltNode {
        let mut bolt_props = HashMap::new();
        for (k, v) in &self.properties {
            bolt_props.insert(k.clone(), resp_to_bolt_value(v));
        }

        BoltNode {
            id: self.id,
            labels: self.labels.clone(),
            properties: bolt_props,
            element_id: Some(self.id.to_string()),
        }
    }
}

/// Represents a graph Relationship returned by FalkorDB.
#[derive(Debug, Clone, PartialEq)]
pub struct FalkorRelationship {
    /// Internal entity integer identifier.
    pub id: i64,
    /// Relationship type label (e.g. `"KNOWS"`).
    pub rel_type: String,
    /// Starting node integer ID.
    pub src_node: i64,
    /// Destination node integer ID.
    pub dest_node: i64,
    /// Relationship properties map.
    pub properties: HashMap<String, RespValue>,
}

impl FalkorRelationship {
    /// Parses a `FalkorRelationship` from a RESP array or map representation.
    ///
    /// FalkorDB encodes edges in RESP2 as:
    /// `[["id", <id>], ["type", <type>], ["src_node", <src>], ["dest_node", <dest>], ["properties", [...]]]`
    pub fn parse(val: &RespValue) -> Result<Self> {
        let mut id = 0i64;
        let mut rel_type = String::new();
        let mut src_node = 0i64;
        let mut dest_node = 0i64;
        let mut properties = HashMap::new();

        if let Some(pairs) = val.as_map() {
            for (k, v) in pairs {
                match k.as_str().unwrap_or("") {
                    "id" => id = v.as_i64().unwrap_or(0),
                    "type" => rel_type = v.to_string_lossy(),
                    "src_node" => src_node = v.as_i64().unwrap_or(0),
                    "dest_node" => dest_node = v.as_i64().unwrap_or(0),
                    "properties" => properties = v.to_hashmap(),
                    _ => {}
                }
            }
        } else if let Some(items) = val.as_array() {
            for item in items {
                if let Some(pair) = item.as_array()
                    && pair.len() == 2
                {
                    let key_str = pair[0].to_string_lossy();
                    match key_str.as_str() {
                        "id" => id = pair[1].as_i64().unwrap_or(0),
                        "type" => rel_type = pair[1].to_string_lossy(),
                        "src_node" => src_node = pair[1].as_i64().unwrap_or(0),
                        "dest_node" => dest_node = pair[1].as_i64().unwrap_or(0),
                        "properties" => properties = pair[1].to_hashmap(),
                        _ => {}
                    }
                }
            }
        } else {
            return Err(NetError::ProtocolError(format!(
                "Expected FalkorDB Relationship structure, got {:?}",
                val
            )));
        }

        Ok(Self {
            id,
            rel_type,
            src_node,
            dest_node,
            properties,
        })
    }

    /// Converts this FalkorDB relationship into the unified [`BoltRelationship`].
    pub fn to_bolt_relationship(&self) -> BoltRelationship {
        let mut bolt_props = HashMap::new();
        for (k, v) in &self.properties {
            bolt_props.insert(k.clone(), resp_to_bolt_value(v));
        }

        BoltRelationship {
            id: self.id,
            start_node_id: self.src_node,
            end_node_id: self.dest_node,
            rel_type: self.rel_type.clone(),
            properties: bolt_props,
            element_id: Some(self.id.to_string()),
            start_element_id: Some(self.src_node.to_string()),
            end_element_id: Some(self.dest_node.to_string()),
        }
    }
}

/// Helper converting a [`RespValue`] to a [`BoltValue`].
pub fn resp_to_bolt_value(val: &RespValue) -> BoltValue {
    match val {
        RespValue::Null => BoltValue::Null,
        RespValue::BulkString(None) => BoltValue::Null,
        RespValue::Array(None) => BoltValue::Null,
        RespValue::Boolean(b) => BoltValue::Boolean(*b),
        RespValue::Integer(i) => BoltValue::Integer(*i),
        RespValue::Double(f) => BoltValue::Float(*f),
        RespValue::SimpleString(s) => BoltValue::String(s.clone()),
        RespValue::BigNumber(s) => BoltValue::String(s.clone()),
        RespValue::BulkString(Some(b)) => {
            BoltValue::String(String::from_utf8_lossy(b).into_owned())
        }
        RespValue::Array(Some(arr)) => {
            BoltValue::List(arr.iter().map(resp_to_bolt_value).collect())
        }
        RespValue::Set(arr) | RespValue::Push(arr) => {
            BoltValue::List(arr.iter().map(resp_to_bolt_value).collect())
        }
        RespValue::Map(pairs) => {
            let mut map = HashMap::new();
            for (k, v) in pairs {
                map.insert(k.to_string_lossy(), resp_to_bolt_value(v));
            }
            BoltValue::Map(map)
        }
        other => BoltValue::String(other.to_string_lossy()),
    }
}

/// Complete tabular execution result from a FalkorDB `GRAPH.QUERY` or `GRAPH.RO_QUERY`.
#[derive(Debug, Clone, PartialEq)]
pub struct FalkorQueryResult {
    /// Column names from the `RETURN` projection.
    pub columns: Vec<String>,
    /// Tabular data rows, where each row contains one `RespValue` per column.
    pub rows: Vec<Vec<RespValue>>,
    /// Server-side execution statistics and mutation counters.
    pub statistics: FalkorStatistics,
}

impl FalkorQueryResult {
    /// Parses a complete FalkorDB query reply from the top-level RESP response.
    ///
    /// Handles:
    /// - 3-element arrays: `[header_columns, data_rows, statistics]`
    /// - 1-element arrays: `[statistics]` (queries with no `RETURN` clause)
    pub fn parse(resp: RespValue) -> Result<Self> {
        let items = match resp {
            RespValue::Array(Some(arr)) => arr,
            RespValue::Error(err) => {
                return Err(NetError::ExecutionError(format!(
                    "FalkorDB query error: {}",
                    err
                )));
            }
            other => {
                return Err(NetError::ProtocolError(format!(
                    "Expected RESP array from FalkorDB query, got {:?}",
                    other
                )));
            }
        };

        if items.len() == 3 {
            // [0] Headers
            let columns = match &items[0] {
                RespValue::Array(Some(cols)) => cols.iter().map(|c| c.to_string_lossy()).collect(),
                _ => Vec::new(),
            };

            // [1] Rows
            let mut rows = Vec::new();
            if let RespValue::Array(Some(raw_rows)) = &items[1] {
                for raw_row in raw_rows {
                    if let RespValue::Array(Some(cells)) = raw_row {
                        rows.push(cells.clone());
                    }
                }
            }

            // [2] Statistics
            let statistics = match &items[2] {
                RespValue::Array(Some(stats_arr)) => FalkorStatistics::parse(stats_arr),
                _ => FalkorStatistics::default(),
            };

            Ok(Self {
                columns,
                rows,
                statistics,
            })
        } else if items.len() == 1 {
            // Non-returning query with statistics only
            let statistics = match &items[0] {
                RespValue::Array(Some(stats_arr)) => FalkorStatistics::parse(stats_arr),
                _ => FalkorStatistics::default(),
            };

            Ok(Self {
                columns: Vec::new(),
                rows: Vec::new(),
                statistics,
            })
        } else {
            Err(NetError::ProtocolError(format!(
                "Unexpected FalkorDB query response array length: {} (expected 1 or 3)",
                items.len()
            )))
        }
    }

    /// Converts tabular result rows into an Apache Arrow [`RecordBatch`].
    pub fn to_arrow_record_batch(&self) -> Result<RecordBatch> {
        let num_cols = self.columns.len();
        let num_rows = self.rows.len();

        if num_cols == 0 || num_rows == 0 {
            // Build an empty RecordBatch with matching schema
            let fields: Vec<Field> = self
                .columns
                .iter()
                .map(|name| Field::new(name, DataType::Utf8, true))
                .collect();
            let schema = Arc::new(Schema::new(fields));
            let columns: Vec<ArrayRef> = self
                .columns
                .iter()
                .map(|_| Arc::new(StringBuilder::new().finish()) as ArrayRef)
                .collect();
            return RecordBatch::try_new(schema, columns).map_err(NetError::ArrowError);
        }

        // Dynamic type inference per column
        let mut inferred_types = vec![DataType::Utf8; num_cols];

        for (col_idx, inferred_type) in inferred_types.iter_mut().enumerate() {
            let mut has_non_null = false;
            let mut can_be_int = true;
            let mut can_be_float = true;
            let mut can_be_bool = true;

            for row in &self.rows {
                if let Some(cell) = row.get(col_idx)
                    && !cell.is_null()
                {
                    has_non_null = true;

                    match cell {
                        RespValue::Integer(_) => {
                            can_be_bool = false;
                        }
                        RespValue::Double(_) => {
                            can_be_int = false;
                            can_be_bool = false;
                        }
                        RespValue::Boolean(_) => {
                            can_be_int = false;
                            can_be_float = false;
                        }
                        RespValue::SimpleString(s) => {
                            if can_be_bool && cell.as_bool().is_none() {
                                can_be_bool = false;
                            }
                            if can_be_int && s.parse::<i64>().is_err() {
                                can_be_int = false;
                            }
                            if can_be_float && s.parse::<f64>().is_err() {
                                can_be_float = false;
                            }
                        }
                        RespValue::BulkString(Some(b)) => {
                            if let Ok(s) = std::str::from_utf8(b) {
                                if can_be_bool && cell.as_bool().is_none() {
                                    can_be_bool = false;
                                }
                                if can_be_int && s.parse::<i64>().is_err() {
                                    can_be_int = false;
                                }
                                if can_be_float && s.parse::<f64>().is_err() {
                                    can_be_float = false;
                                }
                            } else {
                                can_be_int = false;
                                can_be_float = false;
                                can_be_bool = false;
                            }
                        }
                        _ => {
                            // Nested structures, nodes, edges promote to Utf8 / JSON
                            can_be_int = false;
                            can_be_float = false;
                            can_be_bool = false;
                        }
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

        // Build Arrow columnar arrays
        let mut fields = Vec::with_capacity(num_cols);
        let mut columns: Vec<ArrayRef> = Vec::with_capacity(num_cols);

        for (col_idx, name) in self.columns.iter().enumerate() {
            let data_type = &inferred_types[col_idx];
            fields.push(Field::new(name, data_type.clone(), true));

            match data_type {
                DataType::Int64 => {
                    let mut builder = Int64Builder::with_capacity(num_rows);
                    for row in &self.rows {
                        match row.get(col_idx) {
                            Some(cell) if !cell.is_null() => {
                                if let Some(i) = cell.as_i64() {
                                    builder.append_value(i);
                                } else {
                                    builder.append_null();
                                }
                            }
                            _ => builder.append_null(),
                        }
                    }
                    columns.push(Arc::new(builder.finish()));
                }
                DataType::Float64 => {
                    let mut builder = Float64Builder::with_capacity(num_rows);
                    for row in &self.rows {
                        match row.get(col_idx) {
                            Some(cell) if !cell.is_null() => {
                                if let Some(f) = cell.as_f64() {
                                    builder.append_value(f);
                                } else {
                                    builder.append_null();
                                }
                            }
                            _ => builder.append_null(),
                        }
                    }
                    columns.push(Arc::new(builder.finish()));
                }
                DataType::Boolean => {
                    let mut builder = BooleanBuilder::with_capacity(num_rows);
                    for row in &self.rows {
                        match row.get(col_idx) {
                            Some(cell) if !cell.is_null() => {
                                if let Some(b) = cell.as_bool() {
                                    builder.append_value(b);
                                } else {
                                    builder.append_null();
                                }
                            }
                            _ => builder.append_null(),
                        }
                    }
                    columns.push(Arc::new(builder.finish()));
                }
                _ => {
                    // String / JSON fallback
                    let mut builder = StringBuilder::with_capacity(num_rows, num_rows * 32);
                    for row in &self.rows {
                        match row.get(col_idx) {
                            Some(cell) if !cell.is_null() => {
                                builder.append_value(cell.to_string_lossy());
                            }
                            _ => builder.append_null(),
                        }
                    }
                    columns.push(Arc::new(builder.finish()));
                }
            }
        }

        let schema = Arc::new(Schema::new(fields));
        RecordBatch::try_new(schema, columns).map_err(NetError::ArrowError)
    }

    /// Alias for [`to_arrow_record_batch`].
    pub fn to_record_batch(&self) -> Result<RecordBatch> {
        self.to_arrow_record_batch()
    }

    /// Converts this tabular query result into the workspace's unified [`QueryResult`].
    pub fn to_query_result(&self) -> Result<QueryResult> {
        let summary = self.statistics.to_query_summary();

        if self.columns.is_empty() {
            return Ok(QueryResult::empty(summary));
        }

        let batch = self.to_arrow_record_batch()?;
        Ok(QueryResult::new(self.columns.clone(), vec![batch], summary))
    }
}
