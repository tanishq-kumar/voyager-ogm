//! Unit and snapshot tests for DML Mutations (CREATE, MERGE, SET, DELETE, REMOVE).

use voyager_ogm::ast::{Direction, LiteralValue};
use voyager_ogm::builder::{MergeRelationshipProperties, QueryBuilder};
use voyager_ogm::emitters::{AgeEmitter, CypherEmitter, IsoGqlEmitter};
use voyager_ogm::visitor::AstVisitor;

fn match_two_people(builder: &mut QueryBuilder) {
    builder
        .r#match()
        .node(Some("a"), vec!["Person"])
        .where_eq("a", "id", 1i64)
        .r#match()
        .node(Some("b"), vec!["Person"])
        .where_eq("b", "id", 2i64);
}

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
    match gql.visit_query(&arena, root).unwrap_err() {
        voyager_ogm::Error::UnsupportedFeature { dialect, feature } => {
            assert_eq!(dialect, "iso_gql");
            assert!(feature.contains("ON CREATE SET / ON MATCH SET"));
        }
        other => panic!("Expected UnsupportedFeature, got {other:?}"),
    }
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
        &voyager_ogm::ast::LiteralValue::Int64(101)
    );
    assert_eq!(
        res.parameters.get("p1").unwrap(),
        &voyager_ogm::ast::LiteralValue::String("VERIFIED".into())
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

#[test]
fn test_create_node_with_props() {
    let mut builder = QueryBuilder::new();
    builder.create().node_with_props(
        Some("p"),
        vec!["Person"],
        vec![
            ("id", voyager_ogm::ast::LiteralValue::Int64(1)),
            ("name", voyager_ogm::ast::LiteralValue::String("Ada".into())),
        ],
    );

    let (arena, root) = builder.try_build().expect("build failed");

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(res.statement, "CREATE (p:Person {id: $p0, name: $p1})");

    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(res_gql.statement, "INSERT (p:Person {id: $p0, name: $p1})");
}

#[test]
fn test_create_path_with_props() {
    let mut builder = QueryBuilder::new();
    builder
        .create()
        .node_with_props(
            Some("a"),
            vec!["Person"],
            vec![("name", voyager_ogm::ast::LiteralValue::String("Ada".into()))],
        )
        .to_with_props(
            vec!["TX"],
            Some("t"),
            vec![("amount", voyager_ogm::ast::LiteralValue::Float64(750.0))],
        )
        .node_with_props(
            Some("b"),
            vec!["Person"],
            vec![("name", voyager_ogm::ast::LiteralValue::String("Bob".into()))],
        );

    let (arena, root) = builder.try_build().expect("build failed");

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "CREATE (a:Person {name: $p0})-[t:TX {amount: $p1}]->(b:Person {name: $p2})"
    );
}

#[test]
fn test_create_path_with_prop_exprs_uses_parameters_and_literals() {
    let mut builder = QueryBuilder::new();
    builder.create();
    let email = builder.param("email");
    let name = builder.literal(LiteralValue::String("Alice".into()));
    builder.node_with_prop_exprs(
        Some("a"),
        vec!["Person"],
        vec![("email", email), ("name", name)],
    );

    let (arena, root) = builder.try_build().expect("build failed");
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "CREATE (a:Person {email: $email, name: $p0})"
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::String("Alice".into()))
    );
    assert_eq!(res.parameters.len(), 1);
}

#[test]
fn test_match_edge_with_prop_exprs_keeps_hops() {
    let mut builder = QueryBuilder::new();
    builder.r#match();
    builder.node(Some("a"), vec!["Person"]);
    let since = builder.literal(LiteralValue::Int64(2020));
    builder.to_with_prop_exprs(vec!["KNOWS"], Some("r"), vec![("since", since)]);
    builder.node(Some("b"), vec!["Person"]);
    builder.hops(1, 3);
    builder.field("b", "name", Some("n"));

    let (arena, root) = builder.try_build().expect("build failed");
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (a:Person)-[r:KNOWS*1..3 {since: $p0}]->(b:Person) RETURN b.name AS n"
    );
}

#[test]
fn test_incoming_and_undirected_prop_expr_edges() {
    let mut incoming = QueryBuilder::new();
    incoming.r#match();
    incoming.node(Some("b"), vec!["Person"]);
    let since = incoming.literal(LiteralValue::Int64(2020));
    incoming.from_with_prop_exprs(vec!["KNOWS"], Some("r"), vec![("since", since)]);
    incoming.node(Some("a"), vec!["Person"]);
    let (arena, root) = incoming.try_build().expect("build failed");
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (b:Person)<-[r:KNOWS {since: $p0}]-(a:Person)"
    );

    let mut undirected = QueryBuilder::new();
    undirected.r#match();
    undirected.node(Some("a"), vec!["Person"]);
    let since = undirected.literal(LiteralValue::Int64(2020));
    undirected.edge_with_prop_exprs(vec!["KNOWS"], Some("r"), vec![("since", since)]);
    undirected.node(Some("b"), vec!["Person"]);
    let (arena, root) = undirected.try_build().expect("build failed");
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (a:Person)-[r:KNOWS {since: $p0}]-(b:Person)"
    );
}

