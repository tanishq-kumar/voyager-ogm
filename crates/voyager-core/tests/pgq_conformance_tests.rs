//! Formal ISO/IEC 9075-16:2023 SQL:PGQ & DuckPGQ Conformance Test Suite.
//!
//! Ingests and validates DuckPGQ & SQL:2023 PGQ scenarios:
//! 1. GRAPH_TABLE syntax generation with IS Label predicates
//! 2. Multi-hop and bounded repetition paths (-[IS KNOWS]{min, max}->)
//! 3. Incoming (<-[...]-) and undirected (-[...] -) traversals
//! 4. Full operator set (=, !=, >, >=, <, <=, IN, NOT IN, LIKE translations for CONTAINS, STARTS WITH, ENDS WITH)
//! 5. Aggregations in COLUMNS: COUNT, AVG, SUM, MIN, MAX, ARRAY_AGG
//! 6. Parameter isolation and ORDER BY / LIMIT / OFFSET pagination

use voyager_core::ast::{AggregationFunc, BinaryOp, LiteralValue};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::sql_pgq::SqlPgqEmitter;
use voyager_core::visitor::AstVisitor;

#[test]
fn test_pgq_single_node_and_where_conformance() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .where_property("p", "age", BinaryOp::Gt, 30)
        .field("p", "name", Some("name".to_string()))
        .field("p", "age", Some("age".to_string()))
        .order_by_asc("p", "age");

    let (arena, root) = builder.build();
    let mut pgq = SqlPgqEmitter::new("social_graph");
    let res = pgq.visit_query(&arena, root).unwrap();

    assert!(
        res.statement
            .contains("SELECT * FROM GRAPH_TABLE (social_graph MATCH (p IS Person)")
    );
    assert!(res.statement.contains("WHERE p.age > $p0"));
    assert!(
        res.statement
            .contains("COLUMNS (p.name AS name, p.age AS age)")
    );
    assert!(res.statement.contains("ORDER BY p.age ASC"));
    assert_eq!(res.parameters.get("p0"), Some(&LiteralValue::Int64(30)));
}

#[test]
fn test_pgq_all_operator_translations() {
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
    ];

    for (op, op_str, lit) in ops {
        let mut builder = QueryBuilder::new();
        builder
            .match_node(Some("p"), vec!["Person"])
            .where_property("p", "age", op, lit.clone())
            .field("p", "name", None::<String>);

        let (arena, root) = builder.build();
        let mut pgq = SqlPgqEmitter::new("g");
        let res = pgq.visit_query(&arena, root).unwrap();

        assert!(
            res.statement.contains(&format!("p.age {op_str} $p0")),
            "Failed for op {op:?}: {}",
            res.statement
        );
        assert_eq!(res.parameters.get("p0"), Some(&lit));
    }
}

#[test]
fn test_pgq_string_like_translations() {
    // CONTAINS
    let mut b1 = QueryBuilder::new();
    b1.match_node(Some("p"), vec!["Person"])
        .where_contains("p", "city", "don")
        .field("p", "name", None::<String>);
    let (a1, r1) = b1.build();
    let mut pgq = SqlPgqEmitter::new("g");
    let res1 = pgq.visit_query(&a1, r1).unwrap();
    assert!(res1.statement.contains("p.city LIKE '%' || $p0 || '%'"));

    // STARTS WITH
    let mut b2 = QueryBuilder::new();
    b2.match_node(Some("p"), vec!["Person"])
        .where_property("p", "name", BinaryOp::StartsWith, "Al")
        .field("p", "name", None::<String>);
    let (a2, r2) = b2.build();
    let res2 = pgq.visit_query(&a2, r2).unwrap();
    assert!(res2.statement.contains("p.name LIKE $p0 || '%'"));

    // ENDS WITH
    let mut b3 = QueryBuilder::new();
    b3.match_node(Some("p"), vec!["Person"])
        .where_property("p", "name", BinaryOp::EndsWith, "ce")
        .field("p", "name", None::<String>);
    let (a3, r3) = b3.build();
    let res3 = pgq.visit_query(&a3, r3).unwrap();
    assert!(res3.statement.contains("p.name LIKE '%' || $p0"));
}

