//! Approach 3: Combined Edge & Target Node Pattern (`to_edge`)
//!
//! Run with: `cargo run --example 03_single_call_pattern`

use voyager_core::ast::Direction;
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::SqlPgqEmitter;
use voyager_core::visitor::AstVisitor;

fn main() {
    println!("=== Voyager OGM (Rust) - Approach 3: Combined 1-Hop Pattern ===\n");

    let mut builder = QueryBuilder::new();
    builder
        .node(Some("u"), vec!["User"])
        .to_edge(
            Direction::Outgoing,
            vec!["FOLLOWS"],
            Some("f"),
            Some("target"),
            vec!["User"],
        )
        .where_eq("u", "name", "Alice")
        .where_eq("target", "verified", true)
        .field("target", "name", Some("followed_user"))
        .limit(10);

    let (arena, root) = builder.build();

    let mut emitter = SqlPgqEmitter::new("social_graph");
    let compiled = emitter.visit_query(&arena, root).unwrap();

    println!(" Generated SQL:2023 PGQ Query:\n  {}", compiled.statement);
    println!(" Parameters:\n  {:?}", compiled.parameters);
}