#[test]
fn test_prop_expr_variants_match_literal_variants_byte_for_byte() {
    let mut literal = QueryBuilder::new();
    literal.create().node_with_props(
        Some("a"),
        vec!["Person"],
        vec![("name", LiteralValue::String("Alice".into()))],
    );
    literal.to_with_props(
        vec!["KNOWS"],
        Some("r"),
        vec![("since", LiteralValue::Int64(2020))],
    );
    literal.node_with_props(
        Some("b"),
        vec!["Person"],
        vec![("name", LiteralValue::String("Bob".into()))],
    );

    let mut exprs = QueryBuilder::new();
    exprs.create();
    let name = exprs.literal(LiteralValue::String("Alice".into()));
    exprs.node_with_prop_exprs(Some("a"), vec!["Person"], vec![("name", name)]);
    let since = exprs.literal(LiteralValue::Int64(2020));
    exprs.to_with_prop_exprs(vec!["KNOWS"], Some("r"), vec![("since", since)]);
    let name = exprs.literal(LiteralValue::String("Bob".into()));
    exprs.node_with_prop_exprs(Some("b"), vec!["Person"], vec![("name", name)]);

    let (literal_arena, literal_root) = literal.try_build().expect("build failed");
    let (expr_arena, expr_root) = exprs.try_build().expect("build failed");
    let mut cypher = CypherEmitter::new();
    let literal_res = cypher.visit_query(&literal_arena, literal_root).unwrap();
    let mut cypher = CypherEmitter::new();
    let expr_res = cypher.visit_query(&expr_arena, expr_root).unwrap();
    assert_eq!(literal_res.statement, expr_res.statement);
    assert_eq!(literal_res.parameters, expr_res.parameters);
}

#[test]
fn test_merge_with_on_create_and_match_set_dedup() {
    let mut builder = QueryBuilder::new();
    builder
        .merge()
        .node_with_props(
            Some("p"),
            vec!["Person"],
            vec![("id", voyager_ogm::ast::LiteralValue::Int64(1))],
        )
        .on_create_and_match_set("p", "name", "Ada")
        .on_create_and_match_set("p", "age", 36i64);

    let (arena, root) = builder.try_build().expect("build failed");

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MERGE (p:Person {id: $p0}) ON CREATE SET p.name = $p1, p.age = $p2 ON MATCH SET p.name = $p1, p.age = $p2"
    );
    assert_eq!(res.parameters.len(), 3);
}

#[test]
fn test_merge_fold_eq() {
    let mut builder = QueryBuilder::new();
    builder.merge().node(Some("p"), vec!["Person"]);
    builder
        .fold_merge_eq("p", "name", "Ada".into())
        .expect("fold failed");

    let (arena, root) = builder.try_build().expect("build failed");

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(res.statement, "MERGE (p:Person {name: $p0})");
}

#[test]
fn test_merge_leftover_where_error() {
    let mut builder = QueryBuilder::new();
    builder
        .merge()
        .node(Some("p"), vec!["Person"])
        .where_gt("p", "age", 30i64);

    let err = builder.try_build().unwrap_err();
    assert!(err.to_string().contains("MERGE"));
}

#[test]
fn test_create_relationship_between_matched_nodes_all_dialects() {
    let mut builder = QueryBuilder::new();
    match_two_people(&mut builder);
    builder
        .create_relationship(
            "a",
            "KNOWS",
            Direction::Outgoing,
            None::<String>,
            "b",
            vec![("since", LiteralValue::Int64(2024))],
        )
        .expect("relationship creation failed");

    let (arena, root) = builder.try_build().expect("build failed");

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (a:Person) WHERE a.id = $p0 MATCH (b:Person) WHERE b.id = $p1 CREATE (a)-[:KNOWS {since: $p2}]->(b)"
    );
    assert_eq!(res.parameters.get("p0"), Some(&LiteralValue::Int64(1)));
    assert_eq!(res.parameters.get("p1"), Some(&LiteralValue::Int64(2)));
    assert_eq!(res.parameters.get("p2"), Some(&LiteralValue::Int64(2024)));

    let mut gql = IsoGqlEmitter::new();
    let res_gql = gql.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_gql.statement,
        "MATCH (a:Person) WHERE a.id = $p0 MATCH (b:Person) WHERE b.id = $p1 INSERT (a)-[:KNOWS {since: $p2}]->(b)"
    );
    assert_eq!(
        res_gql.parameters.get("p2"),
        Some(&LiteralValue::Int64(2024))
    );

    let mut age = AgeEmitter::new("test_graph");
    let res_age = age.visit_query(&arena, root).unwrap();
    assert_eq!(
        res_age.statement,
        "SELECT * FROM cypher('test_graph', $$ MATCH (a:Person) WHERE a.id = $p0 MATCH (b:Person) WHERE b.id = $p1 CREATE (a)-[:KNOWS {since: $p2}]->(b) $$, %s) AS (result agtype)"
    );
    assert_eq!(
        res_age.parameters.get("p2"),
        Some(&LiteralValue::Int64(2024))
    );
}

