//! Voyager OGM: Graph Mutation Builder (CREATE, MERGE, SET, DELETE)
//!
//! Teaches:
//! - Creating node and relationship patterns with `.create()`
//! - Upserting graph entities with `.merge()`, `.on_create_set()`, and `.on_match_set()`
//! - In-place property updates with `.set_property()` and `.set_merge()`
//! - Deleting entities with `.delete()` and cascading edge removal with `.detach_delete()`
//! - Multi-dialect compilation across openCypher and ISO/IEC 39075:2024 GQL
//!
//! Prerequisites:
//! - Engine: None (offline / compile-only)
//! - Crates: voyager-ogm
//!
//! Run with:
//! `cargo run -p voyager-ogm --example 09_mutation_builder`

use voyager_ogm::builder::QueryBuilder;
use voyager_ogm::emitters::{CypherEmitter, IsoGqlEmitter};
use voyager_ogm::visitor::AstVisitor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Voyager OGM (Rust) - Mutation Builder (CREATE, MERGE, SET, DELETE) ===\n");

    let mut cypher_emitter = CypherEmitter::new();
    let mut gql_emitter = IsoGqlEmitter::new();

    // -----------------------------------------------------------------------
    // 1. CREATE: Single Node & Path Patterns
    // -----------------------------------------------------------------------
    println!("--- [1] CREATE Single Node & Directed Path Pattern ---");

    // Single node creation
    let mut b_create_node = QueryBuilder::new();
    b_create_node
        .create()
        .node(Some("src"), vec!["Account"])
        .set_property("src", "acc_no", "ACC-100")
        .set_property("src", "holder", "Alice")
        .set_property("src", "balance", 5000.0);
    let (arena_node, root_node) = b_create_node.build();
    let compiled_create = cypher_emitter.visit_query(&arena_node, root_node)?;
    println!("CREATE Single Node (openCypher):");
    println!("  Statement:  {}", compiled_create.statement);
    println!("  Parameters: {:?}\n", compiled_create.parameters);

    // Path pattern creation: (src:Account)-[tx:TRANSFERRED_TO]->(dst:Account)
    let mut b_create_path = QueryBuilder::new();
    b_create_path
        .create()
        .node(Some("src"), vec!["Account"])
        .to(vec!["TRANSFERRED_TO"], Some("tx"))
        .node(Some("dst"), vec!["Account"])
        .set_property("tx", "amount", 750.0);
    let (arena_path, root_path) = b_create_path.build();
    let compiled_path = cypher_emitter.visit_query(&arena_path, root_path)?;
    println!("CREATE Relationship Path (openCypher):");
    println!("  Statement:  {}", compiled_path.statement);
    println!("  Parameters: {:?}\n", compiled_path.parameters);

    // -----------------------------------------------------------------------
    // 2. MERGE: Upsert Pattern with ON CREATE and ON MATCH
    // -----------------------------------------------------------------------
    println!("--- [2] MERGE Upsert Pattern ---");
    let mut b_merge = QueryBuilder::new();
    b_merge
        .merge()
        .node(Some("a"), vec!["Account"])
        .on_create_set("a", "holder", "Charlie")
        .on_create_set("a", "status", "PENDING")
        .on_create_set("a", "balance", 0.0)
        .on_match_set("a", "status", "ACTIVE");
    let (arena_merge, root_merge) = b_merge.build();
    let compiled_merge = cypher_emitter.visit_query(&arena_merge, root_merge)?;
    println!("MERGE Upsert Pattern (openCypher):");
    println!("  Statement:  {}", compiled_merge.statement);
    println!("  Parameters: {:?}\n", compiled_merge.parameters);

    // -----------------------------------------------------------------------
    // 3. SET: In-Place Property Updates on Matched Entities
    // -----------------------------------------------------------------------
    println!("--- [3] MATCH ... SET In-Place Property Updates ---");
    let mut b_set = QueryBuilder::new();
    b_set
        .r#match()
        .node(Some("tgt"), vec!["Account"])
        .where_eq("tgt", "acc_no", "ACC-100")
        .set_property("tgt", "balance", 5750.0)
        .set_property("tgt", "status", "VERIFIED")
        .r#return()
        .field("tgt", "acc_no", None::<&str>)
        .field("tgt", "balance", None::<&str>)
        .field("tgt", "status", None::<&str>);
    let (arena_set, root_set) = b_set.build();
    let compiled_set = cypher_emitter.visit_query(&arena_set, root_set)?;
    println!("MATCH ... SET Statement (openCypher):");
    println!("  Statement:  {}", compiled_set.statement);
    println!("  Parameters: {:?}\n", compiled_set.parameters);

    // -----------------------------------------------------------------------
    // 4. DELETE & DETACH DELETE (Cascade Removal)
    // -----------------------------------------------------------------------
    println!("--- [4] DELETE & DETACH DELETE ---");

    // Safe delete (isolated nodes or disconnected entities)
    let mut b_del = QueryBuilder::new();
    b_del
        .r#match()
        .node(Some("inact"), vec!["Account"])
        .where_eq("inact", "status", "INACTIVE")
        .delete(vec!["inact"]);
    let (arena_del, root_del) = b_del.build();
    let compiled_del = cypher_emitter.visit_query(&arena_del, root_del)?;
    println!("DELETE Statement (openCypher):");
    println!("  Statement:  {}\n", compiled_del.statement);

    // Cascade removal (removes node and all attached incident edges)
    let mut b_detach = QueryBuilder::new();
    b_detach
        .r#match()
        .node(Some("banned"), vec!["Account"])
        .where_eq("banned", "status", "BANNED")
        .detach_delete(vec!["banned"]);
    let (arena_detach, root_detach) = b_detach.build();
    let compiled_detach = cypher_emitter.visit_query(&arena_detach, root_detach)?;
    println!("DETACH DELETE Statement (openCypher):");
    println!("  Statement:  {}\n", compiled_detach.statement);

    // -----------------------------------------------------------------------
    // 5. ISO/IEC 39075:2024 GQL INSERT Compilation
    // -----------------------------------------------------------------------
    println!("--- [5] ISO/IEC 39075:2024 GQL Mutation ---");
    let compiled_gql_insert = gql_emitter.visit_query(&arena_node, root_node)?;
    println!("ISO GQL INSERT Statement:");
    println!("  Statement:  {}", compiled_gql_insert.statement);

    println!("\n[SUCCESS] Mutation builder demo completed successfully.");
    Ok(())
}
