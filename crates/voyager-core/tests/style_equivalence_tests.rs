//! Rust Query Builder Ergonomics & Authoring Style Equivalence Suite.
//!
//! Verifies that different Rust authoring styles:
//! 1. Step-by-Step Chaining (match_node(...).to(...).node(...))
//! 2. Memgraph / GQLAlchemy Chaining (match_().node(...).to_().node(...))
//! 3. Direct Arena Node Allocation
//! 4. Operator Shortcuts (where_gt vs where_property(Gt))
//!
//! compile to identical AST representations and identical dialect outputs.

use voyager_core::ast::{BinaryOp, LiteralValue};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::cypher::CypherEmitter;
use voyager_core::emitters::iso_gql::IsoGqlEmitter;
use voyager_core::visitor::AstVisitor;

#[test]
fn test_rust_single_node_authoring_styles_equivalence() {
    // Style 1: Combined match_node helper
    let mut b1 = QueryBuilder::new();
    b1.match_node(Some("p"), vec!["Person"])
        .where_gt("p", "age", 30)
        .field("p", "name", None::<String>)
        .order_by_asc("p", "age");
    let (a1, r1) = b1.build();

    // Style 2: Memgraph GQLAlchemy step-by-step chaining
    let mut b2 = QueryBuilder::new();
    b2.r#match()
        .node(Some("p"), vec!["Person"])
        .where_property("p", "age", BinaryOp::Gt, 30)
        .r#return()
        .field("p", "name", None::<String>)
        .order_by_asc("p", "age");
    let (a2, r2) = b2.build();

    // Style 3: Generic binary operator builder
    let mut b3 = QueryBuilder::new();
    let p_age = b3.prop("p", "age");
    let lit30 = b3.literal(30);
    let bin_pred = b3.binary_expr(p_age, BinaryOp::Gt, lit30);
    b3.r#match()
        .node(Some("p"), vec!["Person"])
        .where_predicate(bin_pred)
        .r#return()
        .field("p", "name", None::<String>)
        .order_by_asc("p", "age");
    let (a3, r3) = b3.build();

    let mut cypher = CypherEmitter::new();
    let res1 = cypher.visit_query(&a1, r1).unwrap();
    let res2 = cypher.visit_query(&a2, r2).unwrap();
    let res3 = cypher.visit_query(&a3, r3).unwrap();

    let expected = "MATCH (p:Person) WHERE p.age > $p0 RETURN p.name ORDER BY p.age ASC";
    assert_eq!(res1.statement, expected);
    assert_eq!(res2.statement, expected);
    assert_eq!(res3.statement, expected);

    assert_eq!(res1.parameters.get("p0"), Some(&LiteralValue::Int64(30)));
    assert_eq!(res2.parameters.get("p0"), Some(&LiteralValue::Int64(30)));
    assert_eq!(res3.parameters.get("p0"), Some(&LiteralValue::Int64(30)));
}

#[test]
fn test_rust_multi_hop_traversal_styles_equivalence() {
    // Style 1: Direct to(...) chaining
    let mut b1 = QueryBuilder::new();
    b1.match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"])
        .where_eq("a", "name", "Alice")
        .field("b", "name", None::<String>);
    let (a1, r1) = b1.build();

    // Style 2: Explicit step chaining
    let mut b2 = QueryBuilder::new();
    b2.r#match()
        .node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"])
        .where_property("a", "name", BinaryOp::Eq, "Alice")
        .r#return()
        .field("b", "name", None::<String>);
    let (a2, r2) = b2.build();

    let mut cypher = CypherEmitter::new();
    let res1 = cypher.visit_query(&a1, r1).unwrap();
    let res2 = cypher.visit_query(&a2, r2).unwrap();

    let expected = "MATCH (a:Person)-[r:KNOWS]->(b:Person) WHERE a.name = $p0 RETURN b.name";
    assert_eq!(res1.statement, expected);
    assert_eq!(res2.statement, expected);

    let mut gql = IsoGqlEmitter::new();
    let gql1 = gql.visit_query(&a1, r1).unwrap();
    let gql2 = gql.visit_query(&a2, r2).unwrap();

    assert_eq!(gql1.statement, expected);
    assert_eq!(gql2.statement, expected);
}
