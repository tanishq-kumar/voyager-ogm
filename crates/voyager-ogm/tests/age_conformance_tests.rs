//! Formal Apache AGE (PostgreSQL Embedded Cypher) Conformance Test Suite.
//!
//! Ingests and validates Apache AGE regression scenarios:
//! 1. SELECT * FROM cypher('graph_name', $$ MATCH ... $$) AS (...) wrapper emission
//! 2. Automatic mapping of projections to PostgreSQL agtype
//! 3. Parameter isolation across the SQL boundary
//! 4. Directed, incoming, undirected, and variable-length traversals
//! 5. Full operator set (=, !=, >, >=, <, <=, IN, NOT IN, CONTAINS, STARTS WITH, ENDS WITH)
//! 6. Projections, aggregations, ordering, and DML mutation wrappers

use voyager_ogm::ast::{AggregationFunc, BinaryOp, LiteralValue};
use voyager_ogm::builder::QueryBuilder;
use voyager_ogm::emitters::age::AgeEmitter;
use voyager_ogm::visitor::AstVisitor;

#[test]
fn test_age_single_node_and_projections() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .where_property("p", "age", BinaryOp::Gt, 30)
        .field("p", "name", None::<String>)
        .field("p", "age", None::<String>);

    let (arena, root) = builder.build();
    let mut age = AgeEmitter::new("social_graph");
    let res = age.visit_query(&arena, root).unwrap();

    let expected = "SELECT * FROM cypher('social_graph', $$ MATCH (p:Person) WHERE p.age > $p0 RETURN p.name, p.age $$, %s) AS (name agtype, age agtype)";
    assert_eq!(res.statement, expected);
    assert_eq!(res.parameters.get("p0"), Some(&LiteralValue::Int64(30)));
}

#[test]
fn test_age_all_operators_and_like_translations() {
    let ops = [
        (BinaryOp::Eq, "=", LiteralValue::Int64(30)),
        (BinaryOp::Neq, "!=", LiteralValue::Int64(30)),
        (BinaryOp::Gt, ">", LiteralValue::Int64(21)),
        (BinaryOp::Gte, ">=", LiteralValue::Int64(21)),
        (BinaryOp::Lt, "<", LiteralValue::Int64(65)),
        (BinaryOp::Lte, "<=", LiteralValue::Int64(65)),
        (
            BinaryOp::In,
            "IN",
            LiteralValue::List(vec![
                LiteralValue::String("London".into()),
                LiteralValue::String("Paris".into()),
            ]),
        ),
        (
            BinaryOp::NotIn,
            "NOT IN",
            LiteralValue::List(vec![LiteralValue::String("Rome".into())]),
        ),
        (
            BinaryOp::Contains,
            "CONTAINS",
            LiteralValue::String("don".into()),
        ),
        (
            BinaryOp::StartsWith,
            "STARTS WITH",
            LiteralValue::String("Al".into()),
        ),
        (
            BinaryOp::EndsWith,
            "ENDS WITH",
            LiteralValue::String("ce".into()),
        ),
    ];

    for (op, op_str, lit) in ops {
        let mut builder = QueryBuilder::new();
        builder
            .match_node(Some("p"), vec!["Person"])
            .where_property("p", "age", op, lit.clone())
            .field("p", "name", None::<String>);

        let (arena, root) = builder.build();
        let mut age = AgeEmitter::new("age_graph");
        let res = age.visit_query(&arena, root).unwrap();

        assert!(
            res.statement.contains(&format!("p.age {op_str} $p0")),
            "Failed for AGE op {op:?}: {}",
            res.statement
        );
        assert_eq!(res.parameters.get("p0"), Some(&lit));
    }
}

#[test]
fn test_age_directed_traversal_with_aliases() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"])
        .where_property("a", "name", BinaryOp::Eq, "Alice")
        .field("a", "name", Some("source".to_string()))
        .field("b", "name", Some("target".to_string()));

    let (arena, root) = builder.build();
    let mut age = AgeEmitter::new("age_graph");
    let res = age.visit_query(&arena, root).unwrap();

    let expected = "SELECT * FROM cypher('age_graph', $$ MATCH (a:Person)-[r:KNOWS]->(b:Person) WHERE a.name = $p0 RETURN a.name AS source, b.name AS target $$, %s) AS (source agtype, target agtype)";
    assert_eq!(res.statement, expected);
    assert_eq!(
        res.parameters.get("p0"),
        Some(&LiteralValue::String("Alice".to_string()))
    );
}

