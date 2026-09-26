//! Centralized Schema Registry & Graph Model Metadata for Voyager OGM.
//!
//! Provides the single source of truth for graph entities (nodes and relationships),
//! property descriptors, data types, constraints, and index definitions across
//! multi-dialect DDL emitters and client bindings.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{OnceLock, RwLock};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::ast::LiteralValue;
use crate::error::{Error, Result};

/// Data types supported for graph model properties across dialects.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FieldType {
    /// Textual UTF-8 string.
    String,
    /// 64-bit signed integer.
    Int64,
    /// 64-bit floating point number.
    Float64,
    /// Boolean true/false value.
    Boolean,
    /// ISO 8601 / RFC 3339 timestamp with timezone.
    DateTime,
    /// Calendar date (YYYY-MM-DD).
    Date,
    /// Temporal duration / interval.
    Duration,
    /// Columnar or homogeneous array of nested items.
    List(Box<FieldType>),
    /// Dynamic key-value dictionary / map.
    Map,
    /// Dynamic or unspecified property type.
    Any,
    /// Vendor-specific or user-defined type extension.
    Custom(String),
}

impl FieldType {
    /// Returns the canonical uppercase type name string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::String => "STRING",
            Self::Int64 => "INTEGER",
            Self::Float64 => "FLOAT",
            Self::Boolean => "BOOLEAN",
            Self::DateTime => "DATETIME",
            Self::Date => "DATE",
            Self::Duration => "DURATION",
            Self::List(_) => "LIST",
            Self::Map => "MAP",
            Self::Any => "ANY",
            Self::Custom(s) => s.as_str(),
        }
    }

    /// Parses a case-insensitive type string into a canonical `FieldType`.
    pub fn parse_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "STR" | "STRING" | "TEXT" | "VARCHAR" => Self::String,
            "INT" | "INT64" | "INTEGER" | "BIGINT" => Self::Int64,
            "FLOAT" | "FLOAT64" | "DOUBLE" | "REAL" => Self::Float64,
            "BOOL" | "BOOLEAN" => Self::Boolean,
            "DATETIME" | "TIMESTAMP" => Self::DateTime,
            "DATE" => Self::Date,
            "DURATION" | "INTERVAL" => Self::Duration,
            "LIST" | "ARRAY" => Self::List(Box::new(Self::Any)),
            "MAP" | "DICT" | "OBJECT" => Self::Map,
            "ANY" => Self::Any,
            other => Self::Custom(other.to_string()),
        }
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Supported constraint types on graph schema fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ConstraintType {
    /// Unique constraint enforcing distinct property values across entities.
    Unique,
    /// Property existence / NOT NULL constraint.
    NotNull,
    /// Combined primary key / Node Key constraint (Unique + NotNull).
    NodeKey,
}

impl ConstraintType {
    /// Returns the canonical constraint name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Unique => "UNIQUE",
            Self::NotNull => "NOT_NULL",
            Self::NodeKey => "NODE_KEY",
        }
    }

    /// Parses a case-insensitive constraint string.
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "UNIQUE" => Some(Self::Unique),
            "NOT_NULL" | "NOT NULL" | "MANDATORY" => Some(Self::NotNull),
            "NODE_KEY" | "NODE KEY" | "KEY" | "PRIMARY_KEY" | "PRIMARY KEY" => Some(Self::NodeKey),
            _ => None,
        }
    }
}

impl fmt::Display for ConstraintType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Supported secondary index types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IndexType {
    /// Standard B-Tree / Range index for equality and range filtering.
    BTree,
    /// Full-text search index.
    Text,
    /// Spatial 2D/3D point index.
    Point,
    /// Approximate Nearest Neighbor (ANN) vector index.
    Vector,
}

impl IndexType {
    /// Returns the canonical index type name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::BTree => "BTREE",
            Self::Text => "TEXT",
            Self::Point => "POINT",
            Self::Vector => "VECTOR",
        }
    }

    /// Parses a case-insensitive index type string.
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "BTREE" | "RANGE" | "DEFAULT" => Some(Self::BTree),
            "TEXT" | "FULLTEXT" => Some(Self::Text),
            "POINT" | "SPATIAL" => Some(Self::Point),
            "VECTOR" => Some(Self::Vector),
            _ => None,
        }
    }
}

