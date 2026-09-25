use voyager_core::ast::*;
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{AgeEmitter, CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

#[test]
fn test_cypher_explain_and_profile() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_gt("p", "age", 30)
        .r#return()
        .field("p", "name", None::<&str>);

    // 1. Normal
    let (arena, root) = builder.clone().build();
    let mut emitter = CypherEmitter::new();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "MATCH (p:Person) WHERE p.age > $p0 RETURN p.name"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::Normal);

    // 2. Explain
    let mut explain_builder = builder.clone();
    explain_builder.explain();
    assert_eq!(explain_builder.execution_mode(), ExecutionMode::Explain);
    let (arena, root) = explain_builder.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "EXPLAIN MATCH (p:Person) WHERE p.age > $p0 RETURN p.name"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::Explain);

    // 3. Profile
    let mut profile_builder = builder.clone();
    profile_builder.profile();
    assert_eq!(profile_builder.execution_mode(), ExecutionMode::Profile);
    let (arena, root) = profile_builder.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "PROFILE MATCH (p:Person) WHERE p.age > $p0 RETURN p.name"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::Profile);

    // 4. Chaining both (explain then profile)
    let mut both_builder1 = builder.clone();
    both_builder1.explain().profile();
    assert_eq!(
        both_builder1.execution_mode(),
        ExecutionMode::ExplainAndProfile
    );
    let (arena, root) = both_builder1.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "PROFILE MATCH (p:Person) WHERE p.age > $p0 RETURN p.name"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::ExplainAndProfile);

    // 5. Chaining both (profile then explain)
    let mut both_builder2 = builder.clone();
    both_builder2.profile().explain();
    assert_eq!(
        both_builder2.execution_mode(),
        ExecutionMode::ExplainAndProfile
    );
    let (arena, root) = both_builder2.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "PROFILE MATCH (p:Person) WHERE p.age > $p0 RETURN p.name"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::ExplainAndProfile);
}

#[test]
fn test_iso_gql_explain_and_profile() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_gt("p", "age", 30)
        .r#return()
        .field("p", "name", None::<&str>);

    let mut emitter = IsoGqlEmitter::new();

    // Explain
    let mut explain_b = builder.clone();
    explain_b.explain();
    let (arena, root) = explain_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "EXPLAIN MATCH (p:Person) WHERE p.age > $p0 RETURN p.name"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::Explain);

    // Profile
    let mut profile_b = builder.clone();
    profile_b.profile();
    let (arena, root) = profile_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "PROFILE MATCH (p:Person) WHERE p.age > $p0 RETURN p.name"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::Profile);

    // Both
    let mut both_b = builder.clone();
    both_b.explain().profile();
    let (arena, root) = both_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "PROFILE MATCH (p:Person) WHERE p.age > $p0 RETURN p.name"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::ExplainAndProfile);
}

#[test]
fn test_sql_pgq_explain_and_profile() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_gt("p", "age", 30)
        .r#return()
        .field("p", "name", None::<&str>);

    let mut emitter = SqlPgqEmitter::new("social_graph");

    // Explain
    let mut explain_b = builder.clone();
    explain_b.explain();
    let (arena, root) = explain_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert!(
        compiled
            .statement
            .starts_with("EXPLAIN SELECT * FROM GRAPH_TABLE (social_graph")
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::Explain);

    // Profile -> EXPLAIN ANALYZE
    let mut profile_b = builder.clone();
    profile_b.profile();
    let (arena, root) = profile_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert!(
        compiled
            .statement
            .starts_with("EXPLAIN ANALYZE SELECT * FROM GRAPH_TABLE (social_graph")
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::Profile);

    // Both -> EXPLAIN ANALYZE
    let mut both_b = builder.clone();
    both_b.explain().profile();
    let (arena, root) = both_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert!(
        compiled
            .statement
            .starts_with("EXPLAIN ANALYZE SELECT * FROM GRAPH_TABLE (social_graph")
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::ExplainAndProfile);
}

#[test]
fn test_apache_age_explain_and_profile() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_gt("p", "age", 30)
        .r#return()
        .field("p", "name", None::<&str>);

    let mut emitter = AgeEmitter::new("age_graph");

    // Explain -> EXPLAIN SELECT * FROM cypher(...)
    let mut explain_b = builder.clone();
    explain_b.explain();
    let (arena, root) = explain_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert!(
        compiled
            .statement
            .starts_with("EXPLAIN SELECT * FROM cypher('age_graph'")
    );
    // Inner Cypher query MUST NOT contain EXPLAIN
    assert!(!compiled.statement.contains("$$ EXPLAIN"));
    assert_eq!(compiled.execution_mode, ExecutionMode::Explain);

    // Profile -> EXPLAIN ANALYZE SELECT * FROM cypher(...)
    let mut profile_b = builder.clone();
    profile_b.profile();
    let (arena, root) = profile_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert!(
        compiled
            .statement
            .starts_with("EXPLAIN ANALYZE SELECT * FROM cypher('age_graph'")
    );
    assert!(!compiled.statement.contains("$$ PROFILE"));
    assert_eq!(compiled.execution_mode, ExecutionMode::Profile);

    // Both -> EXPLAIN ANALYZE SELECT * FROM cypher(...)
    let mut both_b = builder.clone();
    both_b.explain().profile();
    let (arena, root) = both_b.build();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert!(
        compiled
            .statement
            .starts_with("EXPLAIN ANALYZE SELECT * FROM cypher('age_graph'")
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::ExplainAndProfile);
}

#[test]
fn test_procedure_call_explain_and_profile() {
    let mut builder = QueryBuilder::new();
    let mut arena = QueryAstArena::new();
    let arg = arena.alloc(AstNode::Literal(LiteralValue::String("Person".into())));
    let imported_arg = builder.import_subarena(arena, arg);

    builder
        .call_procedure("apoc.path.subgraphNodes", vec![imported_arg])
        .yield_items(vec!["node".into()])
        .explain();

    let (arena, root) = builder.build();
    let mut emitter = CypherEmitter::new();
    let compiled = emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "EXPLAIN CALL apoc.path.subgraphNodes($p0) YIELD node"
    );
    assert_eq!(compiled.execution_mode, ExecutionMode::Explain);
}