#[test]
fn test_pgq_multi_hop_and_quantified_hops() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], Some("r1".to_string()))
        .node(Some("b"), vec!["Person"])
        .to(vec!["WORKS_AT".to_string()], Some("r2".to_string()))
        .node(Some("c"), vec!["Company"])
        .field("a", "name", Some("person".to_string()))
        .field("b", "name", Some("colleague".to_string()))
        .field("c", "name", Some("company".to_string()));

    let (arena, root) = builder.build();
    let mut pgq = SqlPgqEmitter::new("corp_graph");
    let res = pgq.visit_query(&arena, root).unwrap();

    assert!(res.statement.contains(
        "(a IS Person) -[r1 IS KNOWS]-> (b IS Person) -[r2 IS WORKS_AT]-> (c IS Company)"
    ));

    // Variable hops
    let mut b_hops = QueryBuilder::new();
    b_hops
        .match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], Some("r".to_string()))
        .hops(1, 3)
        .node(Some("b"), vec!["Person"])
        .field("b", "name", None::<String>);

    let (a_h, r_h) = b_hops.build();
    let res_h = pgq.visit_query(&a_h, r_h).unwrap();
    assert!(res_h.statement.contains("-[r IS KNOWS]{1,3}->"));
}

#[test]
fn test_pgq_directions_undirected_and_incoming() {
    let mut b_undir = QueryBuilder::new();
    b_undir
        .match_node(Some("a"), vec!["Person"])
        .edge(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"])
        .field("b", "name", None::<String>);

    let (a_u, r_u) = b_undir.build();
    let mut pgq = SqlPgqEmitter::new("g");
    let res_u = pgq.visit_query(&a_u, r_u).unwrap();
    assert!(
        res_u
            .statement
            .contains("(a IS Person) -[r IS KNOWS]- (b IS Person)")
    );

    let mut b_inc = QueryBuilder::new();
    b_inc
        .match_node(Some("c"), vec!["Company"])
        .from(vec!["WORKS_AT".to_string()], Some("r".to_string()))
        .node(Some("a"), vec!["Person"])
        .field("c", "name", None::<String>);

    let (a_i, r_i) = b_inc.build();
    let res_i = pgq.visit_query(&a_i, r_i).unwrap();
    assert!(
        res_i
            .statement
            .contains("(c IS Company) <-[r IS WORKS_AT]- (a IS Person)")
    );
}

#[test]
fn test_pgq_all_aggregations_and_pagination() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .field("p", "city", Some("city".to_string()))
        .select_property_aggregate(
            "p",
            "name",
            AggregationFunc::Count,
            Some("total_count".to_string()),
        )
        .select_property_aggregate(
            "p",
            "age",
            AggregationFunc::Avg,
            Some("avg_age".to_string()),
        )
        .select_property_aggregate(
            "p",
            "age",
            AggregationFunc::Sum,
            Some("total_age".to_string()),
        )
        .select_property_aggregate(
            "p",
            "age",
            AggregationFunc::Min,
            Some("min_age".to_string()),
        )
        .select_property_aggregate(
            "p",
            "age",
            AggregationFunc::Max,
            Some("max_age".to_string()),
        )
        .select_property_aggregate(
            "p",
            "name",
            AggregationFunc::Collect,
            Some("all_names".to_string()),
        )
        .skip(10)
        .limit(25);

    let (arena, root) = builder.build();
    let mut pgq = SqlPgqEmitter::new("social_graph");
    let res = pgq.visit_query(&arena, root).unwrap();

    assert!(res.statement.contains("COUNT(p.name) AS total_count"));
    assert!(res.statement.contains("AVG(p.age) AS avg_age"));
    assert!(res.statement.contains("SUM(p.age) AS total_age"));
    assert!(res.statement.contains("MIN(p.age) AS min_age"));
    assert!(res.statement.contains("MAX(p.age) AS max_age"));
    assert!(res.statement.contains("ARRAY_AGG(p.name) AS all_names"));
    assert!(res.statement.contains("LIMIT 25 OFFSET 10"));
}