impl fmt::Display for IndexType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Metadata descriptor for a single entity property field.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FieldDescriptor {
    /// Property name in the database.
    pub name: String,
    /// Data type of the property.
    pub field_type: FieldType,
    /// Whether this field accepts null values.
    pub nullable: bool,
    /// Whether this field serves as the entity's primary key.
    pub primary_key: bool,
    /// Whether this field has a unique constraint.
    pub unique: bool,
    /// Whether this field is backed by a secondary index.
    pub indexed: bool,
    /// The specific index kind when indexed.
    pub index_type: Option<IndexType>,
    /// Optional default literal value.
    pub default_value: Option<LiteralValue>,
}

impl FieldDescriptor {
    /// Constructs a new field descriptor with default settings.
    pub fn new(name: impl Into<String>, field_type: FieldType) -> Self {
        Self {
            name: name.into(),
            field_type,
            nullable: true,
            primary_key: false,
            unique: false,
            indexed: false,
            index_type: None,
            default_value: None,
        }
    }

    /// Marks this field as a primary key (implies unique and non-nullable).
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.unique = true;
        self.nullable = false;
        self
    }

    /// Marks this field as unique.
    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }

    /// Sets nullability on this field.
    pub fn nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }

    /// Marks this field as indexed with the default B-Tree index.
    pub fn indexed(mut self) -> Self {
        self.indexed = true;
        self.index_type = Some(IndexType::BTree);
        self
    }

    /// Sets a specific index type on this field.
    pub fn with_index_type(mut self, index_type: IndexType) -> Self {
        self.indexed = true;
        self.index_type = Some(index_type);
        self
    }

    /// Sets a default literal value for this field.
    pub fn with_default(mut self, val: LiteralValue) -> Self {
        self.default_value = Some(val);
        self
    }
}

/// Metadata schema defining a Graph Node entity.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeSchema {
    /// High-level model class name (e.g. "User", "Person").
    pub name: String,
    /// Graph database labels associated with this node (e.g. `["Person", "Actor"]`).
    pub labels: Vec<String>,
    /// Property fields keyed by property name.
    pub fields: BTreeMap<String, FieldDescriptor>,
    /// Primary key property name if declared.
    pub primary_key: Option<String>,
}

impl NodeSchema {
    /// Creates a new node schema with an entity name and label list.
    pub fn new(name: impl Into<String>, labels: Vec<String>) -> Self {
        Self {
            name: name.into(),
            labels,
            fields: BTreeMap::new(),
            primary_key: None,
        }
    }

    /// Appends a field descriptor to this node schema.
    pub fn with_field(mut self, field: FieldDescriptor) -> Self {
        if field.primary_key {
            self.primary_key = Some(field.name.clone());
        }
        self.fields.insert(field.name.clone(), field);
        self
    }

    /// Returns the primary label for this node (defaults to the first label or the model name).
    pub fn primary_label(&self) -> &str {
        self.labels
            .first()
            .map(|s| s.as_str())
            .unwrap_or(&self.name)
    }

    /// Returns the field descriptor for the declared primary key, if any.
    pub fn primary_key_field(&self) -> Option<&FieldDescriptor> {
        self.primary_key.as_ref().and_then(|pk| self.fields.get(pk))
    }

    /// Returns all field descriptors with uniqueness constraints.
    pub fn unique_fields(&self) -> Vec<&FieldDescriptor> {
        self.fields
            .values()
            .filter(|f| f.unique || f.primary_key)
            .collect()
    }

    /// Returns all field descriptors with secondary indexes (excluding primary keys).
    pub fn indexed_fields(&self) -> Vec<&FieldDescriptor> {
        self.fields
            .values()
            .filter(|f| f.indexed && !f.primary_key && !f.unique)
            .collect()
    }
}

