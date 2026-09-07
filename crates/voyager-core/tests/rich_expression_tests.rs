use voyager_core::ast::{BinaryOp, LiteralValue};
use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::{AgeEmitter, CypherEmitter, IsoGqlEmitter, SqlPgqEmitter};
use voyager_core::visitor::AstVisitor;

#[test]
fn test_string_and_scalar_functions() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("p"), vec!["Person"]);

    let prop_name = builder.prop("p", "name");
    let prop_email = builder.prop("p", "email");
    let default_val = builder.literal("unknown");

    let to_lower_name = builder.function("toLower", vec![prop_name]);
    let coalesce_expr = builder.function("coalesce", vec![prop_email, default_val]);
    let trim_name = builder.function("trim", vec![prop_name]);

    builder
        .r#return()
        .custom_expr(to_lower_name, Some("lower_name"))
        .custom_expr(coalesce_expr, Some("clean_email"))
        .custom_expr(trim_name, Some("trimmed"));

    let (arena, root) = builder.build();

    let mut cypher_emitter = CypherEmitter::new();
    let compiled = cypher_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "MATCH (p:Person) RETURN toLower(p.name) AS lower_name, coalesce(p.email, $p0) AS clean_email, trim(p.name) AS trimmed"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::String("unknown".into()))
    );

    let mut gql_emitter = IsoGqlEmitter::new();
    let compiled_gql = gql_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled_gql.statement,
        "MATCH (p:Person) RETURN toLower(p.name) AS lower_name, coalesce(p.email, $p0) AS clean_email, trim(p.name) AS trimmed"
    );
}

#[test]
fn test_temporal_and_date_functions() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("tx"), vec!["Transaction"]);

    let now_dt = builder.function("datetime", vec![]);
    let field_ts = builder.prop("tx", "timestamp");
    let day_unit = builder.literal("day");
    let truncated_date = builder.function("date.truncate", vec![day_unit, field_ts]);

    builder
        .r#return()
        .custom_expr(now_dt, Some("current_time"))
        .custom_expr(truncated_date, Some("tx_day"));

    let (arena, root) = builder.build();

    let mut cypher_emitter = CypherEmitter::new();
    let compiled = cypher_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "MATCH (tx:Transaction) RETURN datetime() AS current_time, date.truncate($p0, tx.timestamp) AS tx_day"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::String("day".into()))
    );
}

#[test]
fn test_arithmetic_math_binary_expressions() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("item"), vec!["Product"]);

    let price = builder.prop("item", "price");
    let tax_rate = builder.literal(0.15);
    let discount = builder.literal(5.0);

    // price * 0.15
    let tax_amount = builder.math_expr(price, BinaryOp::Mul, tax_rate);
    // (price + (price * 0.15)) - 5.0
    let total_before_discount = builder.math_expr(price, BinaryOp::Add, tax_amount);
    let final_price = builder.math_expr(total_before_discount, BinaryOp::Sub, discount);

    builder
        .r#return()
        .custom_expr(final_price, Some("final_price"));

    let (arena, root) = builder.build();

    let mut cypher_emitter = CypherEmitter::new();
    let compiled = cypher_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "MATCH (item:Product) RETURN (item.price + (item.price * $p0)) - $p1 AS final_price"
    );
}

#[test]
fn test_unary_expressions_and_null_checks() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("p"), vec!["Person"]);

    let active_prop = builder.prop("p", "is_active");
    let not_active = builder.not_expr(active_prop);

    let email_prop = builder.prop("p", "email");
    let email_not_null = builder.is_not_null_expr(email_prop);

    let score_prop = builder.prop("p", "score");
    let neg_score = builder.neg_expr(score_prop);

    builder
        .where_expr(not_active)
        .where_expr(email_not_null)
        .r#return()
        .custom_expr(neg_score, Some("inverted_score"));

    let (arena, root) = builder.build();

    let mut cypher_emitter = CypherEmitter::new();
    let compiled = cypher_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "MATCH (p:Person) WHERE NOT (p.is_active) AND p.email IS NOT NULL RETURN -p.score AS inverted_score"
    );
}

#[test]
fn test_case_when_conditional_expressions() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("p"), vec!["Person"]);

    let age_prop = builder.prop("p", "age");
    let adult_limit = builder.literal(18);
    let senior_limit = builder.literal(65);

    let is_senior = builder.binary_expr(age_prop, BinaryOp::Gte, senior_limit);
    let senior_label = builder.literal("Senior");

    let is_adult = builder.binary_expr(age_prop, BinaryOp::Gte, adult_limit);
    let adult_label = builder.literal("Adult");

    let default_label = builder.literal("Minor");

    let case_expr = builder.case_when(
        None,
        vec![(is_senior, senior_label), (is_adult, adult_label)],
        Some(default_label),
    );

    builder
        .r#return()
        .custom_expr(case_expr, Some("age_category"));

    let (arena, root) = builder.build();

    let mut cypher_emitter = CypherEmitter::new();
    let compiled = cypher_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "MATCH (p:Person) RETURN CASE WHEN p.age >= $p0 THEN $p1 WHEN p.age >= $p2 THEN $p3 ELSE $p4 END AS age_category"
    );
}

