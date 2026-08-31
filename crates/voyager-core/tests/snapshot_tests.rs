use insta::{assert_snapshot, assert_yaml_snapshot};
use voyager_core::ast::*;
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

#[test]
fn snapshot_movie_graph_co_actors() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["ACTED_IN"], Some("r"))
        .hops(1, 2)
        .node(Some("m"), vec!["Movie"])
        .from(vec!["DIRECTED"], Some("d_rel"))
        .node(Some("d"), vec!["Director"])
        .where_gt("p", "age", 21)
        .where_eq("m", "released", 1999)
        .r#return()
        .field("p", "name", Some("actor"))
        .field("m", "title", Some("movie"))
        .field("d", "name", Some("director"))
        .order_by_asc("p", "name")
        .limit(10);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let cypher_res = cypher.visit_query(&arena, root).unwrap();
    assert_snapshot!("movie_co_actors_cypher", cypher_res.statement);
    assert_yaml_snapshot!(
        "movie_co_actors_cypher_params",
        cypher_res.sorted_parameters()
    );

    let mut pgq = SqlPgqEmitter::new("movie_graph");
    let pgq_res = pgq.visit_query(&arena, root).unwrap();
    assert_snapshot!("movie_co_actors_sql_pgq", pgq_res.statement);

    let mut gql = IsoGqlEmitter::new();
    let gql_res = gql.visit_query(&arena, root).unwrap();
    assert_snapshot!("movie_co_actors_iso_gql", gql_res.statement);
}

#[test]
fn snapshot_ldbc_social_variable_paths() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS"], Some("r"))
        .hops(1, 3)
        .node(Some("b"), vec!["Person"])
        .where_eq("a", "name", "Alice")
        .r#return()
        .field("a", "name", Some("start_person"))
        .field("b", "name", Some("reachable_person"))
        .order_by_asc("b", "name")
        .limit(50);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let cypher_res = cypher.visit_query(&arena, root).unwrap();
    assert_snapshot!("ldbc_social_paths_cypher", cypher_res.statement);

    let mut pgq = SqlPgqEmitter::new("social_graph");
    let pgq_res = pgq.visit_query(&arena, root).unwrap();
    assert_snapshot!("ldbc_social_paths_sql_pgq", pgq_res.statement);

    let mut gql = IsoGqlEmitter::new();
    let gql_res = gql.visit_query(&arena, root).unwrap();
    assert_snapshot!("ldbc_social_paths_iso_gql", gql_res.statement);
}

#[test]
fn snapshot_filter_expressions_string_and_range() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_gte("p", "age", 21)
        .where_lte("p", "age", 65)
        .where_contains("p", "name", "Smith")
        .r#return()
        .field("p", "name", Some("full_name"))
        .field("p", "age", Some("age"))
        .order_by_desc("p", "age");

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let cypher_res = cypher.visit_query(&arena, root).unwrap();
    assert_snapshot!("filter_range_cypher", cypher_res.statement);

    let mut pgq = SqlPgqEmitter::new("person_graph");
    let pgq_res = pgq.visit_query(&arena, root).unwrap();
    assert_snapshot!("filter_range_sql_pgq", pgq_res.statement);

    let mut gql = IsoGqlEmitter::new();
    let gql_res = gql.visit_query(&arena, root).unwrap();
    assert_snapshot!("filter_range_iso_gql", gql_res.statement);
}

#[test]
fn snapshot_aggregations_and_grouping() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["ACTED_IN"], Some("r"))
        .node(Some("m"), vec!["Movie"])
        .r#return()
        .field("p", "name", Some("actor"))
        .select_property_aggregate("m", "title", AggregationFunc::Count, Some("movie_count"))
        .select_property_aggregate("m", "released", AggregationFunc::Avg, Some("avg_released"))
        .order_by_desc("p", "movie_count")
        .limit(20);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let cypher_res = cypher.visit_query(&arena, root).unwrap();
    assert_snapshot!("aggregations_cypher", cypher_res.statement);

    let mut pgq = SqlPgqEmitter::new("movies_graph");
    let pgq_res = pgq.visit_query(&arena, root).unwrap();
    assert_snapshot!("aggregations_sql_pgq", pgq_res.statement);

    let mut gql = IsoGqlEmitter::new();
    let gql_res = gql.visit_query(&arena, root).unwrap();
    assert_snapshot!("aggregations_iso_gql", gql_res.statement);
}

#[test]
fn snapshot_multi_label_conjunction() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person", "Developer"])
        .to(vec!["PRODUCED"], Some("r"))
        .node(Some("s"), vec!["Software", "OpenSource"])
        .where_eq("s", "license", "MIT")
        .r#return()
        .field("p", "name", Some("author"))
        .field("s", "name", Some("project"));

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let cypher_res = cypher.visit_query(&arena, root).unwrap();
    assert_snapshot!("multi_label_cypher", cypher_res.statement);

    let mut pgq = SqlPgqEmitter::new("oss_graph");
    let pgq_res = pgq.visit_query(&arena, root).unwrap();
    assert_snapshot!("multi_label_sql_pgq", pgq_res.statement);

    let mut gql = IsoGqlEmitter::new();
    let gql_res = gql.visit_query(&arena, root).unwrap();
    assert_snapshot!("multi_label_iso_gql", gql_res.statement);
}

#[test]
fn snapshot_vendor_procedure_call() {
    let mut arena = QueryAstArena::new();
    let arg1 = arena.alloc(AstNode::Literal(LiteralValue::String("Person".into())));
    let arg2 = arena.alloc(AstNode::Literal(LiteralValue::Int64(3)));

    let proc_handle = arena.alloc(AstNode::ProcedureCall {
        namespace: Some("apoc.path".into()),
        procedure: "subgraphNodes".into(),
        arguments: vec![arg1, arg2],
        yield_items: vec!["node".into()],
    });

    let mut cypher = CypherEmitter::new();
    let compiled = cypher.visit_query(&arena, proc_handle).unwrap();
    assert_snapshot!("apoc_procedure_cypher", compiled.statement);
    assert_yaml_snapshot!("apoc_procedure_params", compiled.sorted_parameters());
}
