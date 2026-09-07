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

#[test]
fn test_optimizer_branching_patterns_pushdown() {
    let mut builder = QueryBuilder::new();
    // MATCH (p:Person)-[:ACTED_IN]->(m:Movie), (p:Person)-[:DIRECTED]->(d:Movie)
    // WHERE p.city = 'London' AND m.released = 1999 AND d.genre = 'Sci-Fi' AND p.age > 30
    builder
        .match_node(Some("p"), vec!["Person"])
        .to(vec!["ACTED_IN"], Some("r1"))
        .node(Some("m"), vec!["Movie"])
        .match_node(Some("p"), vec!["Person"])
        .to(vec!["DIRECTED"], Some("r2"))
        .node(Some("d"), vec!["Movie"])
        .where_eq("p", "city", "London")
        .where_eq("m", "released", 1999)
        .where_eq("d", "genre", "Sci-Fi")
        .where_gt("p", "age", 30)
        .field("p", "name", Some("person"))
        .field("m", "title", Some("movie"))
        .field("d", "title", Some("directed_movie"));

    let (mut arena, root) = builder.build();

    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // Verify property equalities are hoisted into their respective node patterns across branches
    assert!(
        res.statement.contains("city: $"),
        "Expected p.city hoisted, got: {}",
        res.statement
    );
    assert!(
        res.statement.contains("(m:Movie {released: $"),
        "Expected m.released hoisted, got: {}",
        res.statement
    );
    assert!(
        res.statement.contains("(d:Movie {genre: $"),
        "Expected d.genre hoisted, got: {}",
        res.statement
    );
    // Range inequality remains in WHERE
    assert!(
        res.statement.contains("WHERE p.age > $"),
        "Expected p.age > $ in WHERE, got: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_rich_expressions_and_functions_safety() {
    use voyager_core::ast::BinaryOp;

    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .to(vec!["WORKS_AT"], Some("w"))
        .node(Some("c"), vec!["Company"])
        .where_eq("c", "name", "Acme Corp");

    // Add function condition: toUpper(p.name) = 'ALICE'
    let p_name = builder.prop("p", "name");
    let fn_call = builder.function("toUpper", vec![p_name]);
    let alice_lit = builder.literal("ALICE");
    let fn_eq = builder.binary_expr(fn_call, BinaryOp::Eq, alice_lit);
    builder.where_expr(fn_eq);

    // Add arithmetic condition: (p.age + 5) >= 30
    let p_age = builder.prop("p", "age");
    let five_lit = builder.literal(5);
    let age_plus_5 = builder.math_expr(p_age, BinaryOp::Add, five_lit);
    let thirty_lit = builder.literal(30);
    let arith_gte = builder.binary_expr(age_plus_5, BinaryOp::Gte, thirty_lit);
    builder.where_expr(arith_gte);

    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();

    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // 1. Company name should be hoisted into pattern property map
    assert!(
        res.statement.contains("(c:Company {name: $p0})"),
        "Expected c.name hoisted, got: {}",
        res.statement
    );

    // 2. Function condition (toUpper) MUST NOT be hoisted into property map (would be invalid Cypher)
    assert!(
        !res.statement.contains("{name: toUpper"),
        "Function call was improperly hoisted into property map: {}",
        res.statement
    );
    assert!(
        res.statement.contains("toUpper(p.name) = $p1"),
        "Expected function predicate in WHERE clause, got: {}",
        res.statement
    );

    // 3. Arithmetic condition MUST NOT be hoisted into property map
    assert!(
        res.statement.contains("(p.age + $p2) >= $p3")
            || res.statement.contains("p.age + $p2 >= $p3"),
        "Expected arithmetic predicate in WHERE clause, got: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_anti_optimizations_and_edge_cases() {
    use voyager_core::ast::BinaryOp;

    // Case 1: Cross-variable equality: p1.city = p2.city
    // MUST NOT be hoisted into (p1:Person {city: p2.city}) because property maps only take constants/params!
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p1"), vec!["Person"])
        .match_node(Some("p2"), vec!["Person"]);

    let p1_city = builder.prop("p1", "city");
    let p2_city = builder.prop("p2", "city");
    let cross_eq = builder.binary_expr(p1_city, BinaryOp::Eq, p2_city);
    builder.where_expr(cross_eq);
    builder.where_eq("p1", "country", "UK");
    builder.field("p1", "name", Some("name1"));
    builder.field("p2", "name", Some("name2"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // p1.country is a constant and should be hoisted
    assert!(
        res.statement.contains("(p1:Person {country: $p0})"),
        "Expected p1.country hoisted, got: {}",
        res.statement
    );

    // Cross-variable equality MUST remain in WHERE clause
    assert!(
        res.statement.contains("WHERE p1.city = p2.city"),
        "Expected cross-variable equality preserved in WHERE clause, got: {}",
        res.statement
    );
    assert!(
        !res.statement.contains("{city: p2.city}"),
        "Cross-variable equality was improperly hoisted into property pattern: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_functions_over_parameters_hoisting() {
    use voyager_core::ast::BinaryOp;

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["User"]);

    // p.upper_name = toUpper('alice')
    let p_upper = builder.prop("p", "upper_name");
    let alice_lit = builder.literal("alice");
    let to_upper_alice = builder.function("toUpper", vec![alice_lit]);
    let eq_fn = builder.binary_expr(p_upper, BinaryOp::Eq, to_upper_alice);

    builder.where_expr(eq_fn);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // The function over a literal/parameter is hoisted into the pattern property map!
    assert!(
        res.statement
            .contains("(p:User {upper_name: toUpper($p0)})"),
        "Expected function over parameter hoisted into pattern map, got: {}",
        res.statement
    );
    assert!(
        !res.statement.contains("WHERE"),
        "Expected WHERE clause to be eliminated after hoisting, got: {}",
        res.statement
    );
}