/// Metadata schema defining a Graph Relationship / Edge entity.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RelationshipSchema {
    /// High-level model class name (e.g. "Follows", "WorksAt").
    pub name: String,
    /// Graph relationship type identifier (e.g. `"FOLLOWS"`, `"WORKS_AT"`).
    pub type_name: String,
    /// Allowed source node labels for this relationship.
    pub source_labels: Vec<String>,
    /// Allowed target node labels for this relationship.
    pub target_labels: Vec<String>,
    /// Property fields keyed by property name.
    pub fields: BTreeMap<String, FieldDescriptor>,
    /// Whether this relationship is directed.
    pub directed: bool,
}

impl RelationshipSchema {
    /// Creates a new relationship schema with an entity name and type name.
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            source_labels: Vec::new(),
            target_labels: Vec::new(),
            fields: BTreeMap::new(),
            directed: true,
        }
    }

    /// Sets the allowed source and target node labels for this relationship.
    pub fn with_endpoints(
        mut self,
        source_labels: Vec<String>,
        target_labels: Vec<String>,
    ) -> Self {
        self.source_labels = source_labels;
        self.target_labels = target_labels;
        self
    }

    /// Appends a field descriptor to this relationship schema.
    pub fn with_field(mut self, field: FieldDescriptor) -> Self {
        self.fields.insert(field.name.clone(), field);
        self
    }

    /// Sets whether this relationship is directed.
    pub fn directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }

    /// Returns all field descriptors with non-null or uniqueness constraints.
    pub fn required_fields(&self) -> Vec<&FieldDescriptor> {
        self.fields
            .values()
            .filter(|f| !f.nullable || f.unique || f.primary_key)
            .collect()
    }
}

/// Central, thread-safe registry holding graph entity schemas and property metadata.
#[derive(Debug, Default)]
pub struct SchemaRegistry {
    nodes: RwLock<BTreeMap<String, NodeSchema>>,
    relationships: RwLock<BTreeMap<String, RelationshipSchema>>,
}

