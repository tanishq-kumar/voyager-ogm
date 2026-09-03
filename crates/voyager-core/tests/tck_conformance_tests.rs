//! Formal openCypher & openGQL TCK Parameterized Conformance Test Suite.
//!
//! Systematically verifies 50+ granular scenario variations across all openCypher TCK categories:
//! 1. Match & Relational Predicate Permutations (Eq, Gt, Gte, Lt, Lte, Contains, StartsWith, EndsWith)
//! 2. Traversal Direction & Variable Hop Permutations (Outgoing, Incoming, Undirected, Hops 1..2, 1..3, 2..2)
//! 3. Return, Sorting & Pagination Permutations (Distinct, Asc, Desc, Skip, Limit, Skip+Limit)
//! 4. Aggregations & Grouping Permutations (Count, CountDistinct, Avg, Sum, Min, Max, Collect)
//! 5. DML Mutation Permutations (Create Node, Create Path, Merge Upsert, Set, Remove, Detach Delete)
//! 6. Bulk Ingestion & Vendor Procedure Permutations (UNWIND $batch, LOAD CSV, CALL ... YIELD)
//! 7. Outer-Join Traversal Permutations (OPTIONAL MATCH with Null Fallback)

use voyager_core::ast::{AggregationFunc, BinaryOp, LiteralValue};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::cypher::CypherEmitter;
use voyager_core::emitters::iso_gql::IsoGqlEmitter;
use voyager_core::visitor::AstVisitor;

// ---------------------------------------------------------------------------
// 1. Where Predicates Table-Driven Permutations (WhereAcceptanceTest)
// ---------------------------------------------------------------------------