#[test]
fn test_age_variable_length_and_undirected_traversals() {
    let mut b_hops = QueryBuilder::new();
    b_hops
        .match_node(Some("a"), vec!["Person"])
        .to(vec!["KNOWS".to_string()], None::<String>)
        .hops(1, 3)
        .node(Some("b"), vec!["Person"])
        .field("b", "name", None::<String>);

    let (a_h, r_h) = b_hops.build();
    let mut age = AgeEmitter::new("g");
    let res_h = age.visit_query(&a_h, r_h).unwrap();
    assert!(res_h.statement.contains("[:KNOWS*1..3]"));

    let mut b_undir = QueryBuilder::new();
    b_undir
        .match_node(Some("a"), vec!["Person"])
        .edge(vec!["KNOWS".to_string()], Some("r".to_string()))
        .node(Some("b"), vec!["Person"])
        .field("b", "name", None::<String>);

    let (a_u, r_u) = b_undir.build();
    let res_u = age.visit_query(&a_u, r_u).unwrap();
    assert!(res_u.statement.contains("(a:Person)-[r:KNOWS]-(b:Person)"));
}

#[test]
fn test_age_aggregations_and_grouping() {
    let mut builder = QueryBuilder::new();
    builder
        .match_node(Some("p"), vec!["Person"])
        .field("p", "city", Some("city".to_string()))
        .select_property_aggregate("p", "name", AggregationFunc::Count, Some("cnt".to_string()))
        .select_property_aggregate(
            "p",
            "age",
            AggregationFunc::Avg,
            Some("avg_age".to_string()),
        )
        .select_property_aggregate(
            "p",
            "name",
            AggregationFunc::Collect,
            Some("names".to_string()),
        )
        .order_by_asc("p", "city");

    let (arena, root) = builder.build();
    let mut age = AgeEmitter::new("social_graph");
    let res = age.visit_query(&arena, root).unwrap();

    assert!(res.statement.contains("COUNT(p.name) AS cnt"));
    assert!(res.statement.contains("AVG(p.age) AS avg_age"));
    assert!(res.statement.contains("COLLECT(p.name) AS names"));
    assert!(
        res.statement
            .contains("AS (city agtype, cnt agtype, avg_age agtype, names agtype)")
    );
}

#[test]
fn test_age_mutation_wrappers() {
    let mut age = AgeEmitter::new("age_graph");

    // CREATE
    let mut b_create = QueryBuilder::new();
    b_create.create().node(Some("p"), vec!["Person"]);
    let (a_c, r_c) = b_create.build();
    let res_c = age.visit_query(&a_c, r_c).unwrap();
    assert_eq!(
        res_c.statement,
        "SELECT * FROM cypher('age_graph', $$ CREATE (p:Person) $$) AS (result agtype)"
    );

    // MERGE
    let mut b_merge = QueryBuilder::new();
    b_merge
        .merge()
        .node(Some("p"), vec!["Person"])
        .on_create_set("p", "name", "Eva");
    let (a_m, r_m) = b_merge.build();
    let res_m = age.visit_query(&a_m, r_m).unwrap();
    assert!(
        res_m
            .statement
            .contains("MERGE (p:Person) ON CREATE SET p.name = $p0")
    );
    assert_eq!(
        res_m.parameters.get("p0"),
        Some(&LiteralValue::String("Eva".into()))
    );
}

#[test]
fn test_age_default_and_custom_graph_name() {
    use voyager_ogm::emitters::age::DEFAULT_AGE_GRAPH;

    assert_eq!(DEFAULT_AGE_GRAPH, "age_graph");

    let mut b = QueryBuilder::new();
    b.match_node(Some("p"), vec!["Person"])
        .field("p", "name", None::<String>);
    let (arena, root) = b.build();

    // Default emitter
    let mut default_emitter = AgeEmitter::default();
    let res_default = default_emitter.visit_query(&arena, root).unwrap();
    assert!(
        res_default
            .statement
            .starts_with("SELECT * FROM cypher('age_graph',")
    );

    // Custom graph name emitter
    let mut custom_emitter = AgeEmitter::new("production_graph");
    let res_custom = custom_emitter.visit_query(&arena, root).unwrap();
    assert!(
        res_custom
            .statement
            .starts_with("SELECT * FROM cypher('production_graph',")
    );
}

#[test]
fn test_age_invalid_graph_name_rejected() {
    let mut b = QueryBuilder::new();
    b.match_node(Some("p"), vec!["Person"])
        .field("p", "name", None::<String>);
    let (arena, root) = b.build();

    let invalid_names = [
        "x'); DROP",
        "",
        "a b",
        "123start_with_digit",
        "has-hyphen",
        "drop;--",
    ];
    for name in invalid_names {
        let mut emitter = AgeEmitter::new(name);
        let err = emitter.visit_query(&arena, root);
        assert!(err.is_err(), "Expected graph name '{name}' to be rejected");
        let err_msg = err.unwrap_err().to_string();
        assert!(
            err_msg.contains("Invalid Apache AGE graph name"),
            "Unexpected error message for '{name}': {err_msg}"
        );
    }
}

