//! Voyager OGM: Bulk Ingestion Query Template Compilation
//!
//! Teaches:
//! - Compiling vectorized bulk node creation with `bulk::compile_bulk_create`
//! - High-throughput `UNWIND $batch AS row` pattern generation
//! - Compiling idempotent bulk upserts with `bulk::compile_bulk_merge`
//! - Compiling bulk relationship creation with `bulk::compile_bulk_create_rel`
//! - Multi-dialect bulk template generation (openCypher and ISO/IEC 39075:2024 GQL)
//!
//! Prerequisites:
//! - Engine: None (offline / compile-only)
//! - Crates: voyager-ogm
//!
//! Run with:
//! `cargo run -p voyager-ogm --example 10_bulk_ingest_compilation`

use voyager_ogm::bulk::{compile_bulk_create, compile_bulk_create_rel, compile_bulk_merge};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Voyager OGM (Rust) - Bulk Ingestion Template Compilation ===\n");

    // -----------------------------------------------------------------------
    // 1. Bulk Node Creation (CREATE)
    // -----------------------------------------------------------------------
    println!("--- [1] Bulk Node Creation Template ---");
    let create_cypher = compile_bulk_create(
        "User",
        &["id", "username", "department", "score"],
        "batch",
        "row",
        "cypher",
    )?;
    println!("openCypher Bulk Create Template:");
    println!("  Statement:  {}", create_cypher.statement);
    println!("  Parameters: {:?}\n", create_cypher.parameters);

    let create_gql = compile_bulk_create(
        "User",
        &["id", "username", "department", "score"],
        "batch",
        "row",
        "iso_gql",
    )?;
    println!("ISO GQL Bulk Create Template:");
    println!("  Statement:  {}\n", create_gql.statement);

    // -----------------------------------------------------------------------
    // 2. Idempotent Bulk Node Upsert (MERGE)
    // -----------------------------------------------------------------------
    println!("--- [2] Idempotent Bulk Node Upsert Template (MERGE) ---");
    let merge_cypher = compile_bulk_merge(
        "User",
        "id",
        &["username", "department", "score"],
        "batch",
        "row",
        "cypher",
    )?;
    println!("openCypher Bulk Merge Template:");
    println!("  Statement:  {}", merge_cypher.statement);
    println!("  Parameters: {:?}\n", merge_cypher.parameters);

    let merge_gql = compile_bulk_merge(
        "User",
        "id",
        &["username", "department", "score"],
        "batch",
        "row",
        "iso_gql",
    )?;
    println!("ISO GQL Bulk Merge Template:");
    println!("  Statement:  {}\n", merge_gql.statement);

    // -----------------------------------------------------------------------
    // 3. Bulk Relationship Creation Linking Entities by Key
    // -----------------------------------------------------------------------
    println!("--- [3] Bulk Relationship Linking Template ---");
    let rel_cypher = compile_bulk_create_rel(
        "COLLABORATES_WITH",
        &["project", "hours"],
        "User",
        "id",
        "User",
        "id",
        "batch",
        "row",
        "cypher",
    )?;
    println!("openCypher Bulk Relationship Create Template:");
    println!("  Statement:  {}", rel_cypher.statement);
    println!("  Parameters: {:?}\n", rel_cypher.parameters);

    let rel_gql = compile_bulk_create_rel(
        "COLLABORATES_WITH",
        &["project", "hours"],
        "User",
        "id",
        "User",
        "id",
        "batch",
        "row",
        "iso_gql",
    )?;
    println!("ISO GQL Bulk Relationship Create Template:");
    println!("  Statement:  {}\n", rel_gql.statement);

    println!("[SUCCESS] Bulk ingestion template compilation demo completed successfully.");
    Ok(())
}