impl SchemaRegistry {
    /// Constructs a new, empty schema registry.
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(BTreeMap::new()),
            relationships: RwLock::new(BTreeMap::new()),
        }
    }

    /// Registers a node schema in the registry.
    ///
    /// # Errors
    /// Returns `Error::SchemaError` if the lock is poisoned.
    pub fn register_node(&self, schema: NodeSchema) -> Result<()> {
        let mut nodes = self
            .nodes
            .write()
            .map_err(|e| Error::SchemaError(format!("SchemaRegistry node lock poisoned: {e}")))?;
        nodes.insert(schema.name.clone(), schema);
        Ok(())
    }

    /// Registers a relationship schema in the registry.
    ///
    /// # Errors
    /// Returns `Error::SchemaError` if the lock is poisoned.
    pub fn register_relationship(&self, schema: RelationshipSchema) -> Result<()> {
        let mut rels = self.relationships.write().map_err(|e| {
            Error::SchemaError(format!("SchemaRegistry relationship lock poisoned: {e}"))
        })?;
        rels.insert(schema.name.clone(), schema);
        Ok(())
    }

    /// Retrieves a node schema by model name.
    pub fn get_node(&self, name: &str) -> Option<NodeSchema> {
        let nodes = self.nodes.read().ok()?;
        nodes.get(name).cloned()
    }

    /// Retrieves a node schema by primary label.
    pub fn get_node_by_label(&self, label: &str) -> Option<NodeSchema> {
        let nodes = self.nodes.read().ok()?;
        nodes
            .values()
            .find(|n| n.labels.iter().any(|l| l.eq_ignore_ascii_case(label)))
            .cloned()
    }

    /// Retrieves a relationship schema by model name.
    pub fn get_relationship(&self, name: &str) -> Option<RelationshipSchema> {
        let rels = self.relationships.read().ok()?;
        rels.get(name).cloned()
    }

    /// Retrieves a relationship schema by graph type name (case-insensitive).
    pub fn get_relationship_by_type(&self, type_name: &str) -> Option<RelationshipSchema> {
        let rels = self.relationships.read().ok()?;
        rels.values()
            .find(|r| r.type_name.eq_ignore_ascii_case(type_name))
            .cloned()
    }

    /// Checks if a node schema is registered by name.
    pub fn has_node(&self, name: &str) -> bool {
        self.nodes
            .read()
            .map(|n| n.contains_key(name))
            .unwrap_or(false)
    }

    /// Checks if a relationship schema is registered by name.
    pub fn has_relationship(&self, name: &str) -> bool {
        self.relationships
            .read()
            .map(|r| r.contains_key(name))
            .unwrap_or(false)
    }

    /// Returns a snapshot of all registered node schemas.
    pub fn node_schemas(&self) -> Vec<NodeSchema> {
        self.nodes
            .read()
            .map(|n| n.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns a snapshot of all registered relationship schemas.
    pub fn relationship_schemas(&self) -> Vec<RelationshipSchema> {
        self.relationships
            .read()
            .map(|r| r.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Clears all registered schemas from this registry.
    pub fn clear(&self) {
        if let Ok(mut nodes) = self.nodes.write() {
            nodes.clear();
        }
        if let Ok(mut rels) = self.relationships.write() {
            rels.clear();
        }
    }

    /// Removes a registered node schema by model name, returning it if present.
    pub fn remove_node(&self, name: &str) -> Option<NodeSchema> {
        let mut nodes = self.nodes.write().ok()?;
        nodes.remove(name)
    }

    /// Removes a registered relationship schema by model name, returning it if present.
    pub fn remove_relationship(&self, name: &str) -> Option<RelationshipSchema> {
        let mut rels = self.relationships.write().ok()?;
        rels.remove(name)
    }

    /// Returns the total count of registered schemas (nodes + relationships).
    pub fn len(&self) -> usize {
        let n = self.nodes.read().map(|m| m.len()).unwrap_or(0);
        let r = self.relationships.read().map(|m| m.len()).unwrap_or(0);
        n + r
    }

    /// Returns true if no node or relationship schemas are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Captures a static clone snapshot of the registry's entire state.
    pub fn snapshot(&self) -> SchemaSnapshot {
        let nodes = self.nodes.read().map(|m| m.clone()).unwrap_or_default();
        let relationships = self
            .relationships
            .read()
            .map(|m| m.clone())
            .unwrap_or_default();
        SchemaSnapshot {
            nodes,
            relationships,
        }
    }

    /// Restores or overwrites the registry with a previously captured snapshot.
    pub fn restore(&self, snapshot: SchemaSnapshot) -> Result<()> {
        let mut nodes = self
            .nodes
            .write()
            .map_err(|e| Error::SchemaError(format!("SchemaRegistry node lock poisoned: {e}")))?;
        let mut rels = self.relationships.write().map_err(|e| {
            Error::SchemaError(format!("SchemaRegistry relationship lock poisoned: {e}"))
        })?;
        *nodes = snapshot.nodes;
        *rels = snapshot.relationships;
        Ok(())
    }

    /// Serializes the entire registry state to a JSON string.
    #[cfg(feature = "serde")]
    pub fn to_json(&self) -> Result<String> {
        let snap = self.snapshot();
        serde_json::to_string(&snap).map_err(|e| Error::SchemaError(e.to_string()))
    }

    /// Deserializes and restores the registry state from a JSON string.
    #[cfg(feature = "serde")]
    pub fn from_json(&self, json_str: &str) -> Result<()> {
        let snap: SchemaSnapshot =
            serde_json::from_str(json_str).map_err(|e| Error::SchemaError(e.to_string()))?;
        self.restore(snap)
    }
}

/// Point-in-time snapshot of all registered node and relationship schemas.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SchemaSnapshot {
    /// Registered node schemas keyed by model name.
    pub nodes: BTreeMap<String, NodeSchema>,
    /// Registered relationship schemas keyed by model name.
    pub relationships: BTreeMap<String, RelationshipSchema>,
}

static GLOBAL_REGISTRY: OnceLock<SchemaRegistry> = OnceLock::new();

/// Returns a reference to the global thread-safe singleton schema registry.
pub fn global_schema_registry() -> &'static SchemaRegistry {
    GLOBAL_REGISTRY.get_or_init(SchemaRegistry::new)
}