#[test]
fn test_bulk_compilation_with_age_graph() {
    use voyager_ogm::bulk::{compile_bulk_create_with_graph, compile_bulk_merge_with_graph};

    let props = ["name", "age"];
    let c = compile_bulk_create_with_graph(
        "User",
        &props,
        "batch",
        "row",
        "age",
        Some("production_graph"),
    )
    .unwrap();
    assert!(c.statement.starts_with(
        "SELECT * FROM cypher('production_graph', $$ UNWIND $batch AS row CREATE (_user_0:User)"
    ));

    let m = compile_bulk_merge_with_graph(
        "User",
        "name",
        &["age"],
        "batch",
        "row",
        "age",
        Some("production_graph"),
    )
    .unwrap();
    assert!(m.statement.starts_with(
        "SELECT * FROM cypher('production_graph', $$ UNWIND $batch AS row MERGE (_user_0:User"
    ));
}

#[test]
fn test_age_ddl_graph_schema_targeting() {
    use voyager_ogm::emitters::{
        emit_node_constraint_ddl_with_graph, emit_node_drop_index_ddl_with_graph,
        emit_node_index_ddl, emit_node_index_ddl_with_graph, emit_rel_index_ddl_with_graph,
    };
    use voyager_ogm::schema::{
        FieldDescriptor, FieldType, FullTextIndexConfig, NodeSchema, RelationshipSchema,
        VectorSimilarity,
    };

    let doc_schema = NodeSchema::new("Doc", vec!["Doc".to_string()])
        .with_field(FieldDescriptor::new("id", FieldType::Int64).primary_key())
        .with_field(FieldDescriptor::new("title", FieldType::String))
        .with_field(
            FieldDescriptor::new("embedding", FieldType::List(Box::new(FieldType::Float64)))
                .with_vector_index(
                    128,
                    VectorSimilarity::Cosine,
                    Some("doc_vec_idx".to_string()),
                ),
        )
        .with_fulltext_index(FullTextIndexConfig {
            name: "doc_ft_idx".to_string(),
            properties: vec!["title".to_string()],
            analyzer: Some("english".to_string()),
        });

    // 1. Default graph -> "age_graph"."Doc"
    let default_ddl = emit_node_index_ddl(&doc_schema, "age").unwrap();
    assert_eq!(default_ddl.len(), 2);
    assert!(default_ddl[0].contains("CREATE INDEX IF NOT EXISTS doc_vec_idx ON \"age_graph\".\"Doc\" USING hnsw (embedding vector_cosine_ops);"));
    assert!(default_ddl[1].contains("CREATE INDEX IF NOT EXISTS doc_ft_idx ON \"age_graph\".\"Doc\" USING gin (to_tsvector('english', (properties->>'title'::text)));"));

    // 2. Custom graph -> "production_graph"."Doc"
    let custom_ddl =
        emit_node_index_ddl_with_graph(&doc_schema, "age", Some("production_graph")).unwrap();
    assert_eq!(custom_ddl.len(), 2);
    assert!(custom_ddl[0].contains("CREATE INDEX IF NOT EXISTS doc_vec_idx ON \"production_graph\".\"Doc\" USING hnsw (embedding vector_cosine_ops);"));
    assert!(custom_ddl[1].contains("CREATE INDEX IF NOT EXISTS doc_ft_idx ON \"production_graph\".\"Doc\" USING gin (to_tsvector('english', (properties->>'title'::text)));"));

    // 3. Drop index statements
    let default_drop = emit_node_drop_index_ddl_with_graph(&doc_schema, "age", None).unwrap();
    assert_eq!(default_drop.len(), 2);
    assert_eq!(
        default_drop[0],
        "DROP INDEX IF EXISTS \"age_graph\".doc_vec_idx;"
    );
    assert_eq!(
        default_drop[1],
        "DROP INDEX IF EXISTS \"age_graph\".doc_ft_idx;"
    );

    let custom_drop =
        emit_node_drop_index_ddl_with_graph(&doc_schema, "age", Some("production_graph")).unwrap();
    assert_eq!(custom_drop.len(), 2);
    assert_eq!(
        custom_drop[0],
        "DROP INDEX IF EXISTS \"production_graph\".doc_vec_idx;"
    );
    assert_eq!(
        custom_drop[1],
        "DROP INDEX IF EXISTS \"production_graph\".doc_ft_idx;"
    );

    // 4. Invalid graph name rejected
    assert!(emit_node_index_ddl_with_graph(&doc_schema, "age", Some("bad;drop")).is_err());
    assert!(emit_node_drop_index_ddl_with_graph(&doc_schema, "age", Some("")).is_err());
    assert!(emit_node_index_ddl_with_graph(&doc_schema, "age", Some("123num")).is_err());

    // 5. Relationship and constraint error messages reference graph schema
    let rel_schema = RelationshipSchema::new("Links", "LINKS");
    let rel_err =
        emit_rel_index_ddl_with_graph(&rel_schema, "age", Some("production_graph")).unwrap_err();
    assert!(
        rel_err
            .to_string()
            .contains("\"production_graph\".\"LINKS\"")
    );

    let constr_err =
        emit_node_constraint_ddl_with_graph(&doc_schema, "age", false, Some("production_graph"))
            .unwrap_err();
    assert!(constr_err.to_string().contains("\"<graph>\".\"<Label>\""));
}
