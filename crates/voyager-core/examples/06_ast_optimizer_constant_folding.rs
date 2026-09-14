//! Approach 6: AST Query Optimizer - Compile-Time Arithmetic Constant Folding & Predicate Pushdown
//!
//! Run with: `cargo run --example 06_ast_optimizer_constant_folding`

use voyager_core::ast::BinaryOp;
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::optimizer::{AstOptimizer, OptimizationLevel};
use voyager_core::visitor::AstVisitor;

fn main() {
    println!("=== Voyager OGM (Rust) - Compile-Time Constant Folding Optimizer ===\n");

    // Construct a query using QueryBuilder:
    // 1. Path: (u:User)-[p:PERFORMED]->(e:Event)
    // 2. Arithmetic Filter: e.duration_sec >= u.base_quota + (7 * 24 * 60 * 60)
    // 3. Hoistable Constant Equality: u.age == (10 + 15)
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("u"), vec!["User"])
        .to(vec!["PERFORMED"], Some("p"))
        .node(Some("e"), vec!["Event"]);

    // Build: 7 * 24 * 60 * 60 = 604,800
    let c7 = builder.literal(7i64);
    let c24 = builder.literal(24i64);
    let mul1 = builder.binary_expr(c7, BinaryOp::Mul, c24);
    let c60_a = builder.literal(60i64);
    let mul2 = builder.binary_expr(mul1, BinaryOp::Mul, c60_a);
    let c60_b = builder.literal(60i64);
    let retention_seconds = builder.binary_expr(mul2, BinaryOp::Mul, c60_b);

    // u.base_quota + 604800
    let base_quota = builder.prop("u", "base_quota");
    let quota_with_retention = builder.binary_expr(base_quota, BinaryOp::Add, retention_seconds);

    // e.duration_sec >= (u.base_quota + 604800)
    let duration = builder.prop("e", "duration_sec");
    let pred_duration = builder.binary_expr(duration, BinaryOp::Gte, quota_with_retention);

    // u.age == (10 + 15) -> will fold to 25 and hoist into (u:User {age: $p0})
    let age_prop = builder.prop("u", "age");
    let c10 = builder.literal(10i64);
    let c15 = builder.literal(15i64);
    let add_age = builder.binary_expr(c10, BinaryOp::Add, c15);
    let pred_age = builder.binary_expr(age_prop, BinaryOp::Eq, add_age);

    // Combined filter: pred_duration AND pred_age
    let combined_where = builder.binary_expr(pred_duration, BinaryOp::And, pred_age);
    builder.where_expr(combined_where);

    builder
        .field("u", "name", Some("user_name"))
        .field("e", "duration_sec", Some("duration"));

    let (mut arena, root) = builder.build();

    // --- 1. Unoptimized Cypher Emission ---
    let mut emitter = CypherEmitter::new();
    let unopt = emitter.visit_query(&arena, root).unwrap();
    println!("[1] Unoptimized Cypher Output:");
    println!("  Statement  : {}", unopt.statement);
    println!("  Parameters : {:?}\n", unopt.parameters);

    // --- 2. Run AST Optimizer (Pass 1: Constant Folding + Pass 2: Predicate Pushdown) ---
    let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
    let opt_root = optimizer.optimize(&mut arena, root).unwrap();

    let mut opt_emitter = CypherEmitter::new();
    let opt = opt_emitter.visit_query(&arena, opt_root).unwrap();
    println!("[2] Optimized Cypher Output (Constant Folded & Pushdown):");
    println!("  Statement  : {}", opt.statement);
    println!("  Parameters : {:?}", opt.parameters);
    println!("  -> Notice: (7 * 24 * 60 * 60) was folded to 604800 at compile time.");
    println!("  -> Notice: (10 + 15) was folded to 25 and hoisted into {{age: $p0}} pattern!\n");

    // --- 3. Multi-Dialect Optimized Emission ---
    let mut pgq_emitter = SqlPgqEmitter::new("events_graph");
    let pgq = pgq_emitter.visit_query(&arena, opt_root).unwrap();
    println!("[3] Optimized SQL:2023 PGQ (GRAPH_TABLE):");
    println!("  {}\n", pgq.statement);

    let mut gql_emitter = IsoGqlEmitter::new();
    let gql = gql_emitter.visit_query(&arena, opt_root).unwrap();
    println!("[4] Optimized ISO/IEC 39075:2024 GQL:");
    println!("  {}\n", gql.statement);
}
