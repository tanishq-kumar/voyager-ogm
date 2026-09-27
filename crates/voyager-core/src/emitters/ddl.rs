//! Multi-Dialect DDL Migration Emitters for Voyager OGM.
//!
//! Generates deterministic, standard-compliant schema migration statements
//! across openCypher (Neo4j / Memgraph), ISO GQL, and SQL:2023 PGQ / DuckPGQ
//! directly from native `SchemaRegistry` metadata.

use crate::schema::{FieldType, IndexType, NodeSchema, RelationshipSchema};

/// Maps a canonical `FieldType` to the openCypher / Neo4j 5.x property type identifier.
pub fn field_type_to_neo4j(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::String => "STRING",
        FieldType::Int64 => "INTEGER",
        FieldType::Float64 => "FLOAT",
        FieldType::Boolean => "BOOLEAN",
        FieldType::DateTime => "DATETIME",
        FieldType::Date => "DATE",
        FieldType::Duration => "DURATION",
        FieldType::List(_) => "LIST",
        FieldType::Map => "MAP",
        FieldType::Any => "ANY",
        FieldType::Custom(s) => match s.to_ascii_uppercase().as_str() {
            "STRING" | "STR" | "TEXT" | "VARCHAR" => "STRING",
            "INTEGER" | "INT" | "INT64" | "BIGINT" => "INTEGER",
            "FLOAT" | "FLOAT64" | "DOUBLE" => "FLOAT",
            "BOOLEAN" | "BOOL" => "BOOLEAN",
            "DATETIME" | "TIMESTAMP" => "DATETIME",
            "DATE" => "DATE",
            "DURATION" | "INTERVAL" => "DURATION",
            "LIST" | "ARRAY" => "LIST",
            "MAP" | "DICT" => "MAP",
            _ => "ANY",
        },
    }
}

/// Maps a canonical `FieldType` to the ISO GQL standard type name.
pub fn field_type_to_gql(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::String => "STRING",
        FieldType::Int64 => "INTEGER",
        FieldType::Float64 => "FLOAT",
        FieldType::Boolean => "BOOLEAN",
        FieldType::DateTime => "DATETIME",
        FieldType::Date => "DATE",
        FieldType::Duration => "DURATION",
        FieldType::List(_) => "LIST",
        FieldType::Map => "MAP",
        FieldType::Any => "ANY",
        FieldType::Custom(s) => match s.to_ascii_uppercase().as_str() {
            "STRING" | "STR" | "TEXT" | "VARCHAR" => "STRING",
            "INTEGER" | "INT" | "INT64" | "BIGINT" => "INTEGER",
            "FLOAT" | "FLOAT64" | "DOUBLE" => "FLOAT",
            "BOOLEAN" | "BOOL" => "BOOLEAN",
            "DATETIME" | "TIMESTAMP" => "DATETIME",
            "DATE" => "DATE",
            "DURATION" | "INTERVAL" => "DURATION",
            "LIST" | "ARRAY" => "LIST",
            _ => "ANY",
        },
    }
}

/// Maps a canonical `FieldType` to standard SQL / SQL:2023 PGQ data types.
pub fn field_type_to_pgq(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::String => "VARCHAR",
        FieldType::Int64 => "BIGINT",
        FieldType::Float64 => "DOUBLE PRECISION",
        FieldType::Boolean => "BOOLEAN",
        FieldType::DateTime => "TIMESTAMP WITH TIME ZONE",
        FieldType::Date => "DATE",
        FieldType::Duration => "INTERVAL",
        FieldType::List(_) => "ARRAY",
        FieldType::Map => "JSON",
        FieldType::Any => "VARCHAR",
        FieldType::Custom(s) => match s.to_ascii_uppercase().as_str() {
            "STRING" | "STR" | "TEXT" | "VARCHAR" => "VARCHAR",
            "INTEGER" | "INT" | "INT64" | "BIGINT" => "BIGINT",
            "FLOAT" | "FLOAT64" | "DOUBLE" => "DOUBLE PRECISION",
            "BOOLEAN" | "BOOL" => "BOOLEAN",
            "DATETIME" | "TIMESTAMP" => "TIMESTAMP WITH TIME ZONE",
            "DATE" => "DATE",
            "DURATION" | "INTERVAL" => "INTERVAL",
            "LIST" | "ARRAY" => "ARRAY",
            "MAP" | "JSON" | "JSONB" => "JSON",
            _ => "VARCHAR",
        },
    }
}

