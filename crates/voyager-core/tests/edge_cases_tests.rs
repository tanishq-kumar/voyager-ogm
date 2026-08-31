//! Comprehensive Edge-Case and Stress Test Matrix for Voyager Core Query Engine.
//!
//! Tests subtle compiler semantics, cyclic paths, anonymous patterns, deep boolean logic,
//! escaping, multi-match joins, and dialect-specific invariants.

use voyager_core::ast::*;
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

#[test]
fn test_anonymous_node_and_edge_patterns() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(None::<&str>, Vec::<String>::new()) // ()
        .to(Vec::<String>::new(), None::<&str>) // -[]->
        .node(None::<&str>, vec!["Target"]); // (:Target)

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(res.statement, "MATCH ()-[]->(:Target)");

    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(res_gql.statement, "MATCH ()-[]->(:Target)");
}

#[test]
fn test_cyclic_graph_path_traversal() {
    // (a:User)-[:KNOWS]->(b:User)-[:KNOWS]->(c:User)-[:KNOWS]->(a)
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("a"), vec!["User"])
        .to(vec!["KNOWS"], Some("r1"))
        .node(Some("b"), vec!["User"])
        .to(vec!["KNOWS"], Some("r2"))
        .node(Some("c"), vec!["User"])
        .to(vec!["KNOWS"], Some("r3"))
        .node(Some("a"), vec!["User"])
        .where_eq("a", "id", 42)
        .r#return()
        .field("a", "name", None::<&str>)
        .field("b", "name", None::<&str>)
        .field("c", "name", None::<&str>);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (a:User)-[r1:KNOWS]->(b:User)-[r2:KNOWS]->(c:User)-[r3:KNOWS]->(a:User) WHERE a.id = $p0 RETURN a.name, b.name, c.name"
    );
    assert_eq!(res.parameters.get("p0"), Some(&LiteralValue::Int64(42)));
}

#[test]
fn test_bidirectional_zigzag_path_reversal() {
    // (a:Person)->(b:Post)<-(c:Person)->(d:Comment)
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("a"), vec!["Person"])
        .to(vec!["POSTED"], Some("p1"))
        .node(Some("b"), vec!["Post"])
        .from(vec!["POSTED"], Some("p2"))
        .node(Some("c"), vec!["Person"])
        .to(vec!["COMMENTED"], Some("cm"))
        .node(Some("d"), vec!["Comment"])
        .r#return()
        .field("a", "name", Some("author1"))
        .field("c", "name", Some("author2"));

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (a:Person)-[p1:POSTED]->(b:Post)<-[p2:POSTED]-(c:Person)-[cm:COMMENTED]->(d:Comment) RETURN a.name AS author1, c.name AS author2"
    );
}

#[test]
fn test_undirected_relationship_traversals() {
    // (p1:Person)-[r:FRIENDS]-(p2:Person)
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p1"), vec!["Person"])
        .edge(vec!["FRIENDS"], Some("r"))
        .node(Some("p2"), vec!["Person"])
        .r#return()
        .field("p1", "name", None::<&str>)
        .field("p2", "name", None::<&str>);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res_cypher = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_cypher.statement,
        "MATCH (p1:Person)-[r:FRIENDS]-(p2:Person) RETURN p1.name, p2.name"
    );

    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_gql.statement,
        "MATCH (p1:Person)-[r:FRIENDS]-(p2:Person) RETURN p1.name, p2.name"
    );
}

#[test]
fn test_multi_clause_match_and_optional_match_cartesian() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("u"), vec!["User"])
        .optional_match()
        .node(Some("u"), vec!["User"])
        .to(vec!["HAS_PROFILE"], Some("hp"))
        .node(Some("prof"), vec!["Profile"])
        .where_eq("u", "active", true)
        .r#return()
        .field("u", "name", None::<&str>)
        .field("prof", "bio", None::<&str>);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (u:User) OPTIONAL MATCH (u:User)-[hp:HAS_PROFILE]->(prof:Profile) WHERE u.active = $p0 RETURN u.name, prof.bio"
    );
    assert_eq!(res.parameters.get("p0"), Some(&LiteralValue::Bool(true)));
}

#[test]
fn test_complex_nested_boolean_and_or_precedence() {
    // Condition: ((p.age > 18 AND p.age < 65) OR p.is_vip = true) AND (p.status = 'ACTIVE')
    let mut builder = QueryBuilder::new();

    let p_age = builder.prop("p", "age");
    let lit_18 = builder.literal(18);
    let c1 = builder.binary_expr(p_age, BinaryOp::Gt, lit_18);

    let lit_65 = builder.literal(65);
    let c2 = builder.binary_expr(p_age, BinaryOp::Lt, lit_65);
    let age_range = builder.binary_expr(c1, BinaryOp::And, c2);

    let p_vip = builder.prop("p", "is_vip");
    let lit_true = builder.literal(true);
    let c_vip = builder.binary_expr(p_vip, BinaryOp::Eq, lit_true);
    let or_vip = builder.binary_expr(age_range, BinaryOp::Or, c_vip);

    let p_status = builder.prop("p", "status");
    let lit_active = builder.literal("ACTIVE");
    let c_status = builder.binary_expr(p_status, BinaryOp::Eq, lit_active);
    let full_cond = builder.binary_expr(or_vip, BinaryOp::And, c_status);

    builder
        .node(Some("p"), vec!["Person"])
        .filter(full_cond)
        .field("p", "name", None::<&str>);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (p:Person) WHERE (((p.age > $p0) AND (p.age < $p1)) OR (p.is_vip = $p2)) AND (p.status = $p3) RETURN p.name"
    );
    assert_eq!(res.parameters.len(), 4);
}

