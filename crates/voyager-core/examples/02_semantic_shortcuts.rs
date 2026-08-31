//! Approach 2: Semantic Traversal Shortcuts (`out_edge`, `in_edge`, `field`)
//!
//! Run with: `cargo run --example 02_semantic_shortcuts`

use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::CypherEmitter;
use voyager_core::visitor::AstVisitor;

fn main() {
    println!("=== Voyager OGM (Rust) - Approach 2: Semantic Traversal Shortcuts ===\n");

    let mut builder = QueryBuilder::new();
    builder
        .node_label("Person") // Shortcut for (:Person)
        .out_edge(vec!["KNOWS"], Some("r")) // Shortcut for -[r:KNOWS]->
        .node(Some("friend"), vec!["Person"])
        .where_contains("friend", "city", "London")
        .where_gte("friend", "age", 18)
        .field("friend", "name", Some("friend_name"))
        .field("friend", "city", Some("city"))
        .order_by_desc("friend", "age")
        .limit(25);

    let (arena, root) = builder.build();

    let mut emitter = CypherEmitter::new();
    let compiled = emitter.visit_query(&arena, root).unwrap();

    println!(" Generated Cypher Query:\n  {}", compiled.statement);
    println!(" Parameters:\n  {:?}", compiled.parameters);
}