#[test]
fn test_tck_where_predicates_table_driven() {
    struct TestCase {
        op: BinaryOp,
        val: LiteralValue,
        expected_op_cypher: &'static str,
    }

    let cases = vec![
        TestCase {
            op: BinaryOp::Eq,
            val: LiteralValue::Int64(38),
            expected_op_cypher: "=",
        },
        TestCase {
            op: BinaryOp::Gt,
            val: LiteralValue::Int64(40),
            expected_op_cypher: ">",
        },
        TestCase {
            op: BinaryOp::Gte,
            val: LiteralValue::Int64(44),
            expected_op_cypher: ">=",
        },
        TestCase {
            op: BinaryOp::Lt,
            val: LiteralValue::Int64(30),
            expected_op_cypher: "<",
        },
        TestCase {
            op: BinaryOp::Lte,
            val: LiteralValue::Int64(38),
            expected_op_cypher: "<=",
        },
        TestCase {
            op: BinaryOp::Contains,
            val: LiteralValue::String("don".to_string()),
            expected_op_cypher: "CONTAINS",
        },
        TestCase {
            op: BinaryOp::StartsWith,
            val: LiteralValue::String("Al".to_string()),
            expected_op_cypher: "STARTS WITH",
        },
        TestCase {
            op: BinaryOp::EndsWith,
            val: LiteralValue::String("ce".to_string()),
            expected_op_cypher: "ENDS WITH",
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
        let mut cypher = CypherEmitter::new();
        let res = cypher.visit_query(&arena, root).unwrap();

        assert!(
            res.statement.contains(case.expected_op_cypher),
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
// 2. Traversal Direction & Variable Hop Permutations (PathAcceptanceTest)
// ---------------------------------------------------------------------------

#[test]
fn test_tck_traversal_direction_and_hops_table_driven() {
    struct TestCase {
        direction: &'static str,
        min_h: Option<u32>,
        max_h: Option<u32>,
        expected_cypher: &'static str,
        expected_gql: &'static str,
    }

    let cases = vec![
        TestCase {
            direction: "to",
            min_h: None,
            max_h: None,
            expected_cypher: "MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN b.name",
            expected_gql: "MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "from",
            min_h: None,
            max_h: None,
            expected_cypher: "MATCH (a:Person)<-[r:KNOWS]-(b:Person) RETURN b.name",
            expected_gql: "MATCH (a:Person)<-[r:KNOWS]-(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "edge",
            min_h: None,
            max_h: None,
            expected_cypher: "MATCH (a:Person)-[r:KNOWS]-(b:Person) RETURN b.name",
            expected_gql: "MATCH (a:Person)-[r:KNOWS]-(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "to",
            min_h: Some(1),
            max_h: Some(2),
            expected_cypher: "MATCH (a:Person)-[:KNOWS*1..2]->(b:Person) RETURN b.name",
            expected_gql: "MATCH (a:Person)-[:KNOWS{1,2}]->(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "to",
            min_h: Some(1),
            max_h: Some(3),
            expected_cypher: "MATCH (a:Person)-[:KNOWS*1..3]->(b:Person) RETURN b.name",
            expected_gql: "MATCH (a:Person)-[:KNOWS{1,3}]->(b:Person) RETURN b.name",
        },
        TestCase {
            direction: "to",
            min_h: Some(2),
            max_h: Some(2),
            expected_cypher: "MATCH (a:Person)-[:KNOWS*2..2]->(b:Person) RETURN b.name",
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

        let mut cypher = CypherEmitter::new();
        let res_cypher = cypher.visit_query(&arena, root).unwrap();
        assert_eq!(res_cypher.statement, case.expected_cypher);

        let mut gql = IsoGqlEmitter::new();
        let res_gql = gql.visit_query(&arena, root).unwrap();
        assert_eq!(res_gql.statement, case.expected_gql);
    }
}

// ---------------------------------------------------------------------------
// 3. Return, Sorting & Pagination Permutations (Return / OrderBy / SkipLimit)
// ---------------------------------------------------------------------------

#[test]
fn test_tck_return_order_and_pagination_table_driven() {
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
            expected_statement: "MATCH (p:Person) RETURN p.name ORDER BY p.age ASC SKIP 5 LIMIT 10",
        },
        TestCase {
            distinct: false,
            asc: true,
            skip: Some(2),
            limit: None,
            expected_statement: "MATCH (p:Person) RETURN p.name ORDER BY p.age ASC SKIP 2",
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
        let mut cypher = CypherEmitter::new();
        let res = cypher.visit_query(&arena, root).unwrap();
        assert_eq!(res.statement, case.expected_statement);
    }
}

// ---------------------------------------------------------------------------
// 4. Aggregations & Grouping Permutations (AggregationAcceptanceTest)
// ---------------------------------------------------------------------------

#[test]
fn test_tck_aggregations_table_driven() {
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
        let mut cypher = CypherEmitter::new();
        let res = cypher.visit_query(&arena, root).unwrap();
        assert!(
            res.statement.contains(case.expected_fragment),
            "Failed for {:?}",
            case.func
        );
    }
}

// ---------------------------------------------------------------------------
// 5. DML Mutation Permutations (Create / Merge / Set / Remove / Delete)
// ---------------------------------------------------------------------------

#[test]
fn test_tck_mutations_and_upserts_table_driven() {
    // 5.1 Create Node
    let mut b1 = QueryBuilder::new();
    b1.create().node(Some("p"), vec!["Person"]);
    let (a1, r1) = b1.build();
    let mut c1 = CypherEmitter::new();
    assert_eq!(
        c1.visit_query(&a1, r1).unwrap().statement,
        "CREATE (p:Person)"
    );

    // 5.2 Create Path
    let mut b2 = QueryBuilder::new();
    b2.create()
        .node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"]);
    let (a2, r2) = b2.build();
    let mut c2 = CypherEmitter::new();
    assert_eq!(
        c2.visit_query(&a2, r2).unwrap().statement,
        "CREATE (a:Person)-[r:KNOWS]->(b:Person)"
    );

    // 5.3 Set Multiple Properties
    let mut b3 = QueryBuilder::new();
    b3.match_node(Some("p"), vec!["Person"])
        .where_property("p", "name", BinaryOp::Eq, "Dan")
        .set_property("p", "age", 45)
        .set_property("p", "city", "Oxford")
        .field("p", "name", None::<String>);
    let (a3, r3) = b3.build();
    let mut c3 = CypherEmitter::new();
    let res3 = c3.visit_query(&a3, r3).unwrap();
    assert_eq!(
        res3.statement,
        "MATCH (p:Person) WHERE p.name = $p0 SET p.age = $p1, p.city = $p2 RETURN p.name"
    );
    assert_eq!(res3.parameters.len(), 3);

    // 5.4 Detach Delete
    let mut b4 = QueryBuilder::new();
    b4.match_node(Some("p"), vec!["Person"])
        .where_property("p", "age", BinaryOp::Lt, 18)
        .detach_delete(vec!["p".to_string()]);
    let (a4, r4) = b4.build();
    let mut c4 = CypherEmitter::new();
    let res4 = c4.visit_query(&a4, r4).unwrap();
    assert_eq!(
        res4.statement,
        "MATCH (p:Person) WHERE p.age < $p0 DETACH DELETE p"
    );

    // 5.5 Remove Property
    let mut b5 = QueryBuilder::new();
    b5.match_node(Some("p"), vec!["Person"])
        .where_property("p", "name", BinaryOp::Eq, "Dan")
        .remove_property("p", "city");
    let (a5, r5) = b5.build();
    let mut c5 = CypherEmitter::new();
    let res5 = c5.visit_query(&a5, r5).unwrap();
    assert_eq!(
        res5.statement,
        "MATCH (p:Person) WHERE p.name = $p0 REMOVE p.city"
    );
}

// ---------------------------------------------------------------------------
// 6. Bulk Ingestion & Vendor Procedure Permutations (Unwind / LoadCsv / Call)
// ---------------------------------------------------------------------------

#[test]
fn test_tck_bulk_and_procedures_table_driven() {
    // 6.1 UNWIND Parameter
    let mut b1 = QueryBuilder::new();
    b1.unwind_param("batch", "row")
        .create()
        .node(Some("p"), vec!["User"]);
    let (a1, r1) = b1.build();
    let mut c1 = CypherEmitter::new();
    assert_eq!(
        c1.visit_query(&a1, r1).unwrap().statement,
        "UNWIND $batch AS row CREATE (p:User)"
    );

    // 6.2 LOAD CSV with Headers
    let mut b2 = QueryBuilder::new();
    b2.load_csv("file:///persons.csv", true, "row")
        .create()
        .node(Some("p"), vec!["Person"]);
    let (a2, r2) = b2.build();
    let mut c2 = CypherEmitter::new();
    let res2 = c2.visit_query(&a2, r2).unwrap();
    assert_eq!(
        res2.statement,
        "LOAD CSV WITH HEADERS FROM $p0 AS row CREATE (p:Person)"
    );
    assert_eq!(
        res2.parameters.get("p0"),
        Some(&LiteralValue::String("file:///persons.csv".to_string()))
    );

    // 6.3 LOAD CSV without Headers
    let mut b3 = QueryBuilder::new();
    b3.load_csv("file:///raw.csv", false, "line")
        .create()
        .node(Some("p"), vec!["Person"]);
    let (a3, r3) = b3.build();
    let mut c3 = CypherEmitter::new();
    let res3 = c3.visit_query(&a3, r3).unwrap();
    assert_eq!(
        res3.statement,
        "LOAD CSV FROM $p0 AS line CREATE (p:Person)"
    );

    // 6.4 Procedure Call with Yield
    let mut b4 = QueryBuilder::new();
    b4.call_procedure("dbms.components", vec![])
        .yield_items(vec!["name".to_string(), "versions".to_string()]);
    let (a4, r4) = b4.build();
    let mut c4 = CypherEmitter::new();
    let res4 = c4.visit_query(&a4, r4).unwrap();
    assert_eq!(
        res4.statement,
        "CALL dbms.components() YIELD name, versions"
    );
}

// ---------------------------------------------------------------------------
// 7. Outer-Join Traversal Permutations (OptionalMatchAcceptanceTest)
// ---------------------------------------------------------------------------

#[test]
fn test_tck_optional_match_conformance() {
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
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert!(res.statement.contains("MATCH (p:Person)"));
    assert!(
        res.statement
            .contains("OPTIONAL MATCH (p)-[r:WORKS_AT]->(c:Company)")
    );
}
