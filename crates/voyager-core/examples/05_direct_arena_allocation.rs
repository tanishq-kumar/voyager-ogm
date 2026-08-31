//! Approach 5: Low-Level Direct Memory Arena Allocation (`QueryAstArena`)
//!
//! Run with: `cargo run --example 05_direct_arena_allocation`

use voyager_core::ast::*;
use voyager_core::emitters::IsoGqlEmitter;
use voyager_core::visitor::AstVisitor;

fn main() {
    println!("=== Voyager OGM (Rust) - Approach 5: Low-Level 32-bit AST Arena Allocation ===\n");

    let mut arena = QueryAstArena::new();

    // 1. Allocate NodePattern in the 32-bit arena: (p:Person:Developer)
    let node_handle = arena.alloc(AstNode::NodePattern {
        variable: Some("p".into()),
        labels: vec!["Person".into(), "Developer".into()],
        predicates: vec![],
    });

    // 2. Allocate MatchClause
    let match_handle = arena.alloc(AstNode::MatchClause {
        optional: false,
        paths: vec![node_handle],
        where_clause: None,
    });

    // 3. Allocate ReturnClause: RETURN p.name AS dev_name
    let target_id = arena.alloc(AstNode::Identifier("p".into()));
    let prop_expr = arena.alloc(AstNode::PropertyAccess {
        target: target_id,
        property: "name".into(),
    });

    let return_handle = arena.alloc(AstNode::ReturnClause {
        distinct: false,
        projections: vec![ProjectionItem {
            expression: prop_expr,
            alias: Some("dev_name".into()),
            aggregation: None,
        }],
        order_by: vec![],
        skip: None,
        limit: Some(10),
    });

    // 4. Combine into root QueryStatement
    let root_handle = arena.alloc(AstNode::QueryStatement {
        matches: vec![match_handle],
        mutations: vec![],
        return_clause: Some(return_handle),
    });

    println!(" Total AST nodes in contiguous arena: {}", arena.len());

    let mut emitter = IsoGqlEmitter::new();
    let compiled = emitter.visit_query(&arena, root_handle).unwrap();

    println!(" Generated ISO GQL Statement:\n  {}", compiled.statement);
    println!(" Parameters:\n  {:?}", compiled.parameters);
}