/// Constructs a deterministic openCypher constraint identifier name.
pub fn cypher_constraint_name(label: &str, prop: &str, kind: &str) -> String {
    format!(
        "constraint_{}_{}_{}",
        label.to_ascii_lowercase(),
        prop.to_ascii_lowercase(),
        kind
    )
}

/// Constructs a deterministic openCypher relationship constraint identifier name.
pub fn cypher_rel_constraint_name(rel_type: &str, prop: &str, kind: &str) -> String {
    format!(
        "constraint_rel_{}_{}_{}",
        rel_type.to_ascii_lowercase(),
        prop.to_ascii_lowercase(),
        kind
    )
}

/// Constructs a deterministic openCypher secondary index identifier name.
pub fn cypher_index_name(label: &str, prop: &str) -> String {
    format!(
        "index_{}_{}",
        label.to_ascii_lowercase(),
        prop.to_ascii_lowercase()
    )
}

/// Constructs a deterministic openCypher relationship index identifier name.
pub fn cypher_rel_index_name(rel_type: &str, prop: &str) -> String {
    format!(
        "index_rel_{}_{}",
        rel_type.to_ascii_lowercase(),
        prop.to_ascii_lowercase()
    )
}

// ---------------------------------------------------------------------------
// openCypher DDL Emitter (Neo4j / Memgraph)
// ---------------------------------------------------------------------------

/// Emits openCypher constraint and index creation statements for a node schema.
pub fn emit_cypher_node_ddl(node: &NodeSchema, include_type_constraints: bool) -> Vec<String> {
    let mut statements = Vec::new();
    let primary_label = node.primary_label();

    for field in node.fields.values() {
        let db_name = &field.name;

        // Unique / Primary Key constraint
        if field.unique || field.primary_key {
            let c_name = cypher_constraint_name(primary_label, db_name, "unique");
            statements.push(format!(
                "CREATE CONSTRAINT {c_name} IF NOT EXISTS FOR (n:{primary_label}) REQUIRE n.{db_name} IS UNIQUE"
            ));
        }

        // Secondary index (only if not already protected by unique constraint)
        if field.indexed && !field.unique && !field.primary_key {
            let i_name = cypher_index_name(primary_label, db_name);
            match field.index_type {
                Some(IndexType::Text) => {
                    statements.push(format!(
                        "CREATE TEXT INDEX {i_name} IF NOT EXISTS FOR (n:{primary_label}) ON (n.{db_name})"
                    ));
                }
                Some(IndexType::Point) => {
                    statements.push(format!(
                        "CREATE POINT INDEX {i_name} IF NOT EXISTS FOR (n:{primary_label}) ON (n.{db_name})"
                    ));
                }
                _ => {
                    statements.push(format!(
                        "CREATE INDEX {i_name} IF NOT EXISTS FOR (n:{primary_label}) ON (n.{db_name})"
                    ));
                }
            }
        }

        // Neo4j 5.x Property Type Constraints (Graph Types)
        if include_type_constraints {
            let neo4j_type = field_type_to_neo4j(&field.field_type);
            let t_name = cypher_constraint_name(primary_label, db_name, "type");
            statements.push(format!(
                "CREATE CONSTRAINT {t_name} IF NOT EXISTS FOR (n:{primary_label}) REQUIRE n.{db_name} :: {neo4j_type}"
            ));
        }
    }

    statements
}

/// Emits openCypher constraint and index statements for a relationship schema.
pub fn emit_cypher_rel_ddl(
    rel: &RelationshipSchema,
    include_type_constraints: bool,
) -> Vec<String> {
    let mut statements = Vec::new();
    let type_name = &rel.type_name;

    for field in rel.fields.values() {
        let db_name = &field.name;
        if field.primary_key || field.unique || !field.nullable {
            let c_name = cypher_rel_constraint_name(type_name, db_name, "not_null");
            statements.push(format!(
                "CREATE CONSTRAINT {c_name} IF NOT EXISTS FOR ()-[r:{type_name}]-() REQUIRE r.{db_name} IS NOT NULL"
            ));
        }

        if field.indexed && !field.primary_key && !field.unique {
            let i_name = cypher_rel_index_name(type_name, db_name);
            statements.push(format!(
                "CREATE INDEX {i_name} IF NOT EXISTS FOR ()-[r:{type_name}]-() ON (r.{db_name})"
            ));
        }

        if include_type_constraints {
            let neo4j_type = field_type_to_neo4j(&field.field_type);
            let t_name = cypher_rel_constraint_name(type_name, db_name, "type");
            statements.push(format!(
                "CREATE CONSTRAINT {t_name} IF NOT EXISTS FOR ()-[r:{type_name}]-() REQUIRE r.{db_name} :: {neo4j_type}"
            ));
        }
    }

    statements
}