#[test]
fn test_list_and_pattern_comprehensions() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("u"), vec!["User"]);

    // List comprehension: [x IN u.tags WHERE x STARTS WITH 'a' | toLower(x)]
    let tags_prop = builder.prop("u", "tags");
    let x_ident = builder.ident("x");
    let prefix = builder.literal("a");
    let filter = builder.binary_expr(x_ident, BinaryOp::StartsWith, prefix);
    let lower_x = builder.function("toLower", vec![x_ident]);

    let list_comp = builder.list_comprehension("x", tags_prop, Some(filter), Some(lower_x));

    // Pattern comprehension: [(u)-[:POSTED]->(p:Post) WHERE p.views > 100 | p.title]
    let sub_root = builder.subquery(|q| {
        q.node_var("u")
            .to_types(vec!["POSTED"])
            .node(Some("p"), vec!["Post"]);
    });

    let views_prop = builder.prop("p", "views");
    let min_views = builder.literal(100);
    let views_filter = builder.binary_expr(views_prop, BinaryOp::Gt, min_views);
    let title_prop = builder.prop("p", "title");

    let pattern_comp = builder.pattern_comprehension(sub_root, Some(views_filter), title_prop);

    builder
        .r#return()
        .custom_expr(list_comp, Some("filtered_tags"))
        .custom_expr(pattern_comp, Some("top_post_titles"));

    let (arena, root) = builder.build();

    let mut cypher_emitter = CypherEmitter::new();
    let compiled = cypher_emitter.visit_query(&arena, root).unwrap();
    assert!(
        compiled
            .statement
            .contains("[x IN u.tags WHERE x STARTS WITH $p0 | toLower(x)] AS filtered_tags")
    );
}

#[test]
fn test_existential_and_count_subqueries() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("u"), vec!["User"]);

    // Existential subquery: EXISTS { MATCH (u)-[:AUTHORED]->(p:Post) }
    let sub_path = builder.subquery(|q| {
        q.node_var("u")
            .to_types(vec!["AUTHORED"])
            .node(Some("p"), vec!["Post"]);
    });

    let exists_expr = builder.exists_subquery(sub_path);
    let count_expr = builder.count_subquery(sub_path);

    builder
        .where_expr(exists_expr)
        .r#return()
        .custom_expr(count_expr, Some("post_count"));

    let (arena, root) = builder.build();

    let mut cypher_emitter = CypherEmitter::new();
    let compiled = cypher_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "MATCH (u:User) WHERE EXISTS { MATCH (u)-[:AUTHORED]->(p:Post) } RETURN COUNT { MATCH (u)-[:AUTHORED]->(p:Post) } AS post_count"
    );
}

#[test]
fn test_branching_graph_topologies_multi_pattern() {
    let mut builder = QueryBuilder::new();
    // MATCH (u:User)-[:WORKS_AT]->(c:Company), (u)-[:LIVES_IN]->(city:City)
    builder
        .r#match()
        .node(Some("u"), vec!["User"])
        .to_types(vec!["WORKS_AT"])
        .node(Some("c"), vec!["Company"])
        .pattern()
        .node_var("u")
        .to_types(vec!["LIVES_IN"])
        .node(Some("city"), vec!["City"])
        .where_property(
            "c",
            "name",
            BinaryOp::Eq,
            LiteralValue::String("Google".into()),
        )
        .r#return()
        .field("u", "name", Some("user_name"))
        .field("city", "name", Some("city_name"));

    let (arena, root) = builder.build();

    let mut cypher_emitter = CypherEmitter::new();
    let compiled = cypher_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "MATCH (u:User)-[:WORKS_AT]->(c:Company), (u)-[:LIVES_IN]->(city:City) WHERE c.name = $p0 RETURN u.name AS user_name, city.name AS city_name"
    );
    assert_eq!(
        compiled.parameters.get("p0"),
        Some(&LiteralValue::String("Google".into()))
    );

    let mut age_emitter = AgeEmitter::new("my_graph");
    let compiled_age = age_emitter.visit_query(&arena, root).unwrap();
    assert!(compiled_age.statement.contains("SELECT * FROM cypher('my_graph', $$ MATCH (u:User)-[:WORKS_AT]->(c:Company), (u)-[:LIVES_IN]->(city:City) WHERE c.name = $p0 RETURN u.name AS user_name, city.name AS city_name $$, %s) AS (user_name agtype, city_name agtype)"));
}

#[test]
fn test_sql_pgq_rich_expressions() {
    let mut builder = QueryBuilder::new();
    builder.r#match().node(Some("p"), vec!["Person"]);

    let prop_name = builder.prop("p", "name");
    let upper_name = builder.function("UPPER", vec![prop_name]);

    builder
        .r#return()
        .custom_expr(upper_name, Some("upper_name"));

    let (arena, root) = builder.build();

    let mut pgq_emitter = SqlPgqEmitter::new("social_graph");
    let compiled = pgq_emitter.visit_query(&arena, root).unwrap();
    assert_eq!(
        compiled.statement,
        "SELECT * FROM GRAPH_TABLE (social_graph MATCH (p IS Person) COLUMNS (UPPER(p.name) AS upper_name))"
    );
}
