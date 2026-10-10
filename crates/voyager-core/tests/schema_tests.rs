use std::sync::Arc;
use std::thread;

use voyager_core::ast::LiteralValue;
use voyager_core::schema::{
    FieldDescriptor, FieldType, IndexType, NodeSchema, RelationshipSchema, SchemaRegistry,
    global_schema_registry,
};

#[test]
fn test_field_type_parsing_and_display() {
    assert_eq!(FieldType::parse_str("STRING"), FieldType::String);
    assert_eq!(FieldType::parse_str("text"), FieldType::String);
    assert_eq!(FieldType::parse_str("varchar"), FieldType::String);
    assert_eq!(FieldType::parse_str("int"), FieldType::Int64);
    assert_eq!(FieldType::parse_str("INTEGER"), FieldType::Int64);
    assert_eq!(FieldType::parse_str("bigint"), FieldType::Int64);
    assert_eq!(FieldType::parse_str("float"), FieldType::Float64);
    assert_eq!(FieldType::parse_str("double"), FieldType::Float64);
    assert_eq!(FieldType::parse_str("bool"), FieldType::Boolean);
    assert_eq!(FieldType::parse_str("boolean"), FieldType::Boolean);
    assert_eq!(FieldType::parse_str("datetime"), FieldType::DateTime);
    assert_eq!(FieldType::parse_str("timestamp"), FieldType::DateTime);
    assert_eq!(FieldType::parse_str("date"), FieldType::Date);
    assert_eq!(FieldType::parse_str("duration"), FieldType::Duration);
    assert_eq!(
        FieldType::parse_str("array"),
        FieldType::List(Box::new(FieldType::Any))
    );
    assert_eq!(FieldType::parse_str("map"), FieldType::Map);
    assert_eq!(FieldType::parse_str("any"), FieldType::Any);
    assert_eq!(
        FieldType::parse_str("GEOMETRY"),
        FieldType::Custom("GEOMETRY".to_string())
    );

    assert_eq!(FieldType::String.as_str(), "STRING");
    assert_eq!(FieldType::Int64.as_str(), "INTEGER");
    assert_eq!(FieldType::Float64.as_str(), "FLOAT");
    assert_eq!(FieldType::Boolean.as_str(), "BOOLEAN");
    assert_eq!(FieldType::DateTime.as_str(), "DATETIME");
    assert_eq!(FieldType::Date.as_str(), "DATE");
    assert_eq!(FieldType::Duration.as_str(), "DURATION");
    assert_eq!(format!("{}", FieldType::String), "STRING");
}

#[test]
fn test_field_descriptor_builder() {
    let id_field = FieldDescriptor::new("user_id", FieldType::String).primary_key();
    assert_eq!(id_field.name, "user_id");
    assert_eq!(id_field.field_type, FieldType::String);
    assert!(id_field.primary_key);
    assert!(id_field.unique);
    assert!(!id_field.nullable);

    let email_field = FieldDescriptor::new("email", FieldType::String)
        .unique()
        .nullable(false);
    assert!(email_field.unique);
    assert!(!email_field.nullable);
    assert!(!email_field.primary_key);

    let age_field = FieldDescriptor::new("age", FieldType::Int64)
        .indexed()
        .with_default(LiteralValue::Int64(0));
    assert!(age_field.indexed);
    assert_eq!(age_field.index_type, Some(IndexType::BTree));
    assert_eq!(age_field.default_value, Some(LiteralValue::Int64(0)));

    let bio_field = FieldDescriptor::new("bio", FieldType::String).with_index_type(IndexType::Text);
    assert!(bio_field.indexed);
    assert_eq!(bio_field.index_type, Some(IndexType::Text));
}