/// Emits openCypher DROP statements for a node schema.
pub fn emit_cypher_drop_node_ddl(node: &NodeSchema, include_type_constraints: bool) -> Vec<String> {
    let mut statements = Vec::new();
    let primary_label = node.primary_label();

    for field in node.fields.values() {
        let db_name = &field.name;
        if field.unique || field.primary_key {
            let c_name = cypher_constraint_name(primary_label, db_name, "unique");
            statements.push(format!("DROP CONSTRAINT {c_name} IF EXISTS"));
        }
        if field.indexed {
            let i_name = cypher_index_name(primary_label, db_name);
            statements.push(format!("DROP INDEX {i_name} IF EXISTS"));
        }
        if include_type_constraints {
            let t_name = cypher_constraint_name(primary_label, db_name, "type");
            statements.push(format!("DROP CONSTRAINT {t_name} IF EXISTS"));
        }
    }

    statements
}

/// Emits openCypher DROP statements for a relationship schema.
pub fn emit_cypher_drop_rel_ddl(
    rel: &RelationshipSchema,
    include_type_constraints: bool,
) -> Vec<String> {
    let mut statements = Vec::new();
    let type_name = &rel.type_name;

    for field in rel.fields.values() {
        let db_name = &field.name;
        if field.primary_key || field.unique || !field.nullable {
            let c_name = cypher_rel_constraint_name(type_name, db_name, "not_null");
            statements.push(format!("DROP CONSTRAINT {c_name} IF EXISTS"));
        }
        if field.indexed && !field.primary_key && !field.unique {
            let i_name = cypher_rel_index_name(type_name, db_name);
            statements.push(format!("DROP INDEX {i_name} IF EXISTS"));
        }
        if include_type_constraints {
            let t_name = cypher_rel_constraint_name(type_name, db_name, "type");
            statements.push(format!("DROP CONSTRAINT {t_name} IF EXISTS"));
        }
    }

    statements
}

// ---------------------------------------------------------------------------
// ISO GQL DDL Emitter
// ---------------------------------------------------------------------------

/// Emits an ISO GQL `CREATE GRAPH TYPE <name> AS { ... }` DDL definition.
pub fn emit_gql_graph_type_ddl(
    graph_type_name: &str,
    nodes: &[&NodeSchema],
    relationships: &[&RelationshipSchema],
) -> String {
    let mut elements = Vec::new();

    for node in nodes {
        let primary_label = node.primary_label();
        let mut prop_defs = Vec::new();
        for field in node.fields.values() {
            let gql_type = field_type_to_gql(&field.field_type);
            prop_defs.push(format!("{} {}", field.name, gql_type));
        }
        let props_str = if prop_defs.is_empty() {
            String::new()
        } else {
            format!(" ({})", prop_defs.join(", "))
        };
        elements.push(format!("    NODE {primary_label}{props_str}"));
    }

    for rel in relationships {
        let type_name = &rel.type_name;
        let mut prop_defs = Vec::new();
        for field in rel.fields.values() {
            let gql_type = field_type_to_gql(&field.field_type);
            prop_defs.push(format!("{} {}", field.name, gql_type));
        }
        let props_str = if prop_defs.is_empty() {
            String::new()
        } else {
            format!(" ({})", prop_defs.join(", "))
        };
        elements.push(format!("    EDGE {type_name}{props_str}"));
    }

    let body = elements.join(",\n");
    format!("CREATE GRAPH TYPE {graph_type_name} AS {{\n{body}\n}}")
}

/// Emits an experimental `ALTER CURRENT GRAPH TYPE ADD NODE TYPE` DDL statement.
pub fn emit_gql_alter_node_ddl(node: &NodeSchema) -> String {
    let primary_label = node.primary_label();
    let mut prop_defs = Vec::new();

    for field in node.fields.values() {
        let gql_type = field_type_to_gql(&field.field_type);
        let optional_marker = if field.primary_key || field.unique || !field.nullable {
            ""
        } else {
            "?"
        };
        prop_defs.push(format!("{} :: {gql_type}{optional_marker}", field.name));
    }

    let props_str = if prop_defs.is_empty() {
        String::new()
    } else {
        format!(" {{{}}}", prop_defs.join(", "))
    };

    format!("ALTER CURRENT GRAPH TYPE ADD NODE TYPE (:{primary_label}{props_str})")
}

