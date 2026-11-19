//! Snapshot & Conformance Tests for Multi-Dialect DDL Migration Emitters.
//!
//! Asserts deterministic query emission across openCypher (Neo4j / Memgraph),
//! ISO GQL, and SQL:2023 PGQ / DuckPGQ directly from SchemaRegistry metadata.

use voyager_ogm::schema::{
    FieldDescriptor, FieldType, IndexType, NodeSchema, RelationshipSchema, SchemaRegistry,
};
use voyager_ogm::{
    emit_cypher_drop_node_ddl, emit_cypher_drop_rel_ddl, emit_cypher_node_ddl, emit_cypher_rel_ddl,
    emit_cypher25_drop_graph_type_ddl, emit_cypher25_graph_type_ddl, emit_gql_alter_node_ddl,
    emit_gql_alter_rel_ddl, emit_gql_drop_graph_type_ddl, emit_gql_graph_type_ddl,
    emit_pgq_drop_property_graph_ddl, emit_pgq_property_graph_ddl,
};

fn create_test_node_schema() -> NodeSchema {
    NodeSchema::new("User", vec!["User".to_string(), "Account".to_string()])
        .with_field(FieldDescriptor::new("user_id", FieldType::String).primary_key())
        .with_field(FieldDescriptor::new("email", FieldType::String).unique())
        .with_field(FieldDescriptor::new("age", FieldType::Int64).indexed())
        .with_field(FieldDescriptor::new("bio", FieldType::String).with_index_type(IndexType::Text))
}

fn create_test_rel_schema() -> RelationshipSchema {
    RelationshipSchema::new("Follows", "FOLLOWS")
        .with_endpoints(vec!["User".to_string()], vec!["User".to_string()])
        .with_field(FieldDescriptor::new("since", FieldType::Int64).nullable(false))
        .with_field(FieldDescriptor::new("role", FieldType::String).indexed())
}

#[test]
fn test_cypher_node_ddl_and_drop() {
    let node = create_test_node_schema();

    // 1. DDL generation without property type constraints (Community Edition)
    let ddl = emit_cypher_node_ddl(&node, false);
    assert_eq!(ddl.len(), 5);
    assert_eq!(
        ddl[0],
        "CREATE INDEX index_user_age IF NOT EXISTS FOR (n:User) ON (n.age)"
    );
    assert_eq!(
        ddl[1],
        "CREATE TEXT INDEX index_user_bio IF NOT EXISTS FOR (n:User) ON (n.bio)"
    );
    assert_eq!(
        ddl[2],
        "CREATE CONSTRAINT constraint_user_email_unique IF NOT EXISTS FOR (n:User) REQUIRE n.email IS UNIQUE"
    );
    assert_eq!(
        ddl[3],
        "CREATE CONSTRAINT constraint_user_user_id_unique IF NOT EXISTS FOR (n:User) REQUIRE n.user_id IS UNIQUE"
    );
    assert_eq!(
        ddl[4],
        "CREATE CONSTRAINT constraint_user_user_id_not_null IF NOT EXISTS FOR (n:User) REQUIRE n.user_id IS NOT NULL"
    );

    // 2. DDL generation with Neo4j 5.x property type constraints (Enterprise Edition)
    let enterprise_ddl = emit_cypher_node_ddl(&node, true);
    assert_eq!(enterprise_ddl.len(), 9);
    assert!(enterprise_ddl.contains(
        &"CREATE CONSTRAINT constraint_user_user_id_type IF NOT EXISTS FOR (n:User) REQUIRE n.user_id :: STRING".to_string()
    ));
    assert!(enterprise_ddl.contains(
        &"CREATE CONSTRAINT constraint_user_email_type IF NOT EXISTS FOR (n:User) REQUIRE n.email :: STRING".to_string()
    ));
    assert!(enterprise_ddl.contains(
        &"CREATE CONSTRAINT constraint_user_age_type IF NOT EXISTS FOR (n:User) REQUIRE n.age :: INTEGER".to_string()
    ));

    // 3. DROP DDL generation
    let drop_ddl = emit_cypher_drop_node_ddl(&node, true);
    assert_eq!(drop_ddl.len(), 9);
    assert!(
        drop_ddl
            .contains(&"DROP CONSTRAINT constraint_user_user_id_not_null IF EXISTS".to_string())
    );
    assert!(
        drop_ddl.contains(&"DROP CONSTRAINT constraint_user_user_id_unique IF EXISTS".to_string())
    );
    assert!(
        drop_ddl.contains(&"DROP CONSTRAINT constraint_user_email_unique IF EXISTS".to_string())
    );
    assert!(drop_ddl.contains(&"DROP INDEX index_user_age IF EXISTS".to_string()));
    assert!(drop_ddl.contains(&"DROP INDEX index_user_bio IF EXISTS".to_string()));
    assert!(
        drop_ddl.contains(&"DROP CONSTRAINT constraint_user_user_id_type IF EXISTS".to_string())
    );
}

