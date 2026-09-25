//! Approach 7: Query Profiling and Plan Explanation (`explain` & `profile`)
//!
//! Run with: `cargo run --example 07_explain_and_profile_queries`

use voyager_core::ast::ExecutionMode;
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{AgeEmitter, CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

fn main() {
    println!("=== Voyager OGM (Rust) - Query Profiling & Plan Explanation ===\n");

    // 1. Build a base graph traversal query
    // MATCH (u:User)-[r:FRIENDS_WITH]->(f:User) WHERE u.active = true RETURN u.name, f.name
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("u"), vec!["User"])
        .to(vec!["FRIENDS_WITH"], Some("r"))
        .node(Some("f"), vec!["User"])
        .where_eq("u", "active", true)
        .field("u", "name", Some("user_name"))
        .field("f", "name", Some("friend_name"));

    // Clone builder to show distinct execution modes without mutating the base query
    let mut explain_builder = builder.clone();
    explain_builder.explain();
    assert_eq!(explain_builder.execution_mode(), ExecutionMode::Explain);

    let mut profile_builder = builder.clone();
    profile_builder.profile();
    assert_eq!(profile_builder.execution_mode(), ExecutionMode::Profile);

    // 2. Demonstrate multi-dialect plan explanation across all supported engines
    println!("--- Dialect Keyword Mappings for EXPLAIN ---");

    // openCypher (Neo4j, Memgraph) -> EXPLAIN ...
    let (arena_exp, root_exp) = explain_builder.build();
    let mut cypher_emitter = CypherEmitter::new();
    let cypher_exp = cypher_emitter.visit_query(&arena_exp, root_exp).unwrap();
    println!(
        "[openCypher (Neo4j / Memgraph)]:\n{}\n",
        cypher_exp.statement
    );

    // ISO GQL -> EXPLAIN ...
    let mut gql_emitter = IsoGqlEmitter::new();
    let gql_exp = gql_emitter.visit_query(&arena_exp, root_exp).unwrap();
    println!("[ISO/IEC GQL]:\n{}\n", gql_exp.statement);

    // SQL:2023 PGQ (PostgreSQL, DuckDB) -> EXPLAIN ...
    let mut pgq_emitter = SqlPgqEmitter::new("social_network");
    let pgq_exp = pgq_emitter.visit_query(&arena_exp, root_exp).unwrap();
    println!("[SQL:2023 PGQ]:\n{}\n", pgq_exp.statement);

    // Apache AGE -> EXPLAIN SELECT ... (inner Cypher in $$ ... $$ un-prefixed)
    let mut age_emitter = AgeEmitter::new("social_network");
    let age_exp = age_emitter.visit_query(&arena_exp, root_exp).unwrap();
    println!(
        "[Apache AGE (PostgreSQL Extension)]:\n{}\n",
        age_exp.statement
    );

    // 3. Demonstrate multi-dialect profiling (runtime metrics)
    println!("--- Dialect Keyword Mappings for PROFILE ---");

    let (arena_prof, root_prof) = profile_builder.build();

    let cypher_prof = cypher_emitter.visit_query(&arena_prof, root_prof).unwrap();
    println!("[openCypher (PROFILE)]:\n{}\n", cypher_prof.statement);

    let gql_prof = gql_emitter.visit_query(&arena_prof, root_prof).unwrap();
    println!("[ISO/IEC GQL (PROFILE)]:\n{}\n", gql_prof.statement);

    let pgq_prof = pgq_emitter.visit_query(&arena_prof, root_prof).unwrap();
    println!(
        "[SQL:2023 PGQ (EXPLAIN ANALYZE)]:\n{}\n",
        pgq_prof.statement
    );

    let age_prof = age_emitter.visit_query(&arena_prof, root_prof).unwrap();
    println!("[Apache AGE (EXPLAIN ANALYZE)]:\n{}\n", age_prof.statement);

    // 4. Combined Composable Chaining (.explain().profile())
    let mut combined_builder = builder.clone();
    combined_builder.explain().profile();
    assert_eq!(
        combined_builder.execution_mode(),
        ExecutionMode::ExplainAndProfile
    );
    println!("--- Composable Combined Mode ---");
    println!("Execution Mode: {}", combined_builder.execution_mode());
}