/// Emits an experimental `ALTER CURRENT GRAPH TYPE ADD RELATIONSHIP TYPE` DDL statement.
pub fn emit_gql_alter_rel_ddl(
    rel: &RelationshipSchema,
    source_label: Option<&str>,
    target_label: Option<&str>,
) -> String {
    let type_name = &rel.type_name;
    let mut prop_defs = Vec::new();

    for field in rel.fields.values() {
        let gql_type = field_type_to_gql(&field.field_type);
        prop_defs.push(format!("{} :: {gql_type}", field.name));
    }

    let props_str = if prop_defs.is_empty() {
        String::new()
    } else {
        format!(" {{{}}}", prop_defs.join(", "))
    };

    let src_label = source_label.or_else(|| rel.source_labels.first().map(|s| s.as_str()));
    let tgt_label = target_label.or_else(|| rel.target_labels.first().map(|s| s.as_str()));

    let src_pattern = match src_label {
        Some(s) if !s.is_empty() => format!("(:{s})"),
        _ => "()".to_string(),
    };
    let tgt_pattern = match tgt_label {
        Some(s) if !s.is_empty() => format!("(:{s})"),
        _ => "()".to_string(),
    };

    format!(
        "ALTER CURRENT GRAPH TYPE ADD RELATIONSHIP TYPE {src_pattern}-[:{type_name}{props_str}]->{tgt_pattern}"
    )
}

/// Emits an ISO GQL `DROP GRAPH TYPE` DDL statement.
pub fn emit_gql_drop_graph_type_ddl(graph_type_name: &str) -> String {
    format!("DROP GRAPH TYPE {graph_type_name} IF EXISTS")
}

// ---------------------------------------------------------------------------
// SQL:2023 PGQ DDL Emitter (PostgreSQL / DuckPGQ)
// ---------------------------------------------------------------------------

/// Emits a SQL:2023 PGQ / DuckPGQ `CREATE PROPERTY GRAPH` DDL statement.
pub fn emit_pgq_property_graph_ddl(
    graph_name: &str,
    nodes: &[&NodeSchema],
    relationships: &[&RelationshipSchema],
) -> String {
    let mut v_clauses = Vec::new();
    for node in nodes {
        let primary_label = node.primary_label();
        let table_name = node.name.to_ascii_lowercase();
        if let Some(ref pk) = node.primary_key {
            v_clauses.push(format!("    {table_name} KEY ({pk}) LABEL {primary_label}"));
        } else {
            v_clauses.push(format!("    {table_name} LABEL {primary_label}"));
        }
    }

    let mut e_clauses = Vec::new();
    for rel in relationships {
        let table_name = rel.name.to_ascii_lowercase();
        let label = &rel.type_name;

        let src_label = rel
            .source_labels
            .first()
            .map(|s| s.as_str())
            .unwrap_or("source");
        let tgt_label = rel
            .target_labels
            .first()
            .map(|s| s.as_str())
            .unwrap_or("target");

        let src_table = src_label.to_ascii_lowercase();
        let tgt_table = tgt_label.to_ascii_lowercase();

        let src_key = format!("{src_table}_id");
        let tgt_key = format!("{tgt_table}_id");

        e_clauses.push(format!(
            "    {table_name}\n      SOURCE KEY ({src_key}) REFERENCES {src_table}\n      DESTINATION KEY ({tgt_key}) REFERENCES {tgt_table}\n      LABEL {label}"
        ));
    }

    let vertices_block = v_clauses.join(",\n");
    let edges_block = e_clauses.join(",\n");

    let mut ddl = format!("CREATE PROPERTY GRAPH {graph_name}\n");
    if !vertices_block.is_empty() {
        ddl.push_str(&format!("  VERTEX TABLES (\n{vertices_block}\n  )\n"));
    }
    if !edges_block.is_empty() {
        ddl.push_str(&format!("  EDGE TABLES (\n{edges_block}\n  )"));
    }
    ddl.push(';');
    ddl
}

/// Emits a SQL:2023 PGQ / DuckPGQ `DROP PROPERTY GRAPH` DDL statement.
pub fn emit_pgq_drop_property_graph_ddl(graph_name: &str) -> String {
    format!("DROP PROPERTY GRAPH IF EXISTS {graph_name};")
}
