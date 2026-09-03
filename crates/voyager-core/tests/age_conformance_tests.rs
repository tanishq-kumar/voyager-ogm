//! Formal Apache AGE (PostgreSQL Embedded Cypher) Conformance Test Suite.
//!
//! Ingests and validates Apache AGE regression scenarios:
//! 1. SELECT * FROM cypher('graph_name', $$ MATCH ... $$) AS (...) wrapper emission
//! 2. Automatic mapping of projections to PostgreSQL agtype
//! 3. Parameter isolation across the SQL boundary
//! 4. Directed, incoming, undirected, and variable-length traversals
//! 5. Full operator set (=, !=, >, >=, <, <=, IN, NOT IN, CONTAINS, STARTS WITH, ENDS WITH)
//! 6. Projections, aggregations, ordering, and DML mutation wrappers

use voyager_core::ast::{AggregationFunc, BinaryOp, LiteralValue};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::age::AgeEmitter;
use voyager_core::visitor::AstVisitor;

#[test]
fn test_age_single_node_and_projections() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .where_property("p", "age", BinaryOp::Gt, 30)
        .field("p", "name", None::<String>)
        .field("p", "age", None::<String>);

    let (arena, root) = builder.build();
    let mut age = AgeEmitter::new("social_graph");
    let res = age.visit_query(&arena, root).unwrap();

    let expected = "SELECT * FROM cypher('social_graph', $$ MATCH (p:Person) WHERE p.age > $p0 RETURN p.name, p.age $$, %s) AS (name agtype, age agtype)";
    assert_eq!(res.statement, expected);
    assert_eq!(res.parameters.get("p0"), Some(&LiteralValue::Int64(30)));
}

#[test]
fn test_age_all_operators_and_like_translations() {
    let ops = [
        (BinaryOp::Eq, "=", LiteralValue::Int64(30)),
        (BinaryOp::Neq, "!=", LiteralValue::Int64(30)),
        (BinaryOp::Gt, ">", LiteralValue::Int64(21)),
        (BinaryOp::Gte, ">=", LiteralValue::Int64(21)),
        (BinaryOp::Lt, "<", LiteralValue::Int64(65)),
        (BinaryOp::Lte, "<=", LiteralValue::Int64(65)),
        (
            BinaryOp::In,
            "IN",
            LiteralValue::List(vec![
                LiteralValue::String("London".into()),
                LiteralValue::String("Paris".into()),
            ]),
        ),
        (
            BinaryOp::NotIn,
            "NOT IN",
            LiteralValue::List(vec![LiteralValue::String("Rome".into())]),
        ),
        (
            BinaryOp::Contains,
            "CONTAINS",
            LiteralValue::String("don".into()),
        ),
        (
            BinaryOp::StartsWith,
            "STARTS WITH",
            LiteralValue::String("Al".into()),
        ),
        (
            BinaryOp::EndsWith,
            "ENDS WITH",
            LiteralValue::String("ce".into()),
        ),
    ];

    for (op, op_str, lit) in ops {
        let mut builder = QueryBuilder::new();
        builder
            .match_node(Some("p"), vec!["Person"])
            .where_property("p", "age", op, lit.clone())
            .field("p", "name", None::<String>);

        let (arena, root) = builder.build();
        let mut age = AgeEmitter::new("age_graph");
        let res = age.visit_query(&arena, root).unwrap();

        assert!(
            res.statement.contains(&format!("p.age {op_str} $p0")),
            "Failed for AGE op {op:?}: {}",
            res.statement
        );
        assert_eq!(res.parameters.get("p0"), Some(&lit));
    }
}

#[test]
fn test_age_directed_traversal_with_aliases() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"])
        .where_property("a", "name", BinaryOp::Eq, "Alice")
        .field("a", "name", Some("source".to_string()))
        .field("b", "name", Some("target".to_string()));

    let (arena, root) = builder.build();
    let mut age = AgeEmitter::new("age_graph");
    let res = age.visit_query(&arena, root).unwrap();

    let expected = "SELECT * FROM cypher('age_graph', $$ MATCH (a:Person)-[r:KNOWS]->(b:Person) WHERE a.name = $p0 RETURN a.name AS source, b.name AS target $$, %s) AS (source agtype, target agtype)";
    assert_eq!(res.statement, expected);
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::String("Alice".to_string()))
    );
}

#[test]
fn test_age_variable_length_and_undirected_traversals() {
    let mut b_hops = QueryBuilder::new();
    b_hops
        .match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], None::<String>)
        .hops(1, 3)
        .node(Some("b"), vec!["Person"])
        .field("b", "name", None::<String>);

    let (a_h, r_h) = b_hops.build();
    let mut age = AgeEmitter::new("g");
    let res_h = age.visit_query(&a_h, r_h).unwrap();
    assert!(res_h.statement.contains("[:KNOWS*1..3]"));

    let mut b_undir = QueryBuilder::new();
    b_undir
        .match_node(Some("a"), vec!["Person"])
        .edge(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"])
        .field("b", "name", None::<String>);

    let (a_u, r_u) = b_undir.build();
    let res_u = age.visit_query(&a_u, r_u).unwrap();
    assert!(res_u.statement.contains("(a:Person)-[r:KNOWS]-(b:Person)"));
}

#[test]
fn test_age_aggregations_and_grouping() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .field("p", "city", Some("city".to_string()))
        .select_property_aggregate("p", "name", AggregationFunc::Count, Some("cnt".to_string()))
        .select_property_aggregate(
            "p",
            "age",
            AggregationFunc::Avg,
            Some("avg_age".to_string()),
        )
        .select_property_aggregate(
            "p",
            "name",
            AggregationFunc::Collect,
            Some("names".to_string()),
        )
        .order_by_asc("p", "city");

    let (arena, root) = builder.build();
    let mut age = AgeEmitter::new("social_graph");
    let res = age.visit_query(&arena, root).unwrap();

    assert!(res.statement.contains("COUNT(p.name) AS cnt"));
    assert!(res.statement.contains("AVG(p.age) AS avg_age"));
    assert!(res.statement.contains("COLLECT(p.name) AS names"));
    assert!(
        res.statement
            .contains("AS (city agtype, cnt agtype, avg_age agtype, names agtype)")
    );
}

#[test]
fn test_age_mutation_wrappers() {
    let mut age = AgeEmitter::new("age_graph");

    // CREATE
    let mut b_create = QueryBuilder::new();
    b_create.create().node(Some("p"), vec!["Person"]);
    let (a_c, r_c) = b_create.build();
    let res_c = age.visit_query(&a_c, r_c).unwrap();
    assert_eq!(
        res_c.statement,
        "SELECT * FROM cypher('age_graph', $$ CREATE (p:Person) $$) AS (result agtype)"
    );

    // MERGE
    let mut b_merge = QueryBuilder::new();
    b_merge
        .merge()
        .node(Some("p"), vec!["Person"])
        .on_create_set("p", "name", "Eva");
    let (a_m, r_m) = b_merge.build();
    let res_m = age.visit_query(&a_m, r_m).unwrap();
    assert!(
        res_m
            .statement
            .contains("MERGE (p:Person) ON CREATE SET p.name = $p0")
    );
    assert_eq!(
        res_m.parameters.get("p0"),
        Some(&LiteralValue::String("Eva".into()))
    );
}
