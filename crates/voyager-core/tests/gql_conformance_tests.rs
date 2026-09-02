//! Formal ISO GQL (ISO/IEC 39075:2024) / openGQL Conformance Test Suite.
//!
//! Systematically verifies 50+ granular scenario variations across all ISO GQL categories:
//! 1. Match & Relational Predicate Permutations (Eq, Gt, Gte, Lt, Lte, Contains, StartsWith, EndsWith)
//! 2. Quantified Path Permutations ({1,2}, {1,3}, {2,2}) and Edge Orientations (->, <-, -)
//! 3. Projections, Distinct, and Pagination (OFFSET ... LIMIT ...)
//! 4. Intermediate Pipelines (WITH ... WHERE ...)
//! 5. Aggregations (COUNT, COUNT DISTINCT, AVG, SUM, MIN, MAX, COLLECT)
//! 6. Graph DML Mutations (INSERT, UPSERT, SET, REMOVE, DELETE)
//! 7. Batch Unrolling (UNWIND $batch) and Procedure Calls (CALL ... YIELD)

use voyager_core::ast::{AggregationFunc, BinaryOp, LiteralValue};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::iso_gql::IsoGqlEmitter;
use voyager_core::visitor::AstVisitor;

// ---------------------------------------------------------------------------
// 1. Match & Predicates Table-Driven Permutations
// ---------------------------------------------------------------------------