#[test]
fn test_cypher_rel_ddl_and_drop() {
    let rel = create_test_rel_schema();

    // 1. DDL generation
    let ddl = emit_cypher_rel_ddl(&rel, false);
    assert_eq!(ddl.len(), 2);
    assert_eq!(
        ddl[0],
        "CREATE INDEX index_rel_follows_role IF NOT EXISTS FOR ()-[r:FOLLOWS]-() ON (r.role)"
    );
    assert_eq!(
        ddl[1],
        "CREATE CONSTRAINT constraint_rel_follows_since_not_null IF NOT EXISTS FOR ()-[r:FOLLOWS]-() REQUIRE r.since IS NOT NULL"
    );

    // 2. Enterprise type constraints
    let ent_ddl = emit_cypher_rel_ddl(&rel, true);
    assert_eq!(ent_ddl.len(), 4);
    assert!(ent_ddl.contains(
        &"CREATE CONSTRAINT constraint_rel_follows_since_type IF NOT EXISTS FOR ()-[r:FOLLOWS]-() REQUIRE r.since :: INTEGER".to_string()
    ));

    // 3. DROP statements
    let drop_ddl = emit_cypher_drop_rel_ddl(&rel, true);
    assert_eq!(drop_ddl.len(), 4);
    assert!(
        drop_ddl.contains(
            &"DROP CONSTRAINT constraint_rel_follows_since_not_null IF EXISTS".to_string()
        )
    );
    assert!(drop_ddl.contains(&"DROP INDEX index_rel_follows_role IF EXISTS".to_string()));
    assert!(
        drop_ddl
            .contains(&"DROP CONSTRAINT constraint_rel_follows_since_type IF EXISTS".to_string())
    );
}

