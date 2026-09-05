//! Unit tests for bulk ingestion query generation and UNWIND clause handling.

use voyager_core::builder::QueryBuilder;
use voyager_core::bulk::{compile_bulk_create, compile_bulk_create_rel, compile_bulk_merge};
use voyager_core::emitters::CypherEmitter;
use voyager_core::visitor::AstVisitor;

#[test]
fn test_unwind_param_fluent_builder() {
    let mut builder = QueryBuilder::new();
    let row_name = builder.prop("row", "name");
    let row_age = builder.prop("row", "age");

    builder
        .unwind_param("batch", "row")
        .create()
        .node(Some("p"), vec!["Person"])
        .set_property_expr("p", "name", row_name)
        .set_property_expr("p", "age", row_age);

    let (arena, root) = builder.build();

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "UNWIND $batch AS row CREATE (p:Person) SET p.name = row.name, p.age = row.age"
    );
}

#[test]
fn test_compile_bulk_create_cypher_and_gql() {
    let props = ["name", "age", "email", "active"];
    let cypher_query = compile_bulk_create("User", &props, "batch", "row", "cypher").unwrap();
    assert_eq!(
        cypher_query.statement,
        "UNWIND $batch AS row CREATE (_user_0:User) SET _user_0.name = row.name, _user_0.age = row.age, _user_0.email = row.email, _user_0.active = row.active"
    );

    let gql_query = compile_bulk_create("User", &props, "batch", "row", "iso_gql").unwrap();
    assert_eq!(
        gql_query.statement,
        "UNWIND $batch AS row INSERT (_user_0:User) SET _user_0.name = row.name, _user_0.age = row.age, _user_0.email = row.email, _user_0.active = row.active"
    );
}

#[test]
fn test_compile_bulk_merge_cypher_and_gql() {
    let props = ["name", "age", "updated_at"];
    let cypher_query =
        compile_bulk_merge("Person", "id", &props, "batch", "row", "cypher").unwrap();
    assert_eq!(
        cypher_query.statement,
        "UNWIND $batch AS row MERGE (_person_0:Person {id: row.id}) ON CREATE SET _person_0.name = row.name, _person_0.age = row.age, _person_0.updated_at = row.updated_at ON MATCH SET _person_0.name = row.name, _person_0.age = row.age, _person_0.updated_at = row.updated_at"
    );

    let gql_query = compile_bulk_merge("Person", "id", &props, "batch", "row", "iso_gql").unwrap();
    assert_eq!(
        gql_query.statement,
        "UNWIND $batch AS row UPSERT (_person_0:Person {id: row.id}) SET _person_0.name = row.name, _person_0.age = row.age, _person_0.updated_at = row.updated_at, _person_0.name = row.name, _person_0.age = row.age, _person_0.updated_at = row.updated_at"
    );
}

#[test]
fn test_compile_bulk_create_relationships() {
    let props = ["since", "weight"];
    let query = compile_bulk_create_rel(
        "KNOWS", &props, "Person", "user_id", "Person", "user_id", "batch", "row", "cypher",
    )
    .unwrap();

    assert_eq!(
        query.statement,
        "UNWIND $batch AS row MATCH (_from_person_0:Person {user_id: row.from_user_id}), (_to_person_0:Person {user_id: row.to_user_id}) CREATE (_from_person_0)-[_knows_0:KNOWS]->(_to_person_0) SET _knows_0.since = row.since, _knows_0.weight = row.weight"
    );
}