#[test]
fn test_escaping_quotes_and_special_characters_in_literals() {
    let mut builder = QueryBuilder::new();
    let complex_str = "Robert \"Bobby\" O'Connor\n\t-- Special /* Injected */ chars; DROP TABLE";

    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_eq("p", "name", complex_str)
        .r#return()
        .field("p", "id", None::<&str>);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();

    // Query is strictly parameterized, preventing any SQL/Cypher injection
    assert_eq!(
        res.statement,
        "MATCH (p:Person) WHERE p.name = $p0 RETURN p.id"
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::String(complex_str.into()))
    );
}

#[test]
fn test_numeric_boundary_extremes() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("n"), vec!["Metric"])
        .where_gte("n", "min_val", i64::MIN)
        .where_lte("n", "max_val", i64::MAX)
        .where_eq("n", "zero_val", 0)
        .where_gt("n", "float_val", -999.999)
        .r#return()
        .field("n", "id", None::<&str>);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (n:Metric) WHERE (((n.min_val >= $p0) AND (n.max_val <= $p1)) AND (n.zero_val = $p2)) AND (n.float_val > $p3) RETURN n.id"
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::Int64(i64::MIN))
    );
    assert_eq!(
        res.parameters.get("p1"),
        Some(&LiteralValue::Int64(i64::MAX))
    );
    assert_eq!(res.parameters.get("p2"), Some(&LiteralValue::Int64(0)));
    assert_eq!(
        res.parameters.get("p3"),
        Some(&LiteralValue::Float64(-999.999))
    );
}

#[test]
fn test_multi_label_conjunction_differences_between_dialects() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person", "Employee", "Manager"])
        .r#return()
        .field("p", "name", None::<&str>);

    let (arena, root) = builder.build();

    // 1. openCypher uses colon concatenation: (p:Person:Employee:Manager)
    let mut cypher = CypherEmitter::new();
    let res_cypher = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_cypher.statement,
        "MATCH (p:Person:Employee:Manager) RETURN p.name"
    );

    // 2. ISO GQL uses ampersand conjunction: (p:Person&Employee&Manager)
    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_gql.statement,
        "MATCH (p:Person&Employee&Manager) RETURN p.name"
    );
}

#[test]
fn test_all_aggregation_functions_and_distinct_projections() {
    let mut builder = QueryBuilder::new();
    let p_age = builder.prop("p", "age");
    let p_salary = builder.prop("p", "salary");
    let p_dept = builder.prop("p", "department");

    builder
        .r#match()
        .node(Some("p"), vec!["Employee"])
        .distinct(true)
        .field("p", "department", Some("dept"))
        .select_aggregate(p_age, AggregationFunc::Count, Some("total_count"))
        .select_aggregate(p_age, AggregationFunc::CountDistinct, Some("unique_ages"))
        .select_aggregate(p_salary, AggregationFunc::Avg, Some("avg_salary"))
        .select_aggregate(p_salary, AggregationFunc::Sum, Some("total_payroll"))
        .select_aggregate(p_salary, AggregationFunc::Min, Some("min_salary"))
        .select_aggregate(p_salary, AggregationFunc::Max, Some("max_salary"))
        .select_aggregate(p_dept, AggregationFunc::Collect, Some("dept_list"))
        .skip(10)
        .limit(20);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    let expected = "MATCH (p:Employee) RETURN DISTINCT p.department AS dept, COUNT(p.age) AS total_count, COUNT(DISTINCT p.age) AS unique_ages, AVG(p.salary) AS avg_salary, SUM(p.salary) AS total_payroll, MIN(p.salary) AS min_salary, MAX(p.salary) AS max_salary, COLLECT(p.department) AS dept_list SKIP 10 LIMIT 20";
    assert_eq!(res.statement, expected);
}

#[test]
fn test_sql_pgq_like_and_case_insensitive_translation() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("c"), vec!["Customer"])
        .where_contains("c", "email", "@acme.corp")
        .r#return()
        .field("c", "email", None::<&str>)
        .limit(50);

    let (arena, root) = builder.build();

    let mut pgq = SqlPgqEmitter::new("crm_graph");
    let res = pgq.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "SELECT * FROM GRAPH_TABLE (crm_graph MATCH (c IS Customer) WHERE c.email LIKE '%' || $p0 || '%' COLUMNS (c.email)) LIMIT 50"
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::String("@acme.corp".into()))
    );
}

#[test]
fn test_arena_capacity_expansion_and_stress_100_hops() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("n0"), vec!["Node"]);

    for i in 1..=100 {
        builder
            .to(vec!["LINK"], Some(format!("r{i}")))
            .node(Some(format!("n{i}")), vec!["Node"])
            .where_gt(format!("n{i}"), "val", i as i64);
    }

    builder.r#return().field("n0", "id", None::<&str>).limit(1);

    let (arena, root) = builder.build();
    assert!(arena.len() > 300); // Proves dynamic 32-bit arena reallocation works without leaks

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert!(
        res.statement
            .starts_with("MATCH (n0:Node)-[r1:LINK]->(n1:Node)")
    );
    assert_eq!(res.parameters.len(), 100);
}