#[test]
fn test_node_schema_properties() {
    let schema = NodeSchema::new("User", vec!["User".to_string(), "Account".to_string()])
        .with_field(FieldDescriptor::new("user_id", FieldType::String).primary_key())
        .with_field(FieldDescriptor::new("email", FieldType::String).unique())
        .with_field(FieldDescriptor::new("age", FieldType::Int64).indexed())
        .with_field(FieldDescriptor::new("bio", FieldType::String));

    assert_eq!(schema.name, "User");
    assert_eq!(schema.primary_label(), "User");
    assert_eq!(schema.primary_key, Some("user_id".to_string()));

    let pk = schema.primary_key_field().unwrap();
    assert_eq!(pk.name, "user_id");
    assert!(pk.primary_key);

    let uniques = schema.unique_fields();
    assert_eq!(uniques.len(), 2); // user_id (primary_key) and email (unique)

    let indexed = schema.indexed_fields();
    assert_eq!(indexed.len(), 1); // age
    assert_eq!(indexed[0].name, "age");
}

#[test]
fn test_relationship_schema_properties() {
    let schema = RelationshipSchema::new("Follows", "FOLLOWS")
        .with_endpoints(vec!["User".to_string()], vec!["User".to_string()])
        .with_field(FieldDescriptor::new("since", FieldType::Int64).nullable(false))
        .directed(true);

    assert_eq!(schema.name, "Follows");
    assert_eq!(schema.type_name, "FOLLOWS");
    assert_eq!(schema.source_labels, vec!["User"]);
    assert_eq!(schema.target_labels, vec!["User"]);
    assert!(schema.directed);

    let reqs = schema.required_fields();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].name, "since");
}

#[test]
fn test_schema_registry_crud() {
    let registry = SchemaRegistry::new();

    let user_schema = NodeSchema::new("User", vec!["User".to_string()])
        .with_field(FieldDescriptor::new("user_id", FieldType::String).primary_key())
        .with_field(FieldDescriptor::new("email", FieldType::String).unique());

    let company_schema = NodeSchema::new("Company", vec!["Company".to_string(), "Org".to_string()])
        .with_field(FieldDescriptor::new("company_id", FieldType::String).primary_key())
        .with_field(FieldDescriptor::new("name", FieldType::String));

    let works_at = RelationshipSchema::new("WorksAt", "WORKS_AT")
        .with_endpoints(vec!["User".to_string()], vec!["Company".to_string()])
        .with_field(FieldDescriptor::new("role", FieldType::String));

    registry.register_node(user_schema).unwrap();
    registry.register_node(company_schema).unwrap();
    registry.register_relationship(works_at).unwrap();

    assert!(registry.has_node("User"));
    assert!(registry.has_node("Company"));
    assert!(!registry.has_node("Unknown"));
    assert!(registry.has_relationship("WorksAt"));
    assert!(!registry.has_relationship("Unknown"));

    // Lookup by name
    let retrieved_user = registry.get_node("User").unwrap();
    assert_eq!(retrieved_user.name, "User");
    assert_eq!(retrieved_user.primary_key, Some("user_id".to_string()));

    // Lookup by label (case-insensitive)
    let retrieved_by_label = registry.get_node_by_label("company").unwrap();
    assert_eq!(retrieved_by_label.name, "Company");
    let retrieved_by_sec_label = registry.get_node_by_label("org").unwrap();
    assert_eq!(retrieved_by_sec_label.name, "Company");

    // Lookup by relationship type (case-insensitive)
    let retrieved_rel = registry.get_relationship_by_type("works_at").unwrap();
    assert_eq!(retrieved_rel.name, "WorksAt");
    assert_eq!(retrieved_rel.type_name, "WORKS_AT");

    assert_eq!(registry.node_schemas().len(), 2);
    assert_eq!(registry.relationship_schemas().len(), 1);

    // Clear
    registry.clear();
    assert!(!registry.has_node("User"));
    assert!(!registry.has_relationship("WorksAt"));
    assert_eq!(registry.node_schemas().len(), 0);
}

