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

#[test]
fn test_optimizer_constant_folding_integer_arithmetic() {
    use voyager_core::ast::{BinaryOp, LiteralValue};

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    // p.age == (10 + 20)
    let p_age = builder.prop("p", "age");
    let c10 = builder.literal(10i64);
    let c20 = builder.literal(20i64);
    let sum_expr = builder.binary_expr(c10, BinaryOp::Add, c20);
    let eq_expr = builder.binary_expr(p_age, BinaryOp::Eq, sum_expr);
    builder.where_expr(eq_expr);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // Constant folding evaluates 10 + 20 -> 30, allowing predicate pushdown to hoist {age: $p0} where $p0 = 30!
    assert!(
        res.statement.contains("(p:Person {age: $p0})"),
        "Expected hoisted {{age: $p0}}, got: {}",
        res.statement
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::Int64(30)),
        "Expected folded parameter 30"
    );
    assert!(
        !res.statement.contains("WHERE"),
        "WHERE clause should be eliminated after hoisting, got: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_constant_folding_division_by_zero_and_overflow_safety() {
    use voyager_core::ast::BinaryOp;

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    // 1. Division by zero: p.val == 10 / 0 (must NOT panic or fold)
    let p_val = builder.prop("p", "val");
    let c10 = builder.literal(10i64);
    let c0 = builder.literal(0i64);
    let div_zero = builder.binary_expr(c10, BinaryOp::Div, c0);
    let eq_div = builder.binary_expr(p_val, BinaryOp::Eq, div_zero);

    // 2. Integer overflow: p.big == i64::MAX + 1 (must NOT panic or fold)
    let p_big = builder.prop("p", "big");
    let max_i64 = builder.literal(i64::MAX);
    let c1 = builder.literal(1i64);
    let overflow_expr = builder.binary_expr(max_i64, BinaryOp::Add, c1);
    let eq_overflow = builder.binary_expr(p_big, BinaryOp::Eq, overflow_expr);

    let and_cond = builder.binary_expr(eq_div, BinaryOp::And, eq_overflow);
    builder.where_expr(and_cond);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    // Optimizer execution MUST NOT panic
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // The division by zero and overflow expressions remain safely preserved in WHERE
    assert!(
        res.statement.contains("WHERE"),
        "Unfoldable safe expressions must remain in WHERE: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_constant_folding_floats_and_type_promotion() {
    use voyager_core::ast::{BinaryOp, LiteralValue};

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    // p.rate == 10 + 2.5 (i64 promoted to f64 -> 12.5)
    let p_rate = builder.prop("p", "rate");
    let c10 = builder.literal(10i64);
    let c2_5 = builder.literal(2.5f64);
    let add_expr = builder.binary_expr(c10, BinaryOp::Add, c2_5);
    let eq_expr = builder.binary_expr(p_rate, BinaryOp::Eq, add_expr);
    builder.where_expr(eq_expr);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    assert!(
        res.statement.contains("(p:Person {rate: $p0})"),
        "Expected hoisted {{rate: $p0}}, got: {}",
        res.statement
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::Float64(12.5)),
        "Expected promoted float parameter 12.5"
    );
}

