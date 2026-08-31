use voyager_core::ast::*;
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

#[test]
fn test_cypher_emitter_simple_query() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_gt("p", "age", 21)
        .r#return()
        .field("p", "name", Some("actor"))
        .limit(10);

    let (arena, root) = builder.build();
    let mut emitter = CypherEmitter::new();
    let compiled = emitter
        .visit_query(&arena, root)
        .expect("Cypher emission failed");

    assert_eq!(
        compiled.statement,
        "MATCH (p:Person) WHERE p.age > $p0 RETURN p.name AS actor LIMIT 10"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::Int64(21))
    );
}

#[test]
fn test_cypher_emitter_multi_hop_traversal() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["ACTED_IN"], Some("r"))
        .hops(1, 2)
        .node(Some("m"), vec!["Movie"])
        .from(vec!["DIRECTED"], Some("d_rel"))
        .node(Some("d"), vec!["Director"])
        .where_contains("p", "name", "Keanu")
        .r#return()
        .field("p", "name", Some("actor"))
        .field("m", "title", Some("movie"))
        .field("d", "name", Some("director"))
        .order_by_asc("p", "name")
        .limit(5);

    let (arena, root) = builder.build();
    let mut emitter = CypherEmitter::new();
    let compiled = emitter
        .visit_query(&arena, root)
        .expect("Cypher emission failed");

    assert_eq!(
        compiled.statement,
        "MATCH (p:Person)-[r:ACTED_IN*1..2]->(m:Movie)<-[d_rel:DIRECTED]-(d:Director) WHERE p.name CONTAINS $p0 RETURN p.name AS actor, m.title AS movie, d.name AS director ORDER BY p.name ASC LIMIT 5"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::String("Keanu".into()))
    );
}

#[test]
fn test_sql_pgq_emitter_graph_table() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["ACTED_IN"], Some("r"))
        .node(Some("m"), vec!["Movie"])
        .where_gt("p", "age", 25)
        .r#return()
        .field("p", "name", Some("actor"))
        .field("m", "title", Some("movie"))
        .order_by_asc("p", "name")
        .limit(10);

    let (arena, root) = builder.build();
    let mut emitter = SqlPgqEmitter::new("movies_graph");
    let compiled = emitter
        .visit_query(&arena, root)
        .expect("SQL:PGQ emission failed");

    assert_eq!(
        compiled.statement,
        "SELECT * FROM GRAPH_TABLE (movies_graph MATCH (p IS Person) -[r IS ACTED_IN]-> (m IS Movie) WHERE p.age > $p0 COLUMNS (p.name AS actor, m.title AS movie)) ORDER BY p.name ASC LIMIT 10"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::Int64(25))
    );
}

#[test]
fn test_sql_pgq_emitter_like_translation() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_contains("p", "name", "Matrix")
        .r#return()
        .field("p", "name", Some("name"));

    let (arena, root) = builder.build();
    let mut emitter = SqlPgqEmitter::new("social_network");
    let compiled = emitter
        .visit_query(&arena, root)
        .expect("SQL:PGQ emission failed");

    assert_eq!(
        compiled.statement,
        "SELECT * FROM GRAPH_TABLE (social_network MATCH (p IS Person) WHERE p.name LIKE '%' || $p0 || '%' COLUMNS (p.name AS name))"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::String("Matrix".into()))
    );
}

#[test]
fn test_iso_gql_emitter_standard() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person", "Actor"])
        .to(vec!["ACTED_IN"], Some("r"))
        .node(Some("m"), vec!["Movie"])
        .where_eq("m", "released", 1999)
        .r#return()
        .field("p", "name", Some("actor"))
        .field("m", "title", Some("movie"))
        .order_by_desc("m", "released")
        .limit(15);

    let (arena, root) = builder.build();
    let mut emitter = IsoGqlEmitter::new();
    let compiled = emitter
        .visit_query(&arena, root)
        .expect("ISO GQL emission failed");

    assert_eq!(
        compiled.statement,
        "MATCH (p:Person&Actor)-[r:ACTED_IN]->(m:Movie) WHERE m.released = $p0 RETURN p.name AS actor, m.title AS movie ORDER BY m.released DESC LIMIT 15"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::Int64(1999))
    );
}

#[test]
fn test_cypher_emitter_procedure_call() {
    let mut arena = QueryAstArena::new();
    let arg = arena.alloc(AstNode::Literal(LiteralValue::String("Person".into())));

    let proc_handle = arena.alloc(AstNode::ProcedureCall {
        namespace: Some("apoc.path".into()),
        procedure: "subgraphNodes".into(),
        arguments: vec![arg],
        yield_items: vec!["node".into()],
    });

    let mut emitter = CypherEmitter::new();
    let compiled = emitter
        .visit_query(&arena, proc_handle)
        .expect("Procedure call emission failed");

    assert_eq!(
        compiled.statement,
        "CALL apoc.path.subgraphNodes($p0) YIELD node"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::String("Person".into()))
    );
}