#[test]
fn test_schema_registry_concurrency() {
    let registry = Arc::new(SchemaRegistry::new());
    let mut handles = Vec::new();

    for i in 0..10 {
        let reg = Arc::clone(&registry);
        handles.push(thread::spawn(move || {
            let name = format!("Node_{i}");
            let schema = NodeSchema::new(&name, vec![name.clone()])
                .with_field(FieldDescriptor::new("id", FieldType::Int64).primary_key());
            reg.register_node(schema).unwrap();
            assert!(reg.has_node(&name));
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(registry.node_schemas().len(), 10);
}

#[test]
fn test_global_schema_registry_singleton() {
    let reg1 = global_schema_registry();
    let reg2 = global_schema_registry();
    assert!(std::ptr::eq(reg1, reg2));
}

#[test]
fn test_schema_snapshot_and_json_serialization() {
    let registry = SchemaRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);

    let user_schema = NodeSchema::new("User", vec!["User".to_string()])
        .with_field(FieldDescriptor::new("id", FieldType::String).primary_key())
        .with_field(FieldDescriptor::new("email", FieldType::String).unique());
    registry.register_node(user_schema).unwrap();

    let follows_schema = RelationshipSchema::new("Follows", "FOLLOWS")
        .with_endpoints(vec!["User".to_string()], vec!["User".to_string()])
        .with_field(FieldDescriptor::new("since", FieldType::Int64));
    registry.register_relationship(follows_schema).unwrap();

    assert_eq!(registry.len(), 2);
    assert!(!registry.is_empty());

    // Serialize to JSON
    let json_str = registry.to_json().unwrap();
    assert!(json_str.contains("\"User\""));
    assert!(json_str.contains("\"FOLLOWS\""));
    assert!(json_str.contains("\"email\""));

    // Deserialize into fresh registry
    let fresh_registry = SchemaRegistry::new();
    fresh_registry.from_json(&json_str).unwrap();
    assert_eq!(fresh_registry.len(), 2);
    assert!(fresh_registry.has_node("User"));
    assert!(fresh_registry.has_relationship("Follows"));

    let user = fresh_registry.get_node("User").unwrap();
    assert_eq!(user.primary_key, Some("id".to_string()));
    assert!(user.fields.get("email").unwrap().unique);

    // Test removals
    let removed = fresh_registry.remove_node("User").unwrap();
    assert_eq!(removed.name, "User");
    assert!(!fresh_registry.has_node("User"));
    assert_eq!(fresh_registry.len(), 1);

    let removed_rel = fresh_registry.remove_relationship("Follows").unwrap();
    assert_eq!(removed_rel.name, "Follows");
    assert!(!fresh_registry.has_relationship("Follows"));
    assert!(fresh_registry.is_empty());
}

#[test]
fn test_validate_topology_conformance() {
    use voyager_core::topology::GraphTopology;

    let registry = SchemaRegistry::new();
    let person = NodeSchema::new("Person", vec!["Person".to_string()])
        .with_field(FieldDescriptor::new("name", FieldType::String))
        .with_field(FieldDescriptor::new("age", FieldType::Int64));
    let movie = NodeSchema::new("Movie", vec!["Movie".to_string()])
        .with_field(FieldDescriptor::new("title", FieldType::String))
        .with_field(FieldDescriptor::new("released", FieldType::Int64));
    let acted_in = RelationshipSchema::new("ActedIn", "ACTED_IN")
        .with_endpoints(vec!["Person".to_string()], vec!["Movie".to_string()])
        .with_field(FieldDescriptor::new("role", FieldType::String))
        .directed(true);

    registry.register_node(person).unwrap();
    registry.register_node(movie).unwrap();
    registry.register_relationship(acted_in).unwrap();

    // 1. Valid conforming query
    let valid_topo = GraphTopology::from_query_str(
        "MATCH (p:Person {name: 'Keanu Reeves'})-[:ACTED_IN {role: 'Neo'}]->(m:Movie) WHERE m.released = 1999 RETURN p, m",
    );
    let report = registry.validate_topology(&valid_topo);
    assert!(report.is_valid);
    assert!(report.error_messages().is_empty());

    // 2. Unknown node label with typo suggestion
    let typo_node_topo = GraphTopology::from_query_str("MATCH (p:Persn) RETURN p");
    let report = registry.validate_topology(&typo_node_topo);
    assert!(!report.is_valid);
    assert_eq!(report.error_messages().len(), 1);
    assert!(report.error_messages()[0].contains("Node label 'Persn' is not registered"));
    assert!(report.error_messages()[0].contains("Did you mean label 'Person'?"));

    // 3. Unknown relationship type with typo suggestion
    let typo_rel_topo =
        GraphTopology::from_query_str("MATCH (p:Person)-[:ACTED_INTO]->(m:Movie) RETURN p");
    let report = registry.validate_topology(&typo_rel_topo);
    assert!(!report.is_valid);
    assert!(
        report.error_messages()[0].contains("Relationship type 'ACTED_INTO' is not registered")
    );
    assert!(report.error_messages()[0].contains("Did you mean relationship 'ACTED_IN'?"));

    // 4. Incompatible endpoints
    let company = NodeSchema::new("Company", vec!["Company".to_string()]);
    registry.register_node(company).unwrap();
    let bad_endpoints_topo =
        GraphTopology::from_query_str("MATCH (c:Company)-[:ACTED_IN]->(m:Movie) RETURN c, m");
    let report = registry.validate_topology(&bad_endpoints_topo);
    assert!(!report.is_valid);
    assert!(report.error_messages()[0].contains("cannot connect source label(s) [\"Company\"]"));

    // 5. Property type mismatch
    let type_mismatch_topo =
        GraphTopology::from_query_str("MATCH (p:Person) WHERE p.age = 'twenty' RETURN p");
    let report = registry.validate_topology(&type_mismatch_topo);
    assert!(!report.is_valid);
    assert!(report.error_messages()[0].contains("Property 'Person.age' expects type INTEGER"));

    // 6. Undirected traversal warning
    let undirected_topo =
        GraphTopology::from_query_str("MATCH (p:Person)-[:ACTED_IN]-(m:Movie) RETURN p, m");
    let report = registry.validate_topology(&undirected_topo);
    assert!(report.is_valid); // Warnings don't invalidate query
    assert_eq!(report.warning_messages().len(), 1);
    assert!(
        report.warning_messages()[0]
            .contains("defined as directed in schema, but traversed undirected")
    );
}

#[test]
fn test_vector_index_schema_and_ddl_emission() {
    use voyager_core::{
        FieldDescriptor, FieldType, IndexType, NodeSchema, VectorSimilarity,
        emit_node_drop_index_ddl, emit_node_index_ddl,
    };

    assert_eq!(
        VectorSimilarity::parse_str("cosine"),
        Some(VectorSimilarity::Cosine)
    );
    assert_eq!(
        VectorSimilarity::parse_str("euclidean"),
        Some(VectorSimilarity::Euclidean)
    );
    assert_eq!(
        VectorSimilarity::parse_str("l2"),
        Some(VectorSimilarity::Euclidean)
    );
    assert_eq!(
        VectorSimilarity::parse_str("dot"),
        Some(VectorSimilarity::Dot)
    );
    assert_eq!(
        VectorSimilarity::parse_str("inner_product"),
        Some(VectorSimilarity::Dot)
    );
    assert_eq!(
        VectorSimilarity::parse_str("ip"),
        Some(VectorSimilarity::Dot)
    );
    assert_eq!(VectorSimilarity::parse_str("unknown"), None);

    let vec_field =
        FieldDescriptor::new("embedding", FieldType::List(Box::new(FieldType::Float64)))
            .with_vector_index(
                1536,
                VectorSimilarity::Cosine,
                Some("art_vec_idx".to_string()),
            );

    assert!(vec_field.is_vector_index());
    assert_eq!(vec_field.index_type, Some(IndexType::Vector));
    let cfg = vec_field.vector_config.as_ref().unwrap();
    assert_eq!(cfg.dimensions, 1536);
    assert_eq!(cfg.similarity, VectorSimilarity::Cosine);
    assert_eq!(cfg.index_name.as_deref(), Some("art_vec_idx"));

    let schema = NodeSchema::new("Article", vec!["Article".to_string()])
        .with_field(FieldDescriptor::new("title", FieldType::String).indexed())
        .with_field(vec_field);

    // 1. Cypher DDL (Neo4j 5+)
    let cypher_create = emit_node_index_ddl(&schema, "cypher").unwrap();
    assert_eq!(cypher_create.len(), 2);
    assert!(
        cypher_create.iter().any(|s| s.contains("CREATE VECTOR INDEX art_vec_idx IF NOT EXISTS FOR (n:Article) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1536, `vector.similarity_function`: 'cosine'}}"))
    );
    let cypher_drop = emit_node_drop_index_ddl(&schema, "cypher").unwrap();
    assert!(
        cypher_drop
            .iter()
            .any(|s| s.contains("DROP INDEX art_vec_idx IF EXISTS"))
    );

    // 2. FalkorDB DDL
    let falkor_create = emit_node_index_ddl(&schema, "falkordb").unwrap();
    assert!(
        falkor_create.iter().any(|s| s.contains("CREATE VECTOR INDEX FOR (n:Article) ON (n.embedding) OPTIONS {dimension: 1536, similarityFunction: 'cosine'}"))
    );
    let falkor_drop = emit_node_drop_index_ddl(&schema, "falkordb").unwrap();
    assert!(
        falkor_drop
            .iter()
            .any(|s| s.contains("DROP VECTOR INDEX FOR (n:Article) ON (n.embedding)"))
    );

    // 3. Apache AGE DDL
    let age_create = emit_node_index_ddl(&schema, "age").unwrap();
    assert!(
        age_create.iter().any(|s| s.contains("CREATE INDEX IF NOT EXISTS art_vec_idx ON ag_catalog.\"Article\" USING hnsw (embedding vector_cosine_ops);"))
    );
    let age_drop = emit_node_drop_index_ddl(&schema, "age").unwrap();
    assert!(
        age_drop
            .iter()
            .any(|s| s.contains("DROP INDEX IF EXISTS art_vec_idx;"))
    );

    // 4. PostgreSQL / SQL DDL
    let sql_create = emit_node_index_ddl(&schema, "postgres").unwrap();
    assert!(
        sql_create.iter().any(|s| s.contains("CREATE INDEX IF NOT EXISTS art_vec_idx ON \"article\" USING hnsw (\"embedding\" vector_cosine_ops);"))
    );
    let sql_drop = emit_node_drop_index_ddl(&schema, "postgres").unwrap();
    assert!(
        sql_drop
            .iter()
            .any(|s| s.contains("DROP INDEX IF EXISTS art_vec_idx;"))
    );

    // 5. Memgraph DDL
    let memgraph_create = emit_node_index_ddl(&schema, "memgraph").unwrap();
    assert_eq!(memgraph_create.len(), 2);
    assert!(
        memgraph_create.iter().any(|s| s.contains("CREATE VECTOR INDEX art_vec_idx ON :Article(embedding) WITH CONFIG {\"dimension\": 1536, \"capacity\": 10000, \"metric\": \"cos\"};"))
    );
    assert!(
        memgraph_create
            .iter()
            .any(|s| s.contains("CREATE INDEX ON :Article(title);"))
    );
    let memgraph_drop = emit_node_drop_index_ddl(&schema, "memgraph").unwrap();
    assert!(
        memgraph_drop
            .iter()
            .any(|s| s.contains("DROP VECTOR INDEX art_vec_idx;"))
    );
    assert!(
        memgraph_drop
            .iter()
            .any(|s| s.contains("DROP INDEX ON :Article(title);"))
    );

    // 6. DuckDB DDL
    let duckdb_create = emit_node_index_ddl(&schema, "duckdb").unwrap();
    assert_eq!(duckdb_create.len(), 2);
    assert!(
        duckdb_create.iter().any(|s| s.contains("CREATE INDEX IF NOT EXISTS art_vec_idx ON \"article\" USING HNSW (\"embedding\") WITH (metric = 'cosine');"))
    );
    assert!(duckdb_create.iter().any(|s| {
        s.contains("CREATE INDEX IF NOT EXISTS idx_article_title ON \"article\" (\"title\");")
    }));
    let duckdb_drop = emit_node_drop_index_ddl(&schema, "duckdb").unwrap();
    assert!(
        duckdb_drop
            .iter()
            .any(|s| s.contains("DROP INDEX IF EXISTS art_vec_idx;"))
    );
    assert!(
        duckdb_drop
            .iter()
            .any(|s| s.contains("DROP INDEX IF EXISTS idx_article_title;"))
    );
}
