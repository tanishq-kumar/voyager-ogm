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
