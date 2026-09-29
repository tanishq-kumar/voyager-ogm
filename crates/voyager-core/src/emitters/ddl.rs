//! Multi-Dialect DDL Migration Emitters for Voyager OGM.
//!
//! Generates deterministic, standard-compliant schema migration statements
//! across openCypher (Neo4j / Memgraph), ISO GQL, and SQL:2023 PGQ / DuckPGQ
//! directly from native `SchemaRegistry` metadata.

use crate::error::{Error, Result};
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

        // Property existence (NOT NULL) constraint
        if !field.nullable {
            let c_name = cypher_constraint_name(primary_label, db_name, "not_null");
            statements.push(format!(
                "CREATE CONSTRAINT {c_name} IF NOT EXISTS FOR (n:{primary_label}) REQUIRE n.{db_name} IS NOT NULL"
            ));
        }

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
        if !field.nullable {
            let c_name = cypher_constraint_name(primary_label, db_name, "not_null");
            statements.push(format!("DROP CONSTRAINT {c_name} IF EXISTS"));
        }
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
// Neo4j Cypher 25 Graph Types Emitter (ALTER CURRENT GRAPH TYPE SET)
// ---------------------------------------------------------------------------

/// Emits Neo4j Cypher 25 `ALTER CURRENT GRAPH TYPE SET { ... }` declarative schema statement.
pub fn emit_cypher25_graph_type_ddl(
    nodes: &[&NodeSchema],
    relationships: &[&RelationshipSchema],
) -> String {
    let mut elements = Vec::new();

    for node in nodes {
        let primary_label = node.primary_label();
        let implied: Vec<&str> = node.labels.iter().skip(1).map(|s| s.as_str()).collect();
        let implied_str = if implied.is_empty() {
            String::new()
        } else {
            format!(":{}", implied.join("&"))
        };

        let mut prop_defs = Vec::new();
        for field in node.fields.values() {
            let neo4j_type = field_type_to_neo4j(&field.field_type);
            let constraint_suffix = if field.primary_key {
                " IS KEY".to_string()
            } else {
                let mut s = String::new();
                if !field.nullable {
                    s.push_str(" NOT NULL");
                }
                if field.unique {
                    s.push_str(" IS UNIQUE");
                }
                s
            };
            prop_defs.push(format!("{} :: {neo4j_type}{constraint_suffix}", field.name));
        }
        let props_str = if prop_defs.is_empty() {
            String::new()
        } else {
            format!(" {{{}}}", prop_defs.join(", "))
        };

        if implied_str.is_empty() {
            if props_str.is_empty() {
                elements.push(format!("    (:{primary_label})"));
            } else {
                elements.push(format!("    (:{primary_label} =>{props_str})"));
            }
        } else {
            elements.push(format!(
                "    (:{primary_label} => {implied_str}{props_str})"
            ));
        }
    }

    for rel in relationships {
        let type_name = &rel.type_name;
        let mut prop_defs = Vec::new();
        for field in rel.fields.values() {
            let neo4j_type = field_type_to_neo4j(&field.field_type);
            let constraint_suffix = if field.primary_key {
                " IS KEY".to_string()
            } else {
                let mut s = String::new();
                if !field.nullable {
                    s.push_str(" NOT NULL");
                }
                if field.unique {
                    s.push_str(" IS UNIQUE");
                }
                s
            };
            prop_defs.push(format!("{} :: {neo4j_type}{constraint_suffix}", field.name));
        }
        let props_str = if prop_defs.is_empty() {
            String::new()
        } else {
            format!(" {{{}}}", prop_defs.join(", "))
        };

        let src_label = rel.source_labels.first().map(|s| s.as_str());
        let tgt_label = rel.target_labels.first().map(|s| s.as_str());

        let src_pattern = match src_label {
            Some(s) if !s.is_empty() => format!("(:{s})"),
            _ => "()".to_string(),
        };
        let tgt_pattern = match tgt_label {
            Some(s) if !s.is_empty() => format!("(:{s})"),
            _ => "()".to_string(),
        };

        if props_str.is_empty() {
            elements.push(format!("    {src_pattern}-[:{type_name}]->{tgt_pattern}"));
        } else {
            elements.push(format!(
                "    {src_pattern}-[:{type_name} =>{props_str}]->{tgt_pattern}"
            ));
        }
    }

    let body = elements.join(",\n");
    format!("ALTER CURRENT GRAPH TYPE SET {{\n{body}\n}}")
}

