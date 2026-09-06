//! Comprehensive integration test suite for `AstOptimizer`.

use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::cypher::CypherEmitter;
use voyager_core::emitters::iso_gql::IsoGqlEmitter;
use voyager_core::optimizer::{AstOptimizer, OptimizationLevel};
use voyager_core::visitor::AstVisitor;

#[test]
fn test_optimizer_multi_hop_pushdown() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .to(vec!["ACTED_IN"], Some("r"))
        .node(Some("m"), vec!["Movie"])
        .where_eq("p", "name", "Keanu Reeves")
        .where_eq("m", "title", "The Matrix")
        .where_gt("m", "released", 1995)
        .field("p", "name", Some("actor"))
        .field("m", "title", Some("movie"));

    let (mut arena, root) = builder.build();

    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    // 1. Check openCypher emission
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    assert!(
        res.statement.contains("(p:Person {name: $p0})"),
        "Expected inlined person name, got: {}",
        res.statement
    );
    assert!(
        res.statement.contains("(m:Movie {title: $p1})"),
        "Expected inlined movie title, got: {}",
        res.statement
    );
    assert!(
        res.statement.contains("WHERE m.released > $p2"),
        "Expected remaining inequality in WHERE clause, got: {}",
        res.statement
    );

    // 2. Check ISO GQL emission
    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert!(
        res_gql.statement.contains("(p:Person {name: $p0})"),
        "Expected ISO GQL inlined person name, got: {}",
        res_gql.statement
    );
    assert!(
        res_gql.statement.contains("(m:Movie {title: $p1})"),
        "Expected ISO GQL inlined movie title, got: {}",
        res_gql.statement
    );
}

#[test]
fn test_optimizer_aggressive_dead_variable_pruning() {
    let mut builder = QueryBuilder::new();
    // Anonymous node with auto-generated alias `_dummy_0` that is never referenced
    builder
        .match_node(Some("_dummy_0"), vec!["Temporary"])
        .to(vec!["CONNECTED_TO"], None::<&str>)
        .node(Some("p"), vec!["Person"])
        .where_eq("p", "name", "Alice")
        .field("p", "name", Some("person_name"));

    let (mut arena, root) = builder.build();

    let optimizer = AstOptimizer::new(OptimizationLevel::Aggressive);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // `_dummy_0` should be pruned to an anonymous `(:Temporary)` node!
    assert!(
        res.statement.contains("(:Temporary)"),
        "Expected pruned anonymous node pattern, got: {}",
        res.statement
    );
    assert!(
        !res.statement.contains("_dummy_0"),
        "Expected _dummy_0 to be removed, got: {}",
        res.statement
    );
}