#[test]
fn test_cypher_rel_unique_and_primary_key_constraints() {
    // Unique fields get a uniqueness constraint; existence is emitted only when
    // the field is actually non-nullable (a nullable unique field stays optional).
    let rel = RelationshipSchema::new("Transfer", "TRANSFER")
        .with_endpoints(vec!["Account".to_string()], vec!["Account".to_string()])
        .with_field(FieldDescriptor::new("code", FieldType::String).nullable(false))
        .with_field(FieldDescriptor::new("ref", FieldType::String).unique())
        .with_field(
            FieldDescriptor::new("slug", FieldType::String)
                .unique()
                .indexed(),
        )
        .with_field(
            FieldDescriptor::new("token", FieldType::String)
                .unique()
                .nullable(false),
        )
        .with_field(FieldDescriptor::new("transfer_id", FieldType::String).primary_key());

    let ddl = emit_cypher_rel_ddl(&rel, false);
    assert_eq!(
        ddl,
        vec![
            "CREATE CONSTRAINT constraint_rel_transfer_code_not_null IF NOT EXISTS FOR ()-[r:TRANSFER]-() REQUIRE r.code IS NOT NULL",
            "CREATE CONSTRAINT constraint_rel_transfer_ref_unique IF NOT EXISTS FOR ()-[r:TRANSFER]-() REQUIRE r.ref IS UNIQUE",
            "CREATE CONSTRAINT constraint_rel_transfer_slug_unique IF NOT EXISTS FOR ()-[r:TRANSFER]-() REQUIRE r.slug IS UNIQUE",
            "CREATE CONSTRAINT constraint_rel_transfer_token_unique IF NOT EXISTS FOR ()-[r:TRANSFER]-() REQUIRE r.token IS UNIQUE",
            "CREATE CONSTRAINT constraint_rel_transfer_token_not_null IF NOT EXISTS FOR ()-[r:TRANSFER]-() REQUIRE r.token IS NOT NULL",
            "CREATE CONSTRAINT constraint_rel_transfer_transfer_id_unique IF NOT EXISTS FOR ()-[r:TRANSFER]-() REQUIRE r.transfer_id IS UNIQUE",
            "CREATE CONSTRAINT constraint_rel_transfer_transfer_id_not_null IF NOT EXISTS FOR ()-[r:TRANSFER]-() REQUIRE r.transfer_id IS NOT NULL",
        ]
    );

    // The uniqueness constraint brings its own index, so no secondary index is emitted.
    assert!(!ddl.iter().any(|s| s.contains("CREATE INDEX")));

    let drop_ddl = emit_cypher_drop_rel_ddl(&rel, false);
    assert_eq!(
        drop_ddl,
        vec![
            "DROP CONSTRAINT constraint_rel_transfer_code_not_null IF EXISTS",
            "DROP CONSTRAINT constraint_rel_transfer_ref_unique IF EXISTS",
            "DROP CONSTRAINT constraint_rel_transfer_slug_unique IF EXISTS",
            "DROP CONSTRAINT constraint_rel_transfer_token_unique IF EXISTS",
            "DROP CONSTRAINT constraint_rel_transfer_token_not_null IF EXISTS",
            "DROP CONSTRAINT constraint_rel_transfer_transfer_id_unique IF EXISTS",
            "DROP CONSTRAINT constraint_rel_transfer_transfer_id_not_null IF EXISTS",
        ]
    );

    // The per-model constraint/drop-constraint emitters agree with the aggregate ones.
    assert_eq!(
        voyager_ogm::emit_rel_constraint_ddl(&rel, "cypher", false).unwrap(),
        ddl
    );
    assert_eq!(
        voyager_ogm::emit_rel_drop_constraint_ddl(&rel, "cypher", false).unwrap(),
        drop_ddl
    );
}

#[test]
fn test_cypher_unique_constraints_emitted_before_not_null() {
    // Community Edition accepts uniqueness constraints but rejects existence
    // constraints, so the UNIQUE statement must run first.
    let node = NodeSchema::new("Ordered", vec!["Ordered".to_string()])
        .with_field(FieldDescriptor::new("id", FieldType::String).primary_key());
    let node_ddl = emit_cypher_node_ddl(&node, false);
    let node_unique = node_ddl
        .iter()
        .position(|s| s.contains("IS UNIQUE"))
        .expect("node UNIQUE constraint");
    let node_not_null = node_ddl
        .iter()
        .position(|s| s.contains("IS NOT NULL"))
        .expect("node NOT NULL constraint");
    assert!(node_unique < node_not_null);

    let rel = RelationshipSchema::new("OrderedRel", "ORDERED_REL")
        .with_endpoints(vec!["Ordered".to_string()], vec!["Ordered".to_string()])
        .with_field(FieldDescriptor::new("id", FieldType::String).primary_key());
    let rel_ddl = emit_cypher_rel_ddl(&rel, false);
    let rel_unique = rel_ddl
        .iter()
        .position(|s| s.contains("IS UNIQUE"))
        .expect("relationship UNIQUE constraint");
    let rel_not_null = rel_ddl
        .iter()
        .position(|s| s.contains("IS NOT NULL"))
        .expect("relationship NOT NULL constraint");
    assert!(rel_unique < rel_not_null);

    // Constraint-only emitters follow the same ordering.
    assert_eq!(
        voyager_ogm::emit_node_constraint_ddl(&node, "cypher", false).unwrap(),
        node_ddl
    );
    assert_eq!(
        voyager_ogm::emit_rel_constraint_ddl(&rel, "cypher", false).unwrap(),
        rel_ddl
    );
}