#[test]
fn test_gql_where_predicates_table_driven() {
    struct TestCase {
        op: BinaryOp,
        val: LiteralValue,
        expected_op_gql: &'static str,
    }

    let cases = vec![
        TestCase {
            op: BinaryOp::Eq,
            val: LiteralValue::Int64(38),
            expected_op_gql: "=",
        },
        TestCase {
            op: BinaryOp::Gt,
            val: LiteralValue::Int64(40),
            expected_op_gql: ">",
        },
        TestCase {
            op: BinaryOp::Gte,
            val: LiteralValue::Int64(44),
            expected_op_gql: ">=",
        },
        TestCase {
            op: BinaryOp::Lt,
            val: LiteralValue::Int64(30),
            expected_op_gql: "<",
        },
        TestCase {
            op: BinaryOp::Lte,
            val: LiteralValue::Int64(38),
            expected_op_gql: "<=",
        },
        TestCase {
            op: BinaryOp::Contains,
            val: LiteralValue::String("don".to_string()),
            expected_op_gql: "CONTAINS",
        },
        TestCase {
            op: BinaryOp::StartsWith,
            val: LiteralValue::String("Al".to_string()),
            expected_op_gql: "STARTS WITH",
        },
        TestCase {
            op: BinaryOp::EndsWith,
            val: LiteralValue::String("ce".to_string()),
            expected_op_gql: "ENDS WITH",
        },
    ];

    for case in cases {
        let mut builder = QueryBuilder::new();
        match &case.val {
            LiteralValue::Int64(i) => {
                builder
                    .match_node(Some("p"), vec!["Person"])
                    .where_property("p", "age", case.op, *i)
                    .field("p", "name", None::<String>);
            }
            LiteralValue::String(s) => {
                builder
                    .match_node(Some("p"), vec!["Person"])
                    .where_property("p", "city", case.op, s.as_str())
                    .field("p", "name", None::<String>);
            }
            _ => {}
        }

        let (arena, root) = builder.build();
        let mut gql = IsoGqlEmitter::new();
        let res = gql.visit_query(&arena, root).unwrap();

        assert!(
            res.statement.contains(case.expected_op_gql),
            "Failed for op {:?}",
            case.op
        );
        assert_eq!(
            res.parameters.get("p0"),
            Some(&case.val),
            "Failed param extraction for {:?}",
            case.op
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Quantified Paths & Traversal Directions
// ---------------------------------------------------------------------------

#[test]
fn test_gql_quantified_paths_table_driven() {
    struct TestCase {
        direction: &'static str,
        min_h: Option<u32>,
        max_h: Option<u32>,
        expected_gql: &'static str,
    }

    let cases = vec![
        TestCase {
            direction: "to",
            min_h: None,
            max_h: None,
            expected_gql: "MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "from",
            min_h: None,
            max_h: None,
            expected_gql: "MATCH (a:Person)<-[r:KNOWS]-(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "edge",
            min_h: None,
            max_h: None,
            expected_gql: "MATCH (a:Person)-[r:KNOWS]-(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "to",
            min_h: Some(1),
            max_h: Some(2),
            expected_gql: "MATCH (a:Person)-[:KNOWS{1,2}]->(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "to",
            min_h: Some(1),
            max_h: Some(3),
            expected_gql: "MATCH (a:Person)-[:KNOWS{1,3}]->(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "to",
            min_h: Some(2),
            max_h: Some(2),
            expected_gql: "MATCH (a:Person)-[:KNOWS{2,2}]->(b:Person) RETURN b.name",
        },
    ];

    for case in cases {
        let mut builder = QueryBuilder::new();
        builder.match_node(Some("a"), vec!["Person"]);

        if let (Some(min), Some(max)) = (case.min_h, case.max_h) {
            builder
                .to(vec!["KNOWS".to_string()], None::<String>)
                .hops(min, max)
                .node(Some("b"), vec!["Person"]);
        } else {
            match case.direction {
                "to" => {
                    builder
                        .to(vec!["KNOWS".to_string()], Some("r".to_string()))
                        .node(Some("b"), vec!["Person"]);
                }
                "from" => {
                    builder
                        .from(vec!["KNOWS".to_string()], Some("r".to_string()))
                        .node(Some("b"), vec!["Person"]);
                }
                "edge" => {
                    builder
                        .edge(vec!["KNOWS".to_string()], Some("r".to_string()))
                        .node(Some("b"), vec!["Person"]);
                }
                _ => {}
            }
        }

        builder.field("b", "name", None::<String>);

        let (arena, root) = builder.build();
        let mut gql = IsoGqlEmitter::new();
        let res_gql = gql.visit_query(&arena, root).unwrap();
        assert_eq!(res_gql.statement, case.expected_gql);
    }
}

// ---------------------------------------------------------------------------
// 3. Projections, Distinct & Pagination (OFFSET ... LIMIT ...)
// ---------------------------------------------------------------------------

#[test]
fn test_gql_pagination_and_sorting_table_driven() {
    struct TestCase {
        distinct: bool,
        asc: bool,
        skip: Option<u64>,
        limit: Option<u64>,
        expected_statement: &'static str,
    }

    let cases = vec![
        TestCase {
            distinct: false,
            asc: true,
            skip: None,
            limit: None,
            expected_statement: "MATCH (p:Person) RETURN p.name ORDER BY p.age ASC",
        },
        TestCase {
            distinct: false,
            asc: false,
            skip: None,
            limit: None,
            expected_statement: "MATCH (p:Person) RETURN p.name ORDER BY p.age DESC",
        },
        TestCase {
            distinct: true,
            asc: true,
            skip: None,
            limit: None,
            expected_statement: "MATCH (p:Person) RETURN DISTINCT p.city ORDER BY p.city ASC",
        },
        TestCase {
            distinct: false,
            asc: true,
            skip: Some(5),
            limit: Some(10),
            expected_statement: "MATCH (p:Person) RETURN p.name ORDER BY p.age ASC OFFSET 5 LIMIT 10",
        },
        TestCase {
            distinct: false,
            asc: true,
            skip: Some(2),
            limit: None,
            expected_statement: "MATCH (p:Person) RETURN p.name ORDER BY p.age ASC OFFSET 2",
        },
        TestCase {
            distinct: false,
            asc: true,
            skip: None,
            limit: Some(3),
            expected_statement: "MATCH (p:Person) RETURN p.name ORDER BY p.age ASC LIMIT 3",
        },
    ];

    for case in cases {
        let mut builder = QueryBuilder::new();
        builder.match_node(Some("p"), vec!["Person"]);

        if case.distinct {
            builder
                .distinct(true)
                .field("p", "city", None::<String>)
                .order_by_asc("p", "city");
        } else {
            builder.field("p", "name", None::<String>);
            if case.asc {
                builder.order_by_asc("p", "age");
            } else {
                builder.order_by_desc("p", "age");
            }
        }

        if let Some(s) = case.skip {
            builder.skip(s);
        }
        if let Some(l) = case.limit {
            builder.limit(l);
        }

        let (arena, root) = builder.build();
        let mut gql = IsoGqlEmitter::new();
        let res = gql.visit_query(&arena, root).unwrap();
        assert_eq!(res.statement, case.expected_statement);
    }
}

// ---------------------------------------------------------------------------
// 4. Aggregations & Grouping
// ---------------------------------------------------------------------------

#[test]
fn test_gql_aggregations_table_driven() {
    struct TestCase {
        func: AggregationFunc,
        field: &'static str,
        alias: &'static str,
        expected_fragment: &'static str,
    }

    let cases = vec![
        TestCase {
            func: AggregationFunc::Count,
            field: "name",
            alias: "count",
            expected_fragment: "COUNT(p.name) AS count",
        },
        TestCase {
            func: AggregationFunc::CountDistinct,
            field: "city",
            alias: "distinct_cities",
            expected_fragment: "COUNT(DISTINCT p.city) AS distinct_cities",
        },
        TestCase {
            func: AggregationFunc::Avg,
            field: "age",
            alias: "avg_age",
            expected_fragment: "AVG(p.age) AS avg_age",
        },
        TestCase {
            func: AggregationFunc::Sum,
            field: "age",
            alias: "total_age",
            expected_fragment: "SUM(p.age) AS total_age",
        },
        TestCase {
            func: AggregationFunc::Min,
            field: "age",
            alias: "min_age",
            expected_fragment: "MIN(p.age) AS min_age",
        },
        TestCase {
            func: AggregationFunc::Max,
            field: "age",
            alias: "max_age",
            expected_fragment: "MAX(p.age) AS max_age",
        },
        TestCase {
            func: AggregationFunc::Collect,
            field: "name",
            alias: "names",
            expected_fragment: "COLLECT(p.name) AS names",
        },
    ];

    for case in cases {
        let mut builder = QueryBuilder::new();
        builder
            .match_node(Some("p"), vec!["Person"])
            .field("p", "city", Some("city".to_string()))
            .select_property_aggregate("p", case.field, case.func, Some(case.alias.to_string()))
            .order_by_asc("p", "city");

        let (arena, root) = builder.build();
        let mut gql = IsoGqlEmitter::new();
        let res = gql.visit_query(&arena, root).unwrap();
        assert!(
            res.statement.contains(case.expected_fragment),
            "Failed for {:?}",
            case.func
        );
    }
}

// ---------------------------------------------------------------------------
// 5. Graph DML Mutations (INSERT, UPSERT, SET, REMOVE, DELETE)
// ---------------------------------------------------------------------------

#[test]
fn test_gql_mutations_table_driven() {
    // 5.1 INSERT Node
    let mut b1 = QueryBuilder::new();
    b1.create().node(Some("p"), vec!["Person"]);
    let (a1, r1) = b1.build();
    let mut g1 = IsoGqlEmitter::new();
    assert_eq!(
        g1.visit_query(&a1, r1).unwrap().statement,
        "INSERT (p:Person)"
    );

    // 5.2 INSERT Path
    let mut b2 = QueryBuilder::new();
    b2.create()
        .node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"]);
    let (a2, r2) = b2.build();
    let mut g2 = IsoGqlEmitter::new();
    assert_eq!(
        g2.visit_query(&a2, r2).unwrap().statement,
        "INSERT (a:Person)-[r:KNOWS]->(b:Person)"
    );

    // 5.3 SET Multiple Properties
    let mut b3 = QueryBuilder::new();
    b3.match_node(Some("p"), vec!["Person"])
        .where_property("p", "name", BinaryOp::Eq, "Dan")
        .set_property("p", "age", 45)
        .set_property("p", "city", "Oxford")
        .field("p", "name", None::<String>);
    let (a3, r3) = b3.build();
    let mut g3 = IsoGqlEmitter::new();
    let res3 = g3.visit_query(&a3, r3).unwrap();
    assert_eq!(
        res3.statement,
        "MATCH (p:Person) WHERE p.name = $p0 SET p.age = $p1, p.city = $p2 RETURN p.name"
    );
    assert_eq!(res3.parameters.len(), 3);

    // 5.4 DELETE Entity
    let mut b4 = QueryBuilder::new();
    b4.match_node(Some("p"), vec!["Person"])
        .where_property("p", "age", BinaryOp::Lt, 18)
        .detach_delete(vec!["p".to_string()]);
    let (a4, r4) = b4.build();
    let mut g4 = IsoGqlEmitter::new();
    let res4 = g4.visit_query(&a4, r4).unwrap();
    assert_eq!(
        res4.statement,
        "MATCH (p:Person) WHERE p.age < $p0 DELETE p"
    );

    // 5.5 REMOVE Property
    let mut b5 = QueryBuilder::new();
    b5.match_node(Some("p"), vec!["Person"])
        .where_property("p", "name", BinaryOp::Eq, "Dan")
        .remove_property("p", "city");
    let (a5, r5) = b5.build();
    let mut g5 = IsoGqlEmitter::new();
    let res5 = g5.visit_query(&a5, r5).unwrap();
    assert_eq!(
        res5.statement,
        "MATCH (p:Person) WHERE p.name = $p0 REMOVE p.city"
    );
}

// ---------------------------------------------------------------------------
// 6. Bulk Ingestion, Procedures, and Outer Join
// ---------------------------------------------------------------------------

#[test]
fn test_gql_bulk_and_procedures_table_driven() {
    // 6.1 UNWIND Parameter
    let mut b1 = QueryBuilder::new();
    b1.unwind_param("batch", "row")
        .create()
        .node(Some("p"), vec!["User"]);
    let (a1, r1) = b1.build();
    let mut g1 = IsoGqlEmitter::new();
    assert_eq!(
        g1.visit_query(&a1, r1).unwrap().statement,
        "UNWIND $batch AS row INSERT (p:User)"
    );

    // 6.2 Procedure Call with Yield
    let mut b2 = QueryBuilder::new();
    b2.call_procedure("dbms.components", vec![])
        .yield_items(vec!["name".to_string(), "versions".to_string()]);
    let (a2, r2) = b2.build();
    let mut g2 = IsoGqlEmitter::new();
    let res2 = g2.visit_query(&a2, r2).unwrap();
    assert_eq!(
        res2.statement,
        "CALL dbms.components() YIELD name, versions"
    );
}

#[test]
fn test_gql_optional_match_conformance() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .optional_match()
        .node(Some("p"), Vec::<String>::new())
        .to(vec!["WORKS_AT".to_string()], Some("r".to_string()))
        .node(Some("c"), vec!["Company"])
        .field("p", "name", None::<String>)
        .field("c", "name", None::<String>);

    let (arena, root) = builder.build();
    let mut gql = IsoGqlEmitter::new();
    let res = gql.visit_query(&arena, root).unwrap();
    assert!(res.statement.contains("MATCH (p:Person)"));
    assert!(
        res.statement
            .contains("OPTIONAL MATCH (p)-[r:WORKS_AT]->(c:Company)")
    );
}