#[test]
fn test_optimizer_constant_folding_relational_and_boolean_laws() {
    use voyager_core::ast::BinaryOp;

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    // p.age > 18 AND (10 > 5) -> p.age > 18 AND true -> p.age > 18
    let p_age = builder.prop("p", "age");
    let c18 = builder.literal(18i64);
    let age_gt_18 = builder.binary_expr(p_age, BinaryOp::Gt, c18);

    let c10 = builder.literal(10i64);
    let c5 = builder.literal(5i64);
    let ten_gt_five = builder.binary_expr(c10, BinaryOp::Gt, c5);

    let and_expr = builder.binary_expr(age_gt_18, BinaryOp::And, ten_gt_five);
    builder.where_expr(and_expr);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // Constant 10 > 5 folded to true, then (expr AND true) simplified to expr
    assert!(
        res.statement.contains("WHERE p.age > $p0"),
        "Expected simplified WHERE p.age > $p0, got: {}",
        res.statement
    );
    assert!(
        !res.statement.contains("AND"),
        "Expected AND clause to be completely eliminated, got: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_constant_folding_unary_and_double_negation() {
    use voyager_core::ast::{BinaryOp, LiteralValue, UnaryOp};

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    // p.balance == -(-50) -> 50
    let p_balance = builder.prop("p", "balance");
    let neg_50 = builder.literal(-50i64);
    let double_neg = builder.unary_expr(UnaryOp::Neg, neg_50);
    let eq_expr = builder.binary_expr(p_balance, BinaryOp::Eq, double_neg);
    builder.where_expr(eq_expr);
    builder.field("p", "balance", Some("balance"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    assert!(
        res.statement.contains("(p:Person {balance: $p0})"),
        "Expected hoisted balance, got: {}",
        res.statement
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::Int64(50)),
        "Expected folded unary negation to produce 50"
    );
}

#[test]
fn test_optimizer_constant_folding_partial_reassociation() {
    use voyager_core::ast::BinaryOp;

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    // WHERE (p.age + 10) + 20 > 50 -> p.age + 30 > 50
    let p_age = builder.prop("p", "age");
    let c10 = builder.literal(10i64);
    let inner_add = builder.binary_expr(p_age, BinaryOp::Add, c10);
    let c20 = builder.literal(20i64);
    let outer_add = builder.binary_expr(inner_add, BinaryOp::Add, c20);
    let c50 = builder.literal(50i64);
    let gt_expr = builder.binary_expr(outer_add, BinaryOp::Gt, c50);

    builder.where_expr(gt_expr);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // Partial reassociation evaluates 10 + 20 -> 30, resulting in (p.age + 30) > 50
    assert!(
        res.statement.contains("(p.age + $p0) > $p1"),
        "Expected reassociated expression (p.age + $p0) > $p1, got: {}",
        res.statement
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&voyager_core::ast::LiteralValue::Int64(30)),
        "Expected folded reassociated constant 30"
    );
}

#[test]
fn test_optimizer_constant_folding_multi_dialect_parity() {
    use voyager_core::ast::{BinaryOp, LiteralValue};
    use voyager_core::emitters::sql_pgq::SqlPgqEmitter;

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    // p.age > (7 * 3) -> p.age > 21
    let p_age = builder.prop("p", "age");
    let c7 = builder.literal(7i64);
    let c3 = builder.literal(3i64);
    let mul_expr = builder.binary_expr(c7, BinaryOp::Mul, c3);
    let gt_expr = builder.binary_expr(p_age, BinaryOp::Gt, mul_expr);
    builder.where_expr(gt_expr);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    // 1. openCypher
    let mut cypher = CypherEmitter::new();
    let res_cypher = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_cypher.parameters.get("p0"),
        Some(&LiteralValue::Int64(21))
    );
    assert!(res_cypher.statement.contains("WHERE p.age > $p0"));

    // 2. ISO GQL
    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(res_gql.parameters.get("p0"), Some(&LiteralValue::Int64(21)));
    assert!(res_gql.statement.contains("WHERE p.age > $p0"));

    // 3. SQL:2023 PGQ
    let mut pgq = SqlPgqEmitter::new("social_network");
    let res_pgq = pgq.visit_query(&arena, root).unwrap();
    assert_eq!(res_pgq.parameters.get("p0"), Some(&LiteralValue::Int64(21)));
    assert!(res_pgq.statement.contains("WHERE p.age > $p0"));
}

#[test]
fn test_optimizer_constant_folding_null_safety_multiplication_by_zero() {
    use voyager_core::ast::BinaryOp;

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    // p.val * 0 == 0 must NOT be folded to 0 == 0, because if p.val is NULL,
    // NULL * 0 is NULL (falsy in WHERE). Folding to 0 == 0 would erroneously match NULL nodes.
    let p_val = builder.prop("p", "val");
    let c0_a = builder.literal(0i64);
    let mul_expr = builder.binary_expr(p_val, BinaryOp::Mul, c0_a);
    let c0_b = builder.literal(0i64);
    let eq_expr = builder.binary_expr(mul_expr, BinaryOp::Eq, c0_b);
    builder.where_expr(eq_expr);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // WHERE clause MUST be preserved to evaluate runtime NULL semantics
    assert!(
        res.statement.contains("WHERE"),
        "WHERE clause must NOT be eliminated for p.val * 0: {}",
        res.statement
    );
    assert!(
        res.statement.contains("p.val * $p0") || res.statement.contains("(p.val * $p0)"),
        "Multiplication by zero with dynamic operand must remain intact: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_sql_pgq_hoisted_predicate_preserved() {
    use voyager_core::ast::{BinaryOp, LiteralValue};
    use voyager_core::emitters::sql_pgq::SqlPgqEmitter;

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("p"), vec!["Person"]);

    let p_name = builder.prop("p", "name");
    let c_alice = builder.literal("Alice");
    let eq_expr = builder.binary_expr(p_name, BinaryOp::Eq, c_alice);
    builder.where_expr(eq_expr);
    builder.field("p", "name", Some("name"));

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut pgq = SqlPgqEmitter::new("social_network");
    let res_pgq = pgq.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_pgq.parameters.get("p0"),
        Some(&LiteralValue::String("Alice".to_string()))
    );
    assert!(
        res_pgq
            .statement
            .contains("(p IS Person WHERE p.name = $p0)"),
        "Hoisted predicate must be preserved in SqlPgqEmitter: {}",
        res_pgq.statement
    );
}