#[test]
fn test_cypher25_graph_type_ddl() {
    let node = create_test_node_schema();
    let rel = create_test_rel_schema();

    let ddl = emit_cypher25_graph_type_ddl(&[&node], &[&rel]);
    let expected = "\
ALTER CURRENT GRAPH TYPE SET {
    (:User => :Account {age :: INTEGER, bio :: STRING, email :: STRING IS UNIQUE, user_id :: STRING IS KEY}),
    (:User)-[:FOLLOWS => {role :: STRING, since :: INTEGER NOT NULL}]->(:User)
}";
    assert_eq!(ddl, expected);
    assert_eq!(
        emit_cypher25_drop_graph_type_ddl(),
        "ALTER CURRENT GRAPH TYPE SET {}"
    );
}

#[test]
fn test_gql_create_graph_type_ddl() {
    let node = create_test_node_schema();
    let rel = create_test_rel_schema();

    let ddl = emit_gql_graph_type_ddl("SocialGraph", &[&node], &[&rel]);
    let expected = "\
CREATE GRAPH TYPE SocialGraph AS {
    NODE User (age INTEGER, bio STRING, email STRING, user_id STRING NOT NULL) KEY (user_id),
    EDGE FOLLOWS CONNECTING (User TO User) (role STRING, since INTEGER NOT NULL)
}";
    assert_eq!(ddl, expected);
}

#[test]
fn test_gql_alter_and_drop_ddl() {
    let node = create_test_node_schema();
    let rel = create_test_rel_schema();

    let node_alter = emit_gql_alter_node_ddl(&node);
    assert_eq!(
        node_alter,
        "ALTER CURRENT GRAPH TYPE ADD NODE TYPE (:User {age :: INTEGER?, bio :: STRING?, email :: STRING, user_id :: STRING})"
    );

    let rel_alter = emit_gql_alter_rel_ddl(&rel, Some("User"), Some("User"));
    assert_eq!(
        rel_alter,
        "ALTER CURRENT GRAPH TYPE ADD RELATIONSHIP TYPE (:User)-[:FOLLOWS {role :: STRING, since :: INTEGER}]->(:User)"
    );

    let drop_ddl = emit_gql_drop_graph_type_ddl("SocialGraph");
    assert_eq!(drop_ddl, "DROP GRAPH TYPE SocialGraph IF EXISTS");
}

#[test]
fn test_pgq_property_graph_ddl() {
    let node = create_test_node_schema();
    let rel = create_test_rel_schema();

    let ddl = emit_pgq_property_graph_ddl("social_graph", &[&node], &[&rel]).unwrap();
    let expected = "\
CREATE PROPERTY GRAPH social_graph
  VERTEX TABLES (
    user KEY (user_id) LABEL User PROPERTIES (age, bio, email, user_id)
  )
  EDGE TABLES (
    follows
      SOURCE KEY (user_id) REFERENCES user (user_id)
      DESTINATION KEY (user_id) REFERENCES user (user_id)
      LABEL FOLLOWS PROPERTIES (role, since)
  );";
    assert_eq!(ddl, expected);

    let drop_ddl = emit_pgq_drop_property_graph_ddl("social_graph");
    assert_eq!(drop_ddl, "DROP PROPERTY GRAPH IF EXISTS social_graph;");
}

#[test]
fn test_pgq_missing_endpoints_error() {
    let node = create_test_node_schema();
    let rel_no_endpoints = RelationshipSchema::new("follows", "FOLLOWS");
    let err = emit_pgq_property_graph_ddl("social_graph", &[&node], &[&rel_no_endpoints]);
    assert!(err.is_err());
    assert!(
        err.unwrap_err()
            .to_string()
            .contains("missing source_labels")
    );
}

