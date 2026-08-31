//! Unit and snapshot tests for DML Mutations (CREATE, MERGE, SET, DELETE, REMOVE).

use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{CypherEmitter, IsoGqlEmitter};
use voyager_core::visitor::AstVisitor;

#[test]
fn test_create_single_node_mutation() {
    let mut builder = QueryBuilder::new();
    builder.create().node(Some("p"), vec!["Person"]);

    let (arena, root) = builder.build();

    // 1. Cypher Emission
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(res.statement, "CREATE (p:Person)");

    // 2. ISO GQL Emission
    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(res_gql.statement, "INSERT (p:Person)");
}

#[test]
fn test_create_relationship_path_mutation() {
    let mut builder = QueryBuilder::new();
    builder
        .create()
        .node(Some("a"), vec!["Person"])
        .to(vec!["FOLLOWS"], Some("r"))
        .node(Some("b"), vec!["Person"]);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(res.statement, "CREATE (a:Person)-[r:FOLLOWS]->(b:Person)");

    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_gql.statement,
        "INSERT (a:Person)-[r:FOLLOWS]->(b:Person)"
    );
}

#[test]
fn test_merge_upsert_with_on_create_and_on_match_set() {
    let mut builder = QueryBuilder::new();
    builder
        .merge()
        .node(Some("p"), vec!["User"])
        .on_create_set("p", "created_at", 2026)
        .on_create_set("p", "status", "ACTIVE")
        .on_match_set("p", "updated_at", 2026);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MERGE (p:User) ON CREATE SET p.created_at = $p0, p.status = $p1 ON MATCH SET p.updated_at = $p2"
    );
    assert_eq!(res.parameters.len(), 3);

    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_gql.statement,
        "UPSERT (p:User) SET p.created_at = $p0, p.status = $p1, p.updated_at = $p2"
    );
}

#[test]
fn test_match_then_set_property_mutation() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_eq("p", "id", 101)
        .set_property("p", "status", "VERIFIED")
        .set_property("p", "verified_year", 2026)
        .r#return()
        .field("p", "name", None::<&str>);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (p:Person) WHERE p.id = $p0 SET p.status = $p1, p.verified_year = $p2 RETURN p.name"
    );
    assert_eq!(
        res.parameters.get("p0").unwrap(),
        &voyager_core::ast::LiteralValue::Int64(101)
    );
    assert_eq!(
        res.parameters.get("p1").unwrap(),
        &voyager_core::ast::LiteralValue::String("VERIFIED".into())
    );
}

#[test]
fn test_detach_delete_entity_mutation() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_eq("p", "banned", true)
        .detach_delete(vec!["p"]);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (p:Person) WHERE p.banned = $p0 DETACH DELETE p"
    );
}

#[test]
fn test_remove_property_mutation() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["User"])
        .where_eq("p", "id", 55)
        .remove_property("p", "temporary_token");

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (p:User) WHERE p.id = $p0 REMOVE p.temporary_token"
    );
}