#[test]
fn test_optimizer_linear_clauses_constant_folding_and_variable_tracking() {
    use voyager_core::ast::BinaryOp;
    use voyager_core::emitters::iso_gql::IsoGqlEmitter;

    let mut builder = QueryBuilder::new();
    builder.match_node(Some("_n0"), vec!["Person"]);

    // Constant folding in LET: 10 + 20 -> 30
    let l10 = builder.literal(10i64);
    let l20 = builder.literal(20i64);
    let sum_expr = builder.binary_expr(l10, BinaryOp::Add, l20);
    builder.let_("total", sum_expr);

    // Constant folding and variable tracking in FILTER: _n0.age > (5 * 4) -> _n0.age > 20
    let n_age = builder.prop("_n0", "age");
    let l5 = builder.literal(5i64);
    let l4 = builder.literal(4i64);
    let mult_expr = builder.binary_expr(l5, BinaryOp::Mul, l4);
    let filter_expr = builder.binary_expr(n_age, BinaryOp::Gt, mult_expr);
    builder.filter_(filter_expr);

    let total_ident = builder.ident("total");
    builder.select_expr(total_ident, None::<&str>);

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Aggressive);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut gql = IsoGqlEmitter::new();
    let res = gql.visit_query(&arena, root).unwrap();

    // Constant folding verified: $p0 is 30, $p1 is 20
    assert_eq!(
        res.parameters.get("p0"),
        Some(&voyager_core::ast::LiteralValue::Int64(30))
    );
    assert_eq!(
        res.parameters.get("p1"),
        Some(&voyager_core::ast::LiteralValue::Int64(20))
    );

    // Variable tracking verified: _n0 referenced in FILTER must NOT be pruned!
    assert!(
        res.statement.contains("MATCH (_n0:Person)"),
        "Expected _n0 to be preserved because it is used in FILTER, got: {}",
        res.statement
    );
    assert!(
        res.statement.contains("LET total = $p0"),
        "Expected folded LET expression, got: {}",
        res.statement
    );
    assert!(
        res.statement.contains("FILTER _n0.age > $p1"),
        "Expected folded FILTER predicate, got: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_mutations_create_merge_variable_tracking() {
    let mut builder = QueryBuilder::new();
    // MATCH (_n0:Person)
    builder.match_node(Some("_n0"), vec!["Person"]);
    // CREATE (p:Company)-[:CONNECTED_TO]->(_n0)
    builder.create();
    builder
        .node(Some("c"), vec!["Company"])
        .to(vec!["CONNECTED_TO"], None::<&str>)
        .node(Some("_n0"), Vec::<&str>::new());

    let (mut arena, root) = builder.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Aggressive);
    optimizer.optimize(&mut arena, root).unwrap();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // _n0 was matched and referenced in CREATE: it must NOT be pruned to an anonymous node!
    assert!(
        res.statement.contains("MATCH (_n0:Person)"),
        "Expected _n0 to be preserved in MATCH because it is used in CREATE, got: {}",
        res.statement
    );
    assert!(
        res.statement
            .contains("CREATE (c:Company)-[:CONNECTED_TO]->(_n0)"),
        "Expected valid CREATE referencing _n0, got: {}",
        res.statement
    );
}

#[test]
fn test_optimizer_path_variable_aggressive_pruning() {
    use voyager_core::ast::PathMode;
    use voyager_core::emitters::iso_gql::IsoGqlEmitter;

    // Case 1: Synthetic path variable _p0 is UNREFERENCED -> must be pruned!
    let mut b1 = QueryBuilder::new();
    b1.match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS"], None::<&str>)
        .node(Some("b"), vec!["Person"])
        .path_mode(PathMode::Trail)
        .path_variable("_p0")
        .field("a", "name", None::<&str>);

    let (mut arena1, root1) = b1.build();
    let optimizer = AstOptimizer::new(OptimizationLevel::Aggressive);
    optimizer.optimize(&mut arena1, root1).unwrap();

    let mut gql1 = IsoGqlEmitter::new();
    let res1 = gql1.visit_query(&arena1, root1).unwrap();
    assert!(
        !res1.statement.contains("_p0 ="),
        "Expected unreferenced _p0 to be pruned, got: {}",
        res1.statement
    );
    assert!(
        res1.statement
            .contains("MATCH TRAIL (a:Person)-[:KNOWS]->(b:Person)"),
        "Expected TRAIL keyword preserved without unreferenced variable, got: {}",
        res1.statement
    );

    // Case 2: Synthetic path variable _p0 is REFERENCED -> must NOT be pruned!
    let mut b2 = QueryBuilder::new();
    b2.match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS"], None::<&str>)
        .node(Some("b"), vec!["Person"])
        .path_mode(PathMode::Trail)
        .path_variable("_p0");

    let p_ident = b2.ident("_p0");
    let fn_call = b2.function("path_length", vec![p_ident]);
    b2.let_("len_p", fn_call);
    let len_ident = b2.ident("len_p");
    b2.select_expr(len_ident, None::<&str>);

    let (mut arena2, root2) = b2.build();
    optimizer.optimize(&mut arena2, root2).unwrap();

    let mut gql2 = IsoGqlEmitter::new();
    let res2 = gql2.visit_query(&arena2, root2).unwrap();
    assert!(
        res2.statement
            .contains("MATCH _p0 = TRAIL (a:Person)-[:KNOWS]->(b:Person)"),
        "Expected referenced _p0 to be preserved, got: {}",
        res2.statement
    );
    assert!(
        res2.statement.contains("LET len_p = path_length(_p0)"),
        "Expected LET referencing _p0, got: {}",
        res2.statement
    );
}
