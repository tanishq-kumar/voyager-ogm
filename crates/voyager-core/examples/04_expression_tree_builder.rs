//! Approach 4: Custom Predicate Expression Trees (Nested AND / OR / XOR Logic)
//!
//! Run with: `cargo run --example 04_expression_tree_builder`

use voyager_core::ast::BinaryOp;
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::CypherEmitter;
use voyager_core::visitor::AstVisitor;

fn main() {
    println!("=== Voyager OGM (Rust) - Approach 4: Custom Expression Trees ===\n");

    let mut builder = QueryBuilder::new();

    // Goal: Construct nested condition: (p.age > 18) AND (p.city = 'NY' OR p.city = 'London')
    let prop_age = builder.prop("p", "age");
    let lit_18 = builder.literal(18);
    let cond_age = builder.binary_expr(prop_age, BinaryOp::Gt, lit_18);

    let prop_city1 = builder.prop("p", "city");
    let lit_ny = builder.literal("NY");
    let cond_ny = builder.binary_expr(prop_city1, BinaryOp::Eq, lit_ny);

    let prop_city2 = builder.prop("p", "city");
    let lit_ldn = builder.literal("London");
    let cond_ldn = builder.binary_expr(prop_city2, BinaryOp::Eq, lit_ldn);

    let cond_city_or = builder.binary_expr(cond_ny, BinaryOp::Or, cond_ldn);
    let full_predicate = builder.binary_expr(cond_age, BinaryOp::And, cond_city_or);

    builder
        .node(Some("p"), vec!["Person"])
        .filter(full_predicate)
        .field("p", "name", Some("name"))
        .field("p", "city", Some("city"));

    let (arena, root) = builder.build();

    let mut emitter = CypherEmitter::new();
    let compiled = emitter.visit_query(&arena, root).unwrap();

    println!(
        " Generated Cypher Query with Nested Predicates:\n  {}",
        compiled.statement
    );
    println!(" Extracted Parameter Map:\n  {:?}", compiled.parameters);
}
