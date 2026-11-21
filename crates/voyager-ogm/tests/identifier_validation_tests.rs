//! Identifier injection regression tests (#186).
//!
//! Every string that becomes an identifier in emitted query text must be a plain
//! identifier (or a dotted function name). These tests drive each sink through the
//! fluent API and assert that all four dialect emitters reject the payload.

use voyager_ogm::builder::QueryBuilder;
use voyager_ogm::emitters::{AgeEmitter, CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_ogm::visitor::AstVisitor;
use voyager_ogm::{Error, ErrorCode};

const LABEL_PAYLOAD: &str = "Person) DETACH DELETE p //";
const PARAM_PAYLOAD: &str = "x RETURN 1 //";
const PROP_PAYLOAD: &str = "name} RETURN 1 //";
const FUNC_PAYLOAD: &str = "toLower(1)) RETURN 1 //";

fn assert_invalid<T: std::fmt::Debug>(dialect: &str, kind: &str, result: Result<T, Error>) {
    match result {
        Ok(value) => panic!("{dialect}: expected rejection of {kind}, got {value:?}"),
        Err(error) => {
            let message = error.to_string();
            assert_eq!(
                error.code(),
                ErrorCode::InvalidClause,
                "{dialect}: unexpected error code for {message:?}"
            );
            assert!(
                message.contains("Invalid"),
                "{dialect}: expected an invalid-identifier error, got {message:?}"
            );
            assert!(
                message.contains(kind),
                "{dialect}: expected error to name {kind:?}, got {message:?}"
            );
        }
    }
}

fn assert_all_dialects_reject(builder: QueryBuilder, kind: &str) {
    let (arena, root) = builder
        .try_build()
        .expect("builder should build before emission");

    assert_invalid(
        "cypher",
        kind,
        CypherEmitter::new().visit_query(&arena, root),
    );
    assert_invalid(
        "age",
        kind,
        AgeEmitter::new("graph_name").visit_query(&arena, root),
    );
    assert_invalid(
        "iso_gql",
        kind,
        IsoGqlEmitter::new().visit_query(&arena, root),
    );
    assert_invalid(
        "sql_pgq",
        kind,
        SqlPgqEmitter::new("graph_name").visit_query(&arena, root),
    );
}

#[test]
fn rejects_injection_in_node_variable() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some(LABEL_PAYLOAD), vec!["Person"]);
    assert_all_dialects_reject(builder, "node variable");
}

#[test]
fn rejects_injection_in_node_label() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("p"), vec![LABEL_PAYLOAD]);
    assert_all_dialects_reject(builder, "node label");
}

#[test]
fn rejects_injection_in_edge_variable() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec!["KNOWS"], Some(LABEL_PAYLOAD))
        .node(Some("q"), vec!["Person"]);
    assert_all_dialects_reject(builder, "edge variable");
}

#[test]
fn rejects_injection_in_relationship_type() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .to(vec![LABEL_PAYLOAD], Some("r"))
        .node(Some("q"), vec!["Person"]);
    assert_all_dialects_reject(builder, "relationship type");
}

#[test]
fn rejects_injection_in_property_read() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .where_eq("p", PROP_PAYLOAD, 1i64);
    assert_all_dialects_reject(builder, "property name");
}

#[test]
fn rejects_injection_in_inline_property_map() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node_with_props(Some("p"), vec!["Person"], vec![(PROP_PAYLOAD, 1i64)]);
    assert_all_dialects_reject(builder, "property name");
}

#[test]
fn rejects_injection_in_projection_alias() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .r#return()
        .field("p", "name", Some(PROP_PAYLOAD));
    assert_all_dialects_reject(builder, "alias");
}

#[test]
fn rejects_injection_in_path_variable() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .path_variable(PROP_PAYLOAD)
        .node(Some("p"), vec!["Person"]);
    assert_all_dialects_reject(builder, "path variable");
}

#[test]
fn rejects_injection_in_unwind_alias() {
    let mut builder = QueryBuilder::new();
    builder.unwind_param("batch", PROP_PAYLOAD);
    assert_all_dialects_reject(builder, "unwind alias");
}

#[test]
fn rejects_injection_in_let_variable() {
    let mut builder = QueryBuilder::new();
    let value = builder.literal(1i64);
    builder.let_(PROP_PAYLOAD, value);
    assert_all_dialects_reject(builder, "let variable");
}

#[test]
fn rejects_injection_in_delete_target() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .detach_delete(vec![PROP_PAYLOAD]);
    assert_all_dialects_reject(builder, "delete target");
}

#[test]
fn rejects_injection_in_parameter_name() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("p"), vec!["Person"]);
    let parameter = builder.param(PARAM_PAYLOAD);
    builder.where_expr(parameter);
    assert_all_dialects_reject(builder, "parameter name");
}

#[test]
fn rejects_injection_in_function_name() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("p"), vec!["Person"]).r#return();
    let property = builder.prop("p", "name");
    let function = builder.function(FUNC_PAYLOAD, vec![property]);
    builder.select_expr(function, None::<String>);
    assert_all_dialects_reject(builder, "function name");
}

#[test]
fn rejects_injection_in_set_property_key() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .set_property("p", PROP_PAYLOAD, 1i64);
    assert_all_dialects_reject(builder, "property name");
}

#[test]
fn rejects_injection_in_remove_property_key() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["Person"])
        .remove_property("p", PROP_PAYLOAD);
    assert_all_dialects_reject(builder, "property name");
}

#[test]
fn rejects_injection_in_merge_key() {
    let mut builder = QueryBuilder::new();
    builder
        .merge()
        .node_with_props(Some("p"), vec!["Person"], vec![(PROP_PAYLOAD, 1i64)]);
    assert_all_dialects_reject(builder, "property name");
}

#[test]
fn rejects_injection_in_merge_set_key() {
    let mut builder = QueryBuilder::new();
    builder
        .merge()
        .node(Some("p"), vec!["Person"])
        .on_create_set("p", PROP_PAYLOAD, 1i64);
    assert_all_dialects_reject(builder, "property name");
}

#[test]
fn rejects_injection_in_load_csv_alias() {
    let mut builder = QueryBuilder::new();
    builder.load_csv("file:///data.csv", true, PROP_PAYLOAD);
    assert_all_dialects_reject(builder, "load csv alias");
}

#[test]
fn rejects_injection_in_procedure_name_and_yield_item() {
    let mut builder = QueryBuilder::new();
    builder.call_procedure(FUNC_PAYLOAD, vec![]);
    assert_all_dialects_reject(builder, "procedure name");

    let mut builder = QueryBuilder::new();
    builder
        .call_procedure("dbms.components", vec![])
        .yield_items(vec![FUNC_PAYLOAD.to_string()]);
    assert_all_dialects_reject(builder, "yield item");
}

#[test]
fn accepts_unicode_and_internal_style_identifiers() {
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("_person_0"), vec!["Ünïcode", "日本語"])
        .r#return()
        .field("_person_0", "name", Some("p0"));
    let (arena, root) = builder.try_build().expect("builder should build");

    CypherEmitter::new()
        .visit_query(&arena, root)
        .expect("unicode identifiers should remain valid");
}
