//! Voyager OGM: Declarative Schema, Constraints & Index DDL
//!
//! Teaches:
//! - Defining node and relationship schemas (`NodeSchema`, `RelationshipSchema`)
//! - Adding fields with data types, primary keys, uniqueness, and indexes
//! - Emitting openCypher constraints and secondary indexes across dialects
//! - Generating Neo4j Cypher 25 declarative Graph Type DDL
//! - Generating ISO/IEC 39075:2024 GQL Graph Type DDL
//! - Generating SQL:2023 PGQ Property Graph Catalog DDL
//! - Generating DROP DDL statements for migrations and cleanups
//!
//! Prerequisites:
//! - Engine: None (offline / compile-only)
//! - Crates: voyager-ogm
//!
//! Run with:
//! `cargo run -p voyager-ogm --example 08_schema_ddl_and_indexes`

use voyager_ogm::emitters::{
    emit_cypher25_drop_graph_type_ddl, emit_cypher25_graph_type_ddl, emit_gql_drop_graph_type_ddl,
    emit_gql_graph_type_ddl, emit_node_constraint_ddl, emit_node_drop_constraint_ddl,
    emit_node_drop_index_ddl, emit_node_index_ddl, emit_pgq_drop_property_graph_ddl,
    emit_pgq_property_graph_ddl, emit_rel_constraint_ddl, emit_rel_index_ddl,
};
use voyager_ogm::schema::{FieldDescriptor, FieldType, NodeSchema, RelationshipSchema};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Voyager OGM (Rust) - Declarative Schema, Constraints & Index DDL ===\n");

    // -----------------------------------------------------------------------
    // 1. Define Entity Schemas with Fields, Types, and Constraints
    // -----------------------------------------------------------------------
    let user_schema = NodeSchema::new("User", vec!["User".to_string()])
        .with_field(FieldDescriptor::new("user_id", FieldType::String).primary_key())
        .with_field(FieldDescriptor::new("email", FieldType::String).unique())
        .with_field(FieldDescriptor::new("age", FieldType::Int64).indexed())
        .with_field(FieldDescriptor::new("city", FieldType::String).indexed())
        .with_field(FieldDescriptor::new("is_active", FieldType::Boolean));

    let follows_schema = RelationshipSchema::new("Follows", "FOLLOWS")
        .with_endpoints(vec!["User".to_string()], vec!["User".to_string()])
        .with_field(FieldDescriptor::new("since", FieldType::Int64).indexed())
        .with_field(FieldDescriptor::new("weight", FieldType::Float64));

    println!("Defined schemas for entity types: User (Node) and FOLLOWS (Relationship)\n");

    // -----------------------------------------------------------------------
    // 2. Multi-Dialect openCypher Constraints & Secondary Indexes
    // -----------------------------------------------------------------------
    println!("--- [1] openCypher (Neo4j / Memgraph / FalkorDB) ---");

    let node_constraints = emit_node_constraint_ddl(&user_schema, "cypher", true)?;
    println!("Node Constraints:");
    for stmt in &node_constraints {
        println!("  -> {}", stmt);
    }

    let node_indexes = emit_node_index_ddl(&user_schema, "cypher")?;
    println!("Node Secondary Indexes:");
    for stmt in &node_indexes {
        println!("  -> {}", stmt);
    }

    let rel_constraints = emit_rel_constraint_ddl(&follows_schema, "cypher", true)?;
    if !rel_constraints.is_empty() {
        println!("Relationship Constraints:");
        for stmt in &rel_constraints {
            println!("  -> {}", stmt);
        }
    }

    let rel_indexes = emit_rel_index_ddl(&follows_schema, "cypher")?;
    println!("Relationship Indexes:");
    for stmt in &rel_indexes {
        println!("  -> {}", stmt);
    }
    println!();

    // -----------------------------------------------------------------------
    // 3. PostgreSQL / DuckPGQ Relational Index DDL
    // -----------------------------------------------------------------------
    println!("--- [2] Relational SQL Indexes (PostgreSQL / DuckPGQ) ---");
    let pg_indexes = emit_node_index_ddl(&user_schema, "postgres")?;
    for stmt in &pg_indexes {
        println!("  -> {}", stmt);
    }
    println!();

    // -----------------------------------------------------------------------
    // 4. Neo4j Cypher 25 Declarative Graph Type (Single Contract)
    // -----------------------------------------------------------------------
    println!("--- [3] Neo4j Cypher 25 Declarative Graph Type ---");
    let cypher25_ddl = emit_cypher25_graph_type_ddl(&[&user_schema], &[&follows_schema]);
    println!("{}\n", cypher25_ddl);

    // -----------------------------------------------------------------------
    // 5. ISO/IEC 39075:2024 GQL Graph Type DDL
    // -----------------------------------------------------------------------
    println!("--- [4] ISO/IEC 39075:2024 GQL Graph Type ---");
    let gql_ddl = emit_gql_graph_type_ddl("SocialGraphType", &[&user_schema], &[&follows_schema]);
    println!("{}\n", gql_ddl);

    // -----------------------------------------------------------------------
    // 6. SQL:2023 PGQ Property Graph Catalog DDL
    // -----------------------------------------------------------------------
    println!("--- [5] SQL:2023 PGQ Property Graph Catalog ---");
    let pgq_ddl = emit_pgq_property_graph_ddl("social_graph", &[&user_schema], &[&follows_schema])?;
    println!("{}\n", pgq_ddl);

    // -----------------------------------------------------------------------
    // 7. Safe Migration Teardown / DROP DDL Statements
    // -----------------------------------------------------------------------
    println!("--- [6] Teardown & DROP Statements ---");
    let drop_constraints = emit_node_drop_constraint_ddl(&user_schema, "cypher", true)?;
    for stmt in &drop_constraints {
        println!("  -> {}", stmt);
    }

    let drop_indexes = emit_node_drop_index_ddl(&user_schema, "cypher")?;
    for stmt in &drop_indexes {
        println!("  -> {}", stmt);
    }

    println!("  -> {}", emit_cypher25_drop_graph_type_ddl());
    println!("  -> {}", emit_gql_drop_graph_type_ddl("SocialGraphType"));
    println!("  -> {}", emit_pgq_drop_property_graph_ddl("social_graph"));

    println!("\n[SUCCESS] Schema DDL and index generation demo completed successfully.");
    Ok(())
}