#[test]
fn test_cypher25_empty_and_composite_facets() {
    // 1. Empty fields node and relationship have no dangling =>
    let empty_node = NodeSchema::new("Empty", vec!["Empty".to_string()]);
    let empty_rel = RelationshipSchema::new("EmptyRel", "EMPTY_REL")
        .with_endpoints(vec!["Empty".to_string()], vec!["Empty".to_string()]);
    let ddl = emit_cypher25_graph_type_ddl(&[&empty_node], &[&empty_rel]);
    assert!(ddl.contains("(:Empty)"));
    assert!(!ddl.contains("(:Empty =>)"));
    assert!(ddl.contains("(:Empty)-[:EMPTY_REL]->(:Empty)"));
    assert!(!ddl.contains("[:EMPTY_REL =>]"));

    // 2. Composed NOT NULL and IS UNIQUE facets
    let composite_node = NodeSchema::new("Member", vec!["Member".to_string()]).with_field(
        FieldDescriptor::new("email", FieldType::String)
            .unique()
            .nullable(false),
    );
    let composite_ddl = emit_cypher25_graph_type_ddl(&[&composite_node], &[]);
    assert!(composite_ddl.contains("email :: STRING NOT NULL IS UNIQUE"));
}

#[test]
fn test_schema_registry_ddl_methods() {
    let registry = SchemaRegistry::new();
    let node = create_test_node_schema();
    let rel = create_test_rel_schema();

    registry.register_node(node).unwrap();
    registry.register_relationship(rel).unwrap();

    let cypher_ddl = registry.generate_cypher_ddl(false);
    assert_eq!(cypher_ddl.len(), 7);

    let node_cypher = registry.generate_node_cypher_ddl("User", false);
    assert!(node_cypher.is_some());
    assert_eq!(node_cypher.unwrap().len(), 5);

    let rel_cypher = registry.generate_rel_cypher_ddl("Follows", false);
    assert!(rel_cypher.is_some());
    assert_eq!(rel_cypher.unwrap().len(), 2);

    let cypher25_ddl = registry.generate_cypher25_graph_type_ddl();
    assert!(cypher25_ddl.starts_with("ALTER CURRENT GRAPH TYPE SET {"));
    assert_eq!(
        registry.generate_cypher25_drop_graph_type_ddl(),
        "ALTER CURRENT GRAPH TYPE SET {}"
    );

    let gql_ddl = registry.generate_gql_graph_type_ddl("NetworkGraph");
    assert!(gql_ddl.starts_with("CREATE GRAPH TYPE NetworkGraph AS {"));

    let pgq_ddl = registry.generate_pgq_ddl("network_pgq").unwrap();
    assert!(pgq_ddl.starts_with("CREATE PROPERTY GRAPH network_pgq"));
}

#[test]
fn test_unsupported_constraint_ddl_error() {
    let node = create_test_node_schema();
    let rel = create_test_rel_schema();

    assert!(voyager_ogm::emit_node_constraint_ddl(&node, "age", false).is_err());
    assert!(voyager_ogm::emit_node_constraint_ddl(&node, "falkordb", false).is_err());
    assert!(voyager_ogm::emit_rel_constraint_ddl(&rel, "age", false).is_err());
    assert!(voyager_ogm::emit_rel_constraint_ddl(&rel, "falkordb", false).is_err());
    assert!(voyager_ogm::emit_node_drop_constraint_ddl(&node, "age", false).is_err());
    assert!(voyager_ogm::emit_node_drop_constraint_ddl(&node, "falkordb", false).is_err());
    assert!(voyager_ogm::emit_rel_drop_constraint_ddl(&rel, "age", false).is_err());
    assert!(voyager_ogm::emit_rel_drop_constraint_ddl(&rel, "falkordb", false).is_err());
}