/// Emits Neo4j Cypher 25 `ALTER CURRENT GRAPH TYPE SET {}` to reset the graph type.
pub fn emit_cypher25_drop_graph_type_ddl() -> String {
    "ALTER CURRENT GRAPH TYPE SET {}".to_string()
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
            let not_null = if !field.nullable || field.primary_key {
                " NOT NULL"
            } else {
                ""
            };
            prop_defs.push(format!("{} {}{not_null}", field.name, gql_type));
        }
        let props_str = if prop_defs.is_empty() {
            String::new()
        } else {
            format!(" ({})", prop_defs.join(", "))
        };
        let key_str = if let Some(ref pk) = node.primary_key {
            format!(" KEY ({pk})")
        } else {
            String::new()
        };
        elements.push(format!("    NODE {primary_label}{props_str}{key_str}"));
    }

    for rel in relationships {
        let type_name = &rel.type_name;
        let mut prop_defs = Vec::new();
        for field in rel.fields.values() {
            let gql_type = field_type_to_gql(&field.field_type);
            let not_null = if !field.nullable || field.primary_key {
                " NOT NULL"
            } else {
                ""
            };
            prop_defs.push(format!("{} {}{not_null}", field.name, gql_type));
        }
        let props_str = if prop_defs.is_empty() {
            String::new()
        } else {
            format!(" ({})", prop_defs.join(", "))
        };

        let src_label = rel.source_labels.first().map(|s| s.as_str());
        let tgt_label = rel.target_labels.first().map(|s| s.as_str());
        let connecting_str = match (src_label, tgt_label) {
            (Some(s), Some(t)) if !s.is_empty() && !t.is_empty() => {
                format!(" CONNECTING ({s} TO {t})")
            }
            _ => String::new(),
        };

        elements.push(format!("    EDGE {type_name}{connecting_str}{props_str}"));
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
///
/// # Endpoints and Key Convention Note
/// SQL:2023 PGQ edge tables require explicit vertex endpoints (`source_labels` and `target_labels`).
/// If any relationship has empty endpoints, this function returns an `Err`.
/// By convention, edge tables reference source and target vertex tables using foreign key column
/// names `{src_table}_id` and `{tgt_table}_id` (e.g. `member_id`), referencing the target node's primary key.
pub fn emit_pgq_property_graph_ddl(
    graph_name: &str,
    nodes: &[&NodeSchema],
    relationships: &[&RelationshipSchema],
) -> Result<String> {
    let mut v_clauses = Vec::new();
    for node in nodes {
        let primary_label = node.primary_label();
        let table_name = node.name.to_ascii_lowercase();
        let props_clause = if node.fields.is_empty() {
            String::new()
        } else {
            let field_names: Vec<&str> = node.fields.keys().map(|k| k.as_str()).collect();
            format!(" PROPERTIES ({})", field_names.join(", "))
        };

        if let Some(ref pk) = node.primary_key {
            v_clauses.push(format!(
                "    {table_name} KEY ({pk}) LABEL {primary_label}{props_clause}"
            ));
        } else {
            v_clauses.push(format!(
                "    {table_name} LABEL {primary_label}{props_clause}"
            ));
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
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                Error::SchemaError(format!(
                    "Relationship '{}' is missing source_labels. SQL:2023 PGQ EDGE TABLES require explicit vertex source endpoints.",
                    rel.name
                ))
            })?;
        let tgt_label = rel
            .target_labels
            .first()
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                Error::SchemaError(format!(
                    "Relationship '{}' is missing target_labels. SQL:2023 PGQ EDGE TABLES require explicit vertex target endpoints.",
                    rel.name
                ))
            })?;

        let src_table = src_label.to_ascii_lowercase();
        let tgt_table = tgt_label.to_ascii_lowercase();

        let src_key = format!("{src_table}_id");
        let tgt_key = format!("{tgt_table}_id");

        let src_node_pk = nodes
            .iter()
            .find(|n| n.labels.contains(&src_label.to_string()) || n.name == src_label)
            .and_then(|n| n.primary_key.as_deref());
        let tgt_node_pk = nodes
            .iter()
            .find(|n| n.labels.contains(&tgt_label.to_string()) || n.name == tgt_label)
            .and_then(|n| n.primary_key.as_deref());

        let src_ref_col = src_node_pk.unwrap_or(&src_key);
        let tgt_ref_col = tgt_node_pk.unwrap_or(&tgt_key);

        let props_clause = if rel.fields.is_empty() {
            String::new()
        } else {
            let field_names: Vec<&str> = rel.fields.keys().map(|k| k.as_str()).collect();
            format!(" PROPERTIES ({})", field_names.join(", "))
        };

        e_clauses.push(format!(
            "    {table_name}\n      SOURCE KEY ({src_key}) REFERENCES {src_table} ({src_ref_col})\n      DESTINATION KEY ({tgt_key}) REFERENCES {tgt_table} ({tgt_ref_col})\n      LABEL {label}{props_clause}"
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
    Ok(ddl)
}

/// Emits a SQL:2023 PGQ / DuckPGQ `DROP PROPERTY GRAPH` DDL statement.
pub fn emit_pgq_drop_property_graph_ddl(graph_name: &str) -> String {
    format!("DROP PROPERTY GRAPH IF EXISTS {graph_name};")
}

// ---------------------------------------------------------------------------
// Multi-Dialect Pure Index & Constraint DDL Emitters (RFC-0004 §4.4)
// ---------------------------------------------------------------------------

fn escape_sql_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Emits CREATE INDEX DDL statements for a single node across dialects.
pub fn emit_node_index_ddl(node: &NodeSchema, dialect: &str) -> Result<Vec<String>> {
    let dialect_norm = dialect.to_ascii_lowercase();
    let mut statements = Vec::new();
    let primary_label = node.primary_label();

    if dialect_norm == "age" || dialect_norm == "apache_age" {
        return Err(Error::SchemaError(
            "Apache AGE does not support Cypher 'CREATE INDEX' queries. \
             Indexes in Apache AGE must be defined directly on the underlying PostgreSQL relational tables \
             (e.g. CREATE INDEX ON {graph}.\"Label\" USING gin (properties))."
                .to_string(),
        ));
    }

    if dialect_norm == "falkordb" || dialect_norm == "falkor" {
        for field in node.fields.values() {
            if field.indexed && !field.unique && !field.primary_key {
                statements.push(format!(
                    "CREATE INDEX FOR (n:{primary_label}) ON (n.{})",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    if matches!(
        dialect_norm.as_str(),
        "postgres" | "postgresql" | "duckdb" | "sql_pgq" | "pgq"
    ) {
        let table = primary_label.to_ascii_lowercase();
        let q_table = escape_sql_ident(&table);
        for field in node.fields.values() {
            if field.indexed && !field.unique && !field.primary_key {
                let q_col = escape_sql_ident(&field.name);
                statements.push(format!(
                    "CREATE INDEX IF NOT EXISTS idx_{table}_{} ON {q_table} ({q_col});",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    // Default: openCypher (Neo4j / Memgraph)
    for field in node.fields.values() {
        if field.indexed && !field.unique && !field.primary_key {
            let i_name = cypher_index_name(primary_label, &field.name);
            match field.index_type {
                Some(IndexType::Text) => {
                    statements.push(format!(
                        "CREATE TEXT INDEX {i_name} IF NOT EXISTS FOR (n:{primary_label}) ON (n.{})",
                        field.name
                    ));
                }
                Some(IndexType::Point) => {
                    statements.push(format!(
                        "CREATE POINT INDEX {i_name} IF NOT EXISTS FOR (n:{primary_label}) ON (n.{})",
                        field.name
                    ));
                }
                _ => {
                    statements.push(format!(
                        "CREATE INDEX {i_name} IF NOT EXISTS FOR (n:{primary_label}) ON (n.{})",
                        field.name
                    ));
                }
            }
        }
    }
    Ok(statements)
}

/// Emits CREATE INDEX DDL statements for a single relationship across dialects.
pub fn emit_rel_index_ddl(rel: &RelationshipSchema, dialect: &str) -> Result<Vec<String>> {
    let dialect_norm = dialect.to_ascii_lowercase();
    let mut statements = Vec::new();
    let type_name = &rel.type_name;

    if dialect_norm == "age" || dialect_norm == "apache_age" {
        return Err(Error::SchemaError(
            "Apache AGE does not support Cypher 'CREATE INDEX' queries. \
             Indexes in Apache AGE must be defined directly on the underlying PostgreSQL relational tables."
                .to_string(),
        ));
    }

    if dialect_norm == "falkordb" || dialect_norm == "falkor" {
        for field in rel.fields.values() {
            if field.indexed && !field.unique && !field.primary_key {
                statements.push(format!(
                    "CREATE INDEX FOR ()-[r:{type_name}]-() ON (r.{})",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    if matches!(
        dialect_norm.as_str(),
        "postgres" | "postgresql" | "duckdb" | "sql_pgq" | "pgq"
    ) {
        let table = type_name.to_ascii_lowercase();
        let q_table = escape_sql_ident(&table);
        for field in rel.fields.values() {
            if field.indexed && !field.unique && !field.primary_key {
                let q_col = escape_sql_ident(&field.name);
                statements.push(format!(
                    "CREATE INDEX IF NOT EXISTS idx_{table}_{} ON {q_table} ({q_col});",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    // Default: openCypher
    for field in rel.fields.values() {
        if field.indexed && !field.unique && !field.primary_key {
            let i_name = cypher_rel_index_name(type_name, &field.name);
            statements.push(format!(
                "CREATE INDEX {i_name} IF NOT EXISTS FOR ()-[r:{type_name}]-() ON (r.{})",
                field.name
            ));
        }
    }
    Ok(statements)
}

/// Emits CREATE CONSTRAINT DDL statements for a single node across dialects.
pub fn emit_node_constraint_ddl(
    node: &NodeSchema,
    dialect: &str,
    include_type_constraints: bool,
) -> Result<Vec<String>> {
    let dialect_norm = dialect.to_ascii_lowercase();
    let mut statements = Vec::new();
    let primary_label = node.primary_label();

    if dialect_norm == "age" || dialect_norm == "apache_age" {
        return Err(Error::SchemaError(
            "Apache AGE does not support Cypher 'CREATE CONSTRAINT' queries. \
             Constraints in Apache AGE must be defined directly on the underlying PostgreSQL relational tables."
                .to_string(),
        ));
    }

    if dialect_norm == "falkordb" || dialect_norm == "falkor" {
        return Err(Error::SchemaError(
            "FalkorDB does not support openCypher 'CREATE CONSTRAINT' queries in GRAPH.QUERY. \
             Constraints in FalkorDB must be created via native Redis commands \
             ('GRAPH.CONSTRAINT CREATE <graph_name> UNIQUE NODE <label> PROPERTIES 1 <prop>')."
                .to_string(),
        ));
    }

    if matches!(
        dialect_norm.as_str(),
        "postgres" | "postgresql" | "duckdb" | "sql_pgq" | "pgq"
    ) {
        let table = primary_label.to_ascii_lowercase();
        let q_table = escape_sql_ident(&table);
        for field in node.fields.values() {
            if field.unique && !field.primary_key {
                let q_col = escape_sql_ident(&field.name);
                statements.push(format!(
                    "ALTER TABLE {q_table} ADD CONSTRAINT uq_{table}_{} UNIQUE ({q_col});",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    // Default: openCypher
    for field in node.fields.values() {
        let db_name = &field.name;
        if !field.nullable {
            let c_name = cypher_constraint_name(primary_label, db_name, "not_null");
            statements.push(format!(
                "CREATE CONSTRAINT {c_name} IF NOT EXISTS FOR (n:{primary_label}) REQUIRE n.{db_name} IS NOT NULL"
            ));
        }
        if field.unique || field.primary_key {
            let c_name = cypher_constraint_name(primary_label, db_name, "unique");
            statements.push(format!(
                "CREATE CONSTRAINT {c_name} IF NOT EXISTS FOR (n:{primary_label}) REQUIRE n.{db_name} IS UNIQUE"
            ));
        }
        if include_type_constraints {
            let neo4j_type = field_type_to_neo4j(&field.field_type);
            let t_name = cypher_constraint_name(primary_label, db_name, "type");
            statements.push(format!(
                "CREATE CONSTRAINT {t_name} IF NOT EXISTS FOR (n:{primary_label}) REQUIRE n.{db_name} :: {neo4j_type}"
            ));
        }
    }
    Ok(statements)
}

/// Emits CREATE CONSTRAINT DDL statements for a single relationship across dialects.
pub fn emit_rel_constraint_ddl(
    rel: &RelationshipSchema,
    dialect: &str,
    include_type_constraints: bool,
) -> Result<Vec<String>> {
    let dialect_norm = dialect.to_ascii_lowercase();
    let mut statements = Vec::new();
    let type_name = &rel.type_name;

    if dialect_norm == "age" || dialect_norm == "apache_age" {
        return Err(Error::SchemaError(
            "Apache AGE does not support Cypher 'CREATE CONSTRAINT' queries. \
             Constraints in Apache AGE must be defined directly on the underlying PostgreSQL relational tables."
                .to_string(),
        ));
    }

    if dialect_norm == "falkordb" || dialect_norm == "falkor" {
        return Err(Error::SchemaError(
            "FalkorDB does not support openCypher 'CREATE CONSTRAINT' queries in GRAPH.QUERY. \
             Constraints in FalkorDB must be created via native Redis commands."
                .to_string(),
        ));
    }

    if matches!(
        dialect_norm.as_str(),
        "postgres" | "postgresql" | "duckdb" | "sql_pgq" | "pgq"
    ) {
        let table = type_name.to_ascii_lowercase();
        let q_table = escape_sql_ident(&table);
        for field in rel.fields.values() {
            if field.unique && !field.primary_key {
                let q_col = escape_sql_ident(&field.name);
                statements.push(format!(
                    "ALTER TABLE {q_table} ADD CONSTRAINT uq_{table}_{} UNIQUE ({q_col});",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    // Default: openCypher
    for field in rel.fields.values() {
        let db_name = &field.name;
        if field.primary_key || field.unique || !field.nullable {
            let c_name = cypher_rel_constraint_name(type_name, db_name, "not_null");
            statements.push(format!(
                "CREATE CONSTRAINT {c_name} IF NOT EXISTS FOR ()-[r:{type_name}]-() REQUIRE r.{db_name} IS NOT NULL"
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
    Ok(statements)
}

/// Emits DROP INDEX DDL statements for a single node across dialects.
pub fn emit_node_drop_index_ddl(node: &NodeSchema, dialect: &str) -> Result<Vec<String>> {
    let dialect_norm = dialect.to_ascii_lowercase();
    let mut statements = Vec::new();
    let primary_label = node.primary_label();

    if dialect_norm == "age" || dialect_norm == "apache_age" {
        return Err(Error::SchemaError(
            "Apache AGE does not support Cypher 'DROP INDEX' queries. \
             Indexes in Apache AGE must be dropped directly on the underlying PostgreSQL relational tables."
                .to_string(),
        ));
    }

    if dialect_norm == "falkordb" || dialect_norm == "falkor" {
        for field in node.fields.values() {
            if field.indexed && !field.unique && !field.primary_key {
                statements.push(format!(
                    "DROP INDEX FOR (n:{primary_label}) ON (n.{})",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    if matches!(
        dialect_norm.as_str(),
        "postgres" | "postgresql" | "duckdb" | "sql_pgq" | "pgq"
    ) {
        let table = primary_label.to_ascii_lowercase();
        for field in node.fields.values() {
            if field.indexed && !field.unique && !field.primary_key {
                statements.push(format!("DROP INDEX IF EXISTS idx_{table}_{};", field.name));
            }
        }
        return Ok(statements);
    }

    // Default: openCypher
    for field in node.fields.values() {
        if field.indexed {
            let i_name = cypher_index_name(primary_label, &field.name);
            statements.push(format!("DROP INDEX {i_name} IF EXISTS"));
        }
    }
    Ok(statements)
}

/// Emits DROP INDEX DDL statements for a single relationship across dialects.
pub fn emit_rel_drop_index_ddl(rel: &RelationshipSchema, dialect: &str) -> Result<Vec<String>> {
    let dialect_norm = dialect.to_ascii_lowercase();
    let mut statements = Vec::new();
    let type_name = &rel.type_name;

    if dialect_norm == "age" || dialect_norm == "apache_age" {
        return Err(Error::SchemaError(
            "Apache AGE does not support Cypher 'DROP INDEX' queries. \
             Indexes in Apache AGE must be dropped directly on the underlying PostgreSQL relational tables."
                .to_string(),
        ));
    }

    if dialect_norm == "falkordb" || dialect_norm == "falkor" {
        for field in rel.fields.values() {
            if field.indexed && !field.unique && !field.primary_key {
                statements.push(format!(
                    "DROP INDEX FOR ()-[r:{type_name}]-() ON (r.{})",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    if matches!(
        dialect_norm.as_str(),
        "postgres" | "postgresql" | "duckdb" | "sql_pgq" | "pgq"
    ) {
        let table = type_name.to_ascii_lowercase();
        for field in rel.fields.values() {
            if field.indexed && !field.unique && !field.primary_key {
                statements.push(format!("DROP INDEX IF EXISTS idx_{table}_{};", field.name));
            }
        }
        return Ok(statements);
    }

    // Default: openCypher
    for field in rel.fields.values() {
        if field.indexed && !field.primary_key && !field.unique {
            let i_name = cypher_rel_index_name(type_name, &field.name);
            statements.push(format!("DROP INDEX {i_name} IF EXISTS"));
        }
    }
    Ok(statements)
}

/// Emits DROP CONSTRAINT DDL statements for a single node across dialects.
pub fn emit_node_drop_constraint_ddl(
    node: &NodeSchema,
    dialect: &str,
    include_type_constraints: bool,
) -> Result<Vec<String>> {
    let dialect_norm = dialect.to_ascii_lowercase();
    let mut statements = Vec::new();
    let primary_label = node.primary_label();

    if dialect_norm == "age" || dialect_norm == "apache_age" {
        return Err(Error::SchemaError(
            "Apache AGE does not support Cypher 'DROP CONSTRAINT' queries. \
             Constraints in Apache AGE must be dropped directly on the underlying PostgreSQL relational tables."
                .to_string(),
        ));
    }

    if dialect_norm == "falkordb" || dialect_norm == "falkor" {
        return Err(Error::SchemaError(
            "FalkorDB does not support openCypher 'DROP CONSTRAINT' queries in GRAPH.QUERY. \
             Constraints in FalkorDB must be dropped via native Redis commands \
             ('GRAPH.CONSTRAINT DROP <graph_name> UNIQUE NODE <label> PROPERTIES 1 <prop>')."
                .to_string(),
        ));
    }

    if matches!(
        dialect_norm.as_str(),
        "postgres" | "postgresql" | "duckdb" | "sql_pgq" | "pgq"
    ) {
        let table = primary_label.to_ascii_lowercase();
        let q_table = escape_sql_ident(&table);
        for field in node.fields.values() {
            if field.unique && !field.primary_key {
                statements.push(format!(
                    "ALTER TABLE {q_table} DROP CONSTRAINT IF EXISTS uq_{table}_{};",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    // Default: openCypher
    for field in node.fields.values() {
        let db_name = &field.name;
        if !field.nullable {
            let c_name = cypher_constraint_name(primary_label, db_name, "not_null");
            statements.push(format!("DROP CONSTRAINT {c_name} IF EXISTS"));
        }
        if field.unique || field.primary_key {
            let c_name = cypher_constraint_name(primary_label, db_name, "unique");
            statements.push(format!("DROP CONSTRAINT {c_name} IF EXISTS"));
        }
        if include_type_constraints {
            let t_name = cypher_constraint_name(primary_label, db_name, "type");
            statements.push(format!("DROP CONSTRAINT {t_name} IF EXISTS"));
        }
    }
    Ok(statements)
}

/// Emits DROP CONSTRAINT DDL statements for a single relationship across dialects.
pub fn emit_rel_drop_constraint_ddl(
    rel: &RelationshipSchema,
    dialect: &str,
    include_type_constraints: bool,
) -> Result<Vec<String>> {
    let dialect_norm = dialect.to_ascii_lowercase();
    let mut statements = Vec::new();
    let type_name = &rel.type_name;

    if dialect_norm == "age" || dialect_norm == "apache_age" {
        return Err(Error::SchemaError(
            "Apache AGE does not support Cypher 'DROP CONSTRAINT' queries. \
             Constraints in Apache AGE must be dropped directly on the underlying PostgreSQL relational tables."
                .to_string(),
        ));
    }

    if dialect_norm == "falkordb" || dialect_norm == "falkor" {
        return Err(Error::SchemaError(
            "FalkorDB does not support openCypher 'DROP CONSTRAINT' queries in GRAPH.QUERY. \
             Constraints in FalkorDB must be dropped via native Redis commands."
                .to_string(),
        ));
    }

    if matches!(
        dialect_norm.as_str(),
        "postgres" | "postgresql" | "duckdb" | "sql_pgq" | "pgq"
    ) {
        let table = type_name.to_ascii_lowercase();
        let q_table = escape_sql_ident(&table);
        for field in rel.fields.values() {
            if field.unique && !field.primary_key {
                statements.push(format!(
                    "ALTER TABLE {q_table} DROP CONSTRAINT IF EXISTS uq_{table}_{};",
                    field.name
                ));
            }
        }
        return Ok(statements);
    }

    // Default: openCypher
    for field in rel.fields.values() {
        let db_name = &field.name;
        if field.primary_key || field.unique || !field.nullable {
            let c_name = cypher_rel_constraint_name(type_name, db_name, "not_null");
            statements.push(format!("DROP CONSTRAINT {c_name} IF EXISTS"));
        }
        if include_type_constraints {
            let t_name = cypher_rel_constraint_name(type_name, db_name, "type");
            statements.push(format!("DROP CONSTRAINT {t_name} IF EXISTS"));
        }
    }
    Ok(statements)
}
