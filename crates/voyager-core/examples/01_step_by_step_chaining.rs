//! Approach 1: Step-by-Step Path Chaining (Memgraph GQLAlchemy & ISO GQL Style)
//!
//! Run with: `cargo run --example 01_step_by_step_chaining`

use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

fn main() {
    println!("=== Voyager OGM (Rust) - Approach 1: Step-by-Step Path Chaining ===\n");

    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["ACTED_IN"], Some("r"))
        .hops(1, 2)
        .node(Some("m"), vec!["Movie"])
        .from(vec!["DIRECTED"], Some("d_rel"))
        .node(Some("d"), vec!["Director"])
        .where_gt("p", "age", 21)
        .where_eq("m", "released", 1999)
        .r#return()
        .field("p", "name", Some("actor"))
        .field("m", "title", Some("movie"))
        .field("d", "name", Some("director"))
        .order_by_asc("p", "name")
        .limit(10);

    let (arena, root) = builder.build();

    // 1. Compile to openCypher 9 / Cypher 25
    let mut cypher_emitter = CypherEmitter::new();
    let cypher = cypher_emitter.visit_query(&arena, root).unwrap();
    println!(
        " [openCypher 9 / Cypher 25]:\n  Statement:  {}\n  Parameters: {:?}\n",
        cypher.statement, cypher.parameters
    );

    // 2. Compile to SQL:2023 PGQ (GRAPH_TABLE)
    let mut pgq_emitter = SqlPgqEmitter::new("cinema_graph");
    let pgq = pgq_emitter.visit_query(&arena, root).unwrap();
    println!(
        " [SQL:2023 PGQ (GRAPH_TABLE)]:\n  Statement:  {}\n  Parameters: {:?}\n",
        pgq.statement, pgq.parameters
    );

    // 3. Compile to ISO/IEC 39075:2024 GQL Standard
    let mut gql_emitter = IsoGqlEmitter::new();
    let gql = gql_emitter.visit_query(&arena, root).unwrap();
    println!(
        " [ISO/IEC 39075:2024 GQL]:\n  Statement:  {}\n  Parameters: {:?}\n",
        gql.statement, gql.parameters
    );
}