#[test]
fn test_create_relationship_direction_and_variable_alias() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("a"), vec!["Person"])
        .r#match()
        .node(Some("b"), vec!["Person"])
        .create_relationship(
            "a",
            "KNOWS",
            Direction::Incoming,
            Some("r"),
            "b",
            vec![("since", LiteralValue::Int64(2024))],
        )
        .expect("relationship creation failed");

    let (arena, root) = builder.try_build().expect("build failed");
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (a:Person) MATCH (b:Person) CREATE (a)<-[r:KNOWS {since: $p0}]-(b)"
    );
    assert_eq!(res.parameters.get("p0"), Some(&LiteralValue::Int64(2024)));
}

#[test]
fn test_create_relationship_undirected_materializes_outgoing() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("a"), vec!["Person"])
        .r#match()
        .node(Some("b"), vec!["Person"])
        .create_relationship(
            "a",
            "KNOWS",
            Direction::Undirected,
            None::<String>,
            "b",
            Vec::<(&str, LiteralValue)>::new(),
        )
        .expect("relationship creation failed");

    let (arena, root) = builder.try_build().expect("build failed");
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (a:Person) MATCH (b:Person) CREATE (a)-[:KNOWS]->(b)"
    );
}

#[test]
fn test_create_relationship_rejects_unbound_endpoint() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("a"), vec!["Person"]);

    let err = builder
        .create_relationship(
            "a",
            "KNOWS",
            Direction::Outgoing,
            None::<String>,
            "missing",
            Vec::<(&str, LiteralValue)>::new(),
        )
        .expect_err("unbound target should fail");

    assert!(err.to_string().contains("missing"));
    assert!(err.to_string().contains("target"));
}

#[test]
fn test_create_relationship_allows_node_created_earlier_in_query() {
    let mut builder = QueryBuilder::new();
    builder.create().node(Some("a"), vec!["Person"]);
    builder
        .create_relationship(
            "a",
            "KNOWS",
            Direction::Outgoing,
            None::<String>,
            "a",
            Vec::<(&str, LiteralValue)>::new(),
        )
        .expect("relationship creation failed");

    let (arena, root) = builder.try_build().expect("build failed");
    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(res.statement, "CREATE (a:Person) CREATE (a)-[:KNOWS]->(a)");
}

#[test]
fn test_merge_relationship_with_key_and_set_properties() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("a"), vec!["Person"])
        .r#match()
        .node(Some("b"), vec!["Person"])
        .merge_relationship(
            "a",
            "RATED",
            Direction::Outgoing,
            Some("r"),
            "b",
            MergeRelationshipProperties {
                key: vec![("key".to_string(), LiteralValue::String("k1".into()))],
                shared: vec![("note".to_string(), LiteralValue::String("n".into()))],
                create_only: vec![("score".to_string(), LiteralValue::Int64(5))],
            },
        )
        .expect("relationship merge failed");

    let (arena, root) = builder.try_build().expect("build failed");

    let mut cypher = CypherEmitter::new();
    let res = cypher.visit_query(&arena, root).unwrap();
    assert_eq!(
        res.statement,
        "MATCH (a:Person) MATCH (b:Person) MERGE (a)-[r:RATED {key: $p0}]->(b) ON CREATE SET r.note = $p1, r.score = $p2 ON MATCH SET r.note = $p1"
    );
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::String("k1".into()))
    );
    assert_eq!(
        res.parameters.get("p1"),
        Some(&LiteralValue::String("n".into()))
    );
    assert_eq!(res.parameters.get("p2"), Some(&LiteralValue::Int64(5)));

    let mut gql = IsoGqlEmitter::new();
    match gql.visit_query(&arena, root).unwrap_err() {
        voyager_ogm::Error::UnsupportedFeature { dialect, feature } => {
            assert_eq!(dialect, "iso_gql");
            assert!(feature.contains("ON CREATE SET / ON MATCH SET"));
        }
        other => panic!("Expected UnsupportedFeature, got {other:?}"),
    }
}

#[test]
fn test_merge_relationship_requires_alias_for_set_properties() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("a"), vec!["Person"])
        .r#match()
        .node(Some("b"), vec!["Person"]);

    let err = builder
        .merge_relationship(
            "a",
            "RATED",
            Direction::Outgoing,
            None::<String>,
            "b",
            MergeRelationshipProperties {
                key: Vec::new(),
                shared: vec![("note".to_string(), LiteralValue::String("n".into()))],
                create_only: Vec::new(),
            },
        )
        .expect_err("set properties without alias should fail");

    assert!(err.to_string().contains("variable alias"));
}
