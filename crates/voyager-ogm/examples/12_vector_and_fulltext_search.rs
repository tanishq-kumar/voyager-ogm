//! Voyager OGM: Vector & Full-Text Search DDL & Query Building
//!
//! Teaches:
//! - Declaring vector embeddings on `NodeSchema` using `VectorIndexConfig`
//! - Emitting vector index DDL across Neo4j, Memgraph, DuckDB, and PostgreSQL pgvector
//! - Declaring BM25 full-text search indexes with `FullTextIndexConfig`
//! - Emitting full-text index DDL across dialects
//! - Resolving engine-specific full-text procedure calls via `resolve_fulltext_search()`
//! - Building full-text search queries using `QueryBuilder::fulltext_search()`
//! - Building vector similarity search queries using `QueryBuilder::call_procedure()`
//!
//! Prerequisites:
//! - Engine: None (offline / compile-only)
//! - Crates: voyager-ogm
//!
//! Run with:
//! `cargo run -p voyager-ogm --example 12_vector_and_fulltext_search`

use voyager_ogm::builder::QueryBuilder;
use voyager_ogm::emitters::{CypherEmitter, emit_node_index_ddl};
use voyager_ogm::schema::{
    DdlDialect, FieldDescriptor, FieldType, FullTextIndexConfig, NodeSchema, VectorSimilarity,
};
use voyager_ogm::visitor::AstVisitor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Voyager OGM (Rust) - Vector & Full-Text Search ===\n");

    // -----------------------------------------------------------------------
    // 1. Entity Schema with Vector & Full-Text Descriptors
    // -----------------------------------------------------------------------
    println!("--- [1] Schema Definition with Vector and Full-Text Descriptors ---");
    let doc_schema = NodeSchema::new("Document", vec!["Document".to_string()])
        .with_field(FieldDescriptor::new("doc_id", FieldType::String).primary_key())
        .with_field(FieldDescriptor::new("title", FieldType::String).indexed())
        .with_field(FieldDescriptor::new("body", FieldType::String))
        .with_field(
            FieldDescriptor::new("embedding", FieldType::List(Box::new(FieldType::Float64)))
                .with_vector_index(
                    1536,
                    VectorSimilarity::Cosine,
                    Some("doc_embedding_idx".to_string()),
                ),
        )
        .with_fulltext_index(
            FullTextIndexConfig::new(
                "doc_fulltext_idx",
                vec!["title".to_string(), "body".to_string()],
            )
            .with_analyzer("english"),
        );

    println!("Defined Document schema with 1536-dim vector embedding and full-text index.\n");

    // -----------------------------------------------------------------------
    // 2. Multi-Dialect Vector Index DDL Generation
    // -----------------------------------------------------------------------
    println!("--- [2] Multi-Dialect Vector Index DDL ---");

    // Neo4j 5+ (openCypher db.index.vector)
    let ddl_neo4j = emit_node_index_ddl(&doc_schema, "cypher")?;
    if let Some(vec_stmt) = ddl_neo4j.iter().find(|s| s.contains("VECTOR")) {
        println!("Neo4j openCypher Vector Index DDL:\n  {}\n", vec_stmt);
    }

    // Memgraph (MAGE vector index)
    let ddl_mg = emit_node_index_ddl(&doc_schema, "memgraph")?;
    if let Some(vec_stmt) = ddl_mg.iter().find(|s| s.contains("VECTOR")) {
        println!("Memgraph Vector Index DDL:\n  {}\n", vec_stmt);
    }

    // DuckDB (HNSW extension)
    let ddl_duck = emit_node_index_ddl(&doc_schema, "duckdb")?;
    if let Some(vec_stmt) = ddl_duck.iter().find(|s| s.contains("HNSW")) {
        println!("DuckDB HNSW Vector Index DDL:\n  {}\n", vec_stmt);
    }

    // PostgreSQL (pgvector HNSW)
    let ddl_pg = emit_node_index_ddl(&doc_schema, "postgres")?;
    if let Some(vec_stmt) = ddl_pg.iter().find(|s| s.contains("hnsw")) {
        println!("PostgreSQL pgvector HNSW Index DDL:\n  {}\n", vec_stmt);
    }

    // -----------------------------------------------------------------------
    // 3. Multi-Dialect Full-Text Index DDL Generation
    // -----------------------------------------------------------------------
    println!("--- [3] Multi-Dialect Full-Text Index DDL ---");
    if let Some(ft_stmt) = ddl_neo4j.iter().find(|s| s.contains("FULLTEXT")) {
        println!("Neo4j Full-Text Index DDL:\n  {}\n", ft_stmt);
    }

    // -----------------------------------------------------------------------
    // 4. Construct Full-Text Search Query via QueryBuilder
    // -----------------------------------------------------------------------
    println!("--- [4] Cross-Dialect Full-Text Search Query Building ---");
    let mut b_ft = QueryBuilder::new();
    let ft_properties = vec!["title".to_string(), "body".to_string()];
    b_ft.fulltext_search(
        DdlDialect::Cypher,
        false,
        "doc_fulltext_idx",
        "Document",
        "graph databases OR vector embeddings",
        &ft_properties,
        Some("doc"),
        Some("score"),
    );
    b_ft.where_gte("score", "", 0.75);
    let (arena_ft, root_ft) = b_ft.build();
    let mut cypher_emitter = CypherEmitter::new();
    let compiled_ft = cypher_emitter.visit_query(&arena_ft, root_ft)?;
    println!("Compiled Full-Text Search Query (openCypher):");
    println!("  Statement:  {}\n", compiled_ft.statement);

    // -----------------------------------------------------------------------
    // 5. Construct Vector Similarity Search Query via QueryBuilder
    // -----------------------------------------------------------------------
    println!("--- [5] Vector Similarity Search Query Building ---");
    let mut b_vec = QueryBuilder::new();
    let idx_arg = b_vec.literal("doc_embedding_idx");
    let k_arg = b_vec.literal(10i64);
    let vec_arg = b_vec.param("query_vector");
    b_vec.call_procedure("db.index.vector.queryNodes", vec![idx_arg, k_arg, vec_arg]);
    b_vec.yield_items(vec!["node AS doc".to_string(), "score".to_string()]);
    let (arena_vec, root_vec) = b_vec.build();
    let compiled_vec = cypher_emitter.visit_query(&arena_vec, root_vec)?;
    println!("Compiled Vector Similarity Query (openCypher):");
    println!("  Statement:  {}", compiled_vec.statement);
    println!("  Parameters: {:?}", compiled_vec.parameters);

    println!("\n[SUCCESS] Vector and full-text search demo completed successfully.");
    Ok(())
}
