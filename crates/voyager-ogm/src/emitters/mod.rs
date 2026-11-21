//! Dialect Query Emitters for openCypher, ISO GQL, SQL:2023 PGQ, and Apache AGE.

pub mod age;
pub mod cypher;
pub mod ddl;
pub mod iso_gql;
pub mod sql_pgq;

pub use age::{AgeEmitter, DEFAULT_AGE_GRAPH, is_valid_age_graph_name};
pub use cypher::CypherEmitter;
pub use ddl::{
    emit_cypher_drop_node_ddl, emit_cypher_drop_rel_ddl, emit_cypher_node_ddl, emit_cypher_rel_ddl,
    emit_cypher25_drop_graph_type_ddl, emit_cypher25_graph_type_ddl, emit_gql_alter_node_ddl,
    emit_gql_alter_rel_ddl, emit_gql_drop_graph_type_ddl, emit_gql_graph_type_ddl,
    emit_node_constraint_ddl, emit_node_constraint_ddl_with_graph, emit_node_drop_constraint_ddl,
    emit_node_drop_constraint_ddl_with_graph, emit_node_drop_index_ddl,
    emit_node_drop_index_ddl_with_graph, emit_node_index_ddl, emit_node_index_ddl_with_graph,
    emit_pgq_drop_property_graph_ddl, emit_pgq_property_graph_ddl, emit_rel_constraint_ddl,
    emit_rel_constraint_ddl_with_graph, emit_rel_drop_constraint_ddl,
    emit_rel_drop_constraint_ddl_with_graph, emit_rel_drop_index_ddl,
    emit_rel_drop_index_ddl_with_graph, emit_rel_index_ddl, emit_rel_index_ddl_with_graph,
};
pub use iso_gql::IsoGqlEmitter;
pub use sql_pgq::SqlPgqEmitter;

use crate::ast::{AstNode, LabelExpression, NodeHandle, QueryAstArena};
use crate::error::{Error, Result};
use std::collections::HashSet;

/// Returns `true` when `value` is a plain identifier.
///
/// A plain identifier starts with a Unicode letter or `_` and continues with
/// Unicode letters, digits, or `_`. Unicode letters are accepted because Cypher
/// and GQL allow them, and rejecting them would break existing valid labels.
pub(crate) fn is_plain_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_alphanumeric())
}

/// Returns `true` when `value` is one or more plain identifiers joined by single dots.
///
/// Examples: `toLower`, `date.truncate`, `apoc.coll.sum`.
pub(crate) fn is_dotted_identifier(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(is_plain_identifier)
}

fn invalid_identifier(kind: &str, value: &str) -> Error {
    Error::InvalidClause(format!(
        "Invalid {kind} {value:?}: identifiers must start with a Unicode letter or '_' and contain only Unicode letters, digits, and '_'"
    ))
}

fn invalid_dotted_identifier(kind: &str, value: &str) -> Error {
    Error::InvalidClause(format!(
        "Invalid {kind} {value:?}: expected one or more identifiers joined by single dots (for example 'toLower', 'date.truncate', or 'apoc.coll.sum')"
    ))
}

fn validate_identifier(kind: &str, value: &str) -> Result<()> {
    if is_plain_identifier(value) {
        Ok(())
    } else {
        Err(invalid_identifier(kind, value))
    }
}

fn validate_dotted_identifier(kind: &str, value: &str) -> Result<()> {
    if is_dotted_identifier(value) {
        Ok(())
    } else {
        Err(invalid_dotted_identifier(kind, value))
    }
}

fn validate_yield_item(value: &str) -> Result<()> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    match parts.as_slice() {
        [name] => validate_identifier("yield item", name),
        [name, as_keyword, alias] if as_keyword.eq_ignore_ascii_case("AS") => {
            validate_identifier("yield item", name)?;
            validate_identifier("yield alias", alias)
        }
        _ => Err(Error::InvalidClause(format!(
            "Invalid yield item {value:?}: expected '<identifier>' or '<identifier> AS <identifier>'"
        ))),
    }
}

fn validate_label_expression(kind: &str, expr: &LabelExpression) -> Result<()> {
    match expr {
        LabelExpression::Label(label) => validate_identifier(kind, label),
        LabelExpression::And(left, right) | LabelExpression::Or(left, right) => {
            validate_label_expression(kind, left)?;
            validate_label_expression(kind, right)
        }
        LabelExpression::Not(inner) => validate_label_expression(kind, inner),
        LabelExpression::Wildcard => Ok(()),
    }
}

/// Validates every identifier reachable from `root` before it is written into query text.
///
/// This is the single pre-emission choke point shared by every dialect emitter, so
/// fluent-API queries and `compile_query_from_spec` queries are both covered.
pub(crate) fn validate_query_identifiers(arena: &QueryAstArena, root: NodeHandle) -> Result<()> {
    let mut visited = HashSet::new();
    validate_node_identifiers(arena, root, &mut visited)
}

fn validate_projection_identifiers(
    arena: &QueryAstArena,
    projection: &crate::ast::ProjectionItem,
    visited: &mut HashSet<NodeHandle>,
) -> Result<()> {
    if let Some(alias) = &projection.alias {
        validate_identifier("alias", alias)?;
    }
    validate_node_identifiers(arena, projection.expression, visited)
}

fn validate_optional_handle(
    arena: &QueryAstArena,
    handle: Option<NodeHandle>,
    visited: &mut HashSet<NodeHandle>,
) -> Result<()> {
    if let Some(handle) = handle {
        validate_node_identifiers(arena, handle, visited)?;
    }
    Ok(())
}

fn validate_handle_list(
    arena: &QueryAstArena,
    handles: &[NodeHandle],
    visited: &mut HashSet<NodeHandle>,
) -> Result<()> {
    for &handle in handles {
        validate_node_identifiers(arena, handle, visited)?;
    }
    Ok(())
}

fn validate_node_identifiers(
    arena: &QueryAstArena,
    handle: NodeHandle,
    visited: &mut HashSet<NodeHandle>,
) -> Result<()> {
    if !visited.insert(handle) {
        return Ok(());
    }
    match arena.get(handle)? {
        AstNode::NodePattern {
            variable,
            label_expression,
            predicates,
        } => {
            if let Some(variable) = variable {
                validate_identifier("node variable", variable)?;
            }
            if let Some(expr) = label_expression {
                validate_label_expression("node label", expr)?;
            }
            validate_handle_list(arena, predicates, visited)?;
        }
        AstNode::EdgePattern {
            variable,
            label_expression,
            predicates,
            target_node,
            ..
        } => {
            if let Some(variable) = variable {
                validate_identifier("edge variable", variable)?;
            }
            if let Some(expr) = label_expression {
                validate_label_expression("relationship type", expr)?;
            }
            validate_handle_list(arena, predicates, visited)?;
            validate_node_identifiers(arena, *target_node, visited)?;
        }
        AstNode::PathChain {
            path_variable,
            start_node,
            edges,
            ..
        } => {
            if let Some(variable) = path_variable {
                validate_identifier("path variable", variable)?;
            }
            validate_node_identifiers(arena, *start_node, visited)?;
            validate_handle_list(arena, edges, visited)?;
        }
        AstNode::BinaryExpression { left, right, .. } => {
            validate_node_identifiers(arena, *left, visited)?;
            validate_node_identifiers(arena, *right, visited)?;
        }
        AstNode::UnaryExpression { operand, .. } => {
            validate_node_identifiers(arena, *operand, visited)?;
        }
        AstNode::FunctionCall { name, arguments } => {
            validate_dotted_identifier("function name", name)?;
            validate_handle_list(arena, arguments, visited)?;
        }
        AstNode::CaseExpression {
            operand,
            when_then_branches,
            else_branch,
        } => {
            validate_optional_handle(arena, *operand, visited)?;
            for (when_expr, then_expr) in when_then_branches {
                validate_node_identifiers(arena, *when_expr, visited)?;
                validate_node_identifiers(arena, *then_expr, visited)?;
            }
            validate_optional_handle(arena, *else_branch, visited)?;
        }
        AstNode::ListComprehension {
            variable,
            list_expression,
            where_filter,
            map_expression,
        } => {
            validate_identifier("list comprehension variable", variable)?;
            validate_node_identifiers(arena, *list_expression, visited)?;
            validate_optional_handle(arena, *where_filter, visited)?;
            validate_optional_handle(arena, *map_expression, visited)?;
        }
        AstNode::PatternComprehension {
            path,
            where_filter,
            projection,
        } => {
            validate_node_identifiers(arena, *path, visited)?;
            validate_optional_handle(arena, *where_filter, visited)?;
            validate_node_identifiers(arena, *projection, visited)?;
        }
        AstNode::ExistsSubquery { subquery } | AstNode::CountSubquery { subquery } => {
            validate_node_identifiers(arena, *subquery, visited)?;
        }
        AstNode::ListLiteral(items) => {
            validate_handle_list(arena, items, visited)?;
        }
        AstNode::PropertyAccess { target, property } => {
            validate_node_identifiers(arena, *target, visited)?;
            if !property.is_empty() {
                validate_identifier("property name", property)?;
            }
        }
        AstNode::Literal(_) => {}
        // `col("n.uuid")` from the fluent API is emitted verbatim as a variable/property path,
        // so identifier expressions accept dotted names while still rejecting injection syntax.
        AstNode::Identifier(identifier) => validate_dotted_identifier("identifier", identifier)?,
        AstNode::Parameter(parameter) => validate_identifier("parameter name", parameter)?,
        AstNode::UnwindClause { expression, alias } => {
            validate_node_identifiers(arena, *expression, visited)?;
            validate_identifier("unwind alias", alias)?;
        }
        AstNode::WhereClause { root_predicate } => {
            validate_node_identifiers(arena, *root_predicate, visited)?;
        }
        AstNode::MatchClause {
            paths,
            where_clause,
            ..
        } => {
            validate_handle_list(arena, paths, visited)?;
            validate_optional_handle(arena, *where_clause, visited)?;
        }
        AstNode::ReturnClause {
            projections,
            order_by,
            ..
        } => {
            for projection in projections {
                validate_projection_identifiers(arena, projection, visited)?;
            }
            for (expression, _) in order_by {
                validate_node_identifiers(arena, *expression, visited)?;
            }
        }
        AstNode::ProcedureCall {
            namespace,
            procedure,
            arguments,
            yield_items,
            ..
        } => {
            if let Some(namespace) = namespace {
                validate_dotted_identifier("procedure namespace", namespace)?;
            }
            validate_identifier("procedure name", procedure)?;
            validate_handle_list(arena, arguments, visited)?;
            for item in yield_items {
                validate_yield_item(item)?;
            }
        }
        AstNode::CreateClause { paths } => {
            validate_handle_list(arena, paths, visited)?;
        }
        AstNode::MergeClause {
            path,
            on_create_set,
            on_match_set,
        } => {
            validate_node_identifiers(arena, *path, visited)?;
            validate_handle_list(arena, on_create_set, visited)?;
            validate_handle_list(arena, on_match_set, visited)?;
        }
        AstNode::SetClause { items } => {
            validate_handle_list(arena, items, visited)?;
        }
        AstNode::SetItem { target, value, .. } => {
            validate_node_identifiers(arena, *target, visited)?;
            validate_node_identifiers(arena, *value, visited)?;
        }
        AstNode::DeleteClause { targets, .. } => {
            for &target in targets {
                if let AstNode::Identifier(identifier) = arena.get(target)? {
                    validate_identifier("delete target", identifier)?;
                } else {
                    validate_node_identifiers(arena, target, visited)?;
                }
            }
        }
        AstNode::RemoveClause { items } => {
            validate_handle_list(arena, items, visited)?;
        }
        AstNode::LoadCsvClause { url, alias, .. } => {
            validate_node_identifiers(arena, *url, visited)?;
            validate_identifier("load csv alias", alias)?;
        }
        AstNode::WithClause {
            projections,
            order_by,
            where_clause,
            ..
        } => {
            for projection in projections {
                validate_projection_identifiers(arena, projection, visited)?;
            }
            for (expression, _) in order_by {
                validate_node_identifiers(arena, *expression, visited)?;
            }
            validate_optional_handle(arena, *where_clause, visited)?;
        }
        AstNode::LetClause {
            variable,
            expression,
        } => {
            validate_identifier("let variable", variable)?;
            validate_node_identifiers(arena, *expression, visited)?;
        }
        AstNode::FilterClause { predicate } => {
            validate_node_identifiers(arena, *predicate, visited)?;
        }
        AstNode::QueryStatement {
            load_csv,
            unwinds,
            matches,
            linear_clauses,
            with_clauses,
            mutations,
            return_clause,
            ..
        } => {
            validate_optional_handle(arena, *load_csv, visited)?;
            validate_handle_list(arena, unwinds, visited)?;
            validate_handle_list(arena, matches, visited)?;
            validate_handle_list(arena, linear_clauses, visited)?;
            validate_handle_list(arena, with_clauses, visited)?;
            validate_handle_list(arena, mutations, visited)?;
            validate_optional_handle(arena, *return_clause, visited)?;
        }
    }
    Ok(())
}

/// Emits a node label expression for openCypher.
///
/// For simple conjunctions of labels (e.g. `Person & Developer`), emits standard colon-separated
/// labels `:Person:Developer` for universal backwards compatibility.
/// For complex boolean label expressions (e.g. `(Person | Company) & !Inactive`), emits
/// standard Cypher 9 / 25 label expressions.
pub(crate) fn emit_cypher_node_labels(buffer: &mut String, expr: Option<&LabelExpression>) {
    let Some(expr) = expr else { return };
    if let Some(labels) = expr.to_conjunction_labels() {
        for label in labels {
            buffer.push(':');
            buffer.push_str(&label);
        }
    } else {
        buffer.push(':');
        let wrap_top_or = matches!(expr, LabelExpression::Or(_, _));
        if wrap_top_or {
            buffer.push('(');
        }
        format_cypher_label_expr(buffer, expr);
        if wrap_top_or {
            buffer.push(')');
        }
    }
}

/// Emits an edge label expression for openCypher: `[:TYPE1|TYPE2]`.
pub(crate) fn emit_cypher_edge_labels(buffer: &mut String, expr: Option<&LabelExpression>) {
    let Some(expr) = expr else { return };
    buffer.push(':');
    if let Some(types) = expr.to_disjunction_labels() {
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                buffer.push('|');
            }
            buffer.push_str(t);
        }
    } else {
        format_cypher_label_expr(buffer, expr);
    }
}

/// Emits a node label expression for ISO GQL.
///
/// For simple conjunctions of multiple labels, emits `:Label1&Label2`.
/// For complex boolean expressions, emits Cypher 9 / GQL standard syntax.
pub(crate) fn emit_gql_node_labels(buffer: &mut String, expr: Option<&LabelExpression>) {
    let Some(expr) = expr else { return };
    if let Some(labels) = expr.to_conjunction_labels() {
        buffer.push(':');
        for (i, label) in labels.iter().enumerate() {
            if i > 0 {
                buffer.push('&');
            }
            buffer.push_str(label);
        }
    } else {
        buffer.push(':');
        let wrap_top_or = matches!(expr, LabelExpression::Or(_, _));
        if wrap_top_or {
            buffer.push('(');
        }
        format_cypher_label_expr(buffer, expr);
        if wrap_top_or {
            buffer.push(')');
        }
    }
}

/// Emits an edge label expression for ISO GQL: `[:TYPE1|TYPE2]`.
pub(crate) fn emit_gql_edge_labels(buffer: &mut String, expr: Option<&LabelExpression>) {
    let Some(expr) = expr else { return };
    buffer.push(':');
    if let Some(types) = expr.to_disjunction_labels() {
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                buffer.push('|');
            }
            buffer.push_str(t);
        }
    } else {
        format_cypher_label_expr(buffer, expr);
    }
}

/// Emits a node label expression for SQL:2023 PGQ.
///
/// For simple conjunctions, emits ` IS Label1 IS Label2...`.
/// For complex expressions, emits ` IS <formatted_expr>`.
pub(crate) fn emit_pgq_node_labels(buffer: &mut String, expr: Option<&LabelExpression>) {
    let Some(expr) = expr else { return };
    if let Some(labels) = expr.to_conjunction_labels() {
        for label in labels {
            buffer.push_str(" IS ");
            buffer.push_str(&label);
        }
    } else {
        buffer.push_str(" IS ");
        format_pgq_label_expr(buffer, expr);
    }
}

/// Emits an edge label expression for SQL:2023 PGQ: `-[r IS TYPE1 | TYPE2]->`.
pub(crate) fn emit_pgq_edge_labels(buffer: &mut String, expr: Option<&LabelExpression>) {
    let Some(expr) = expr else { return };
    buffer.push_str(" IS ");
    if let Some(types) = expr.to_disjunction_labels() {
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                buffer.push_str(" | ");
            }
            buffer.push_str(t);
        }
    } else {
        format_pgq_label_expr(buffer, expr);
    }
}

fn format_cypher_label_expr(buffer: &mut String, expr: &LabelExpression) {
    match expr {
        LabelExpression::Label(s) => buffer.push_str(s),
        LabelExpression::Wildcard => buffer.push('%'),
        LabelExpression::Not(inner) => {
            buffer.push('!');
            match &**inner {
                LabelExpression::And(_, _) | LabelExpression::Or(_, _) => {
                    buffer.push('(');
                    format_cypher_label_expr(buffer, inner);
                    buffer.push(')');
                }
                _ => format_cypher_label_expr(buffer, inner),
            }
        }
        LabelExpression::And(left, right) => {
            let wrap_left = matches!(&**left, LabelExpression::Or(_, _));
            if wrap_left {
                buffer.push('(');
            }
            format_cypher_label_expr(buffer, left);
            if wrap_left {
                buffer.push(')');
            }

            buffer.push('&');

            let wrap_right = matches!(&**right, LabelExpression::Or(_, _));
            if wrap_right {
                buffer.push('(');
            }
            format_cypher_label_expr(buffer, right);
            if wrap_right {
                buffer.push(')');
            }
        }
        LabelExpression::Or(left, right) => {
            format_cypher_label_expr(buffer, left);
            buffer.push('|');
            format_cypher_label_expr(buffer, right);
        }
    }
}

fn format_pgq_label_expr(buffer: &mut String, expr: &LabelExpression) {
    match expr {
        LabelExpression::Label(s) => buffer.push_str(s),
        LabelExpression::Wildcard => buffer.push('%'),
        LabelExpression::Not(inner) => {
            buffer.push('!');
            match &**inner {
                LabelExpression::And(_, _) | LabelExpression::Or(_, _) => {
                    buffer.push('(');
                    format_pgq_label_expr(buffer, inner);
                    buffer.push(')');
                }
                _ => format_pgq_label_expr(buffer, inner),
            }
        }
        LabelExpression::And(left, right) => {
            let wrap_left = matches!(&**left, LabelExpression::Or(_, _));
            if wrap_left {
                buffer.push('(');
            }
            format_pgq_label_expr(buffer, left);
            if wrap_left {
                buffer.push(')');
            }

            buffer.push_str(" & ");

            let wrap_right = matches!(&**right, LabelExpression::Or(_, _));
            if wrap_right {
                buffer.push('(');
            }
            format_pgq_label_expr(buffer, right);
            if wrap_right {
                buffer.push(')');
            }
        }
        LabelExpression::Or(left, right) => {
            format_pgq_label_expr(buffer, left);
            buffer.push_str(" | ");
            format_pgq_label_expr(buffer, right);
        }
    }
}

#[cfg(test)]
mod identifier_validation_tests {
    use super::{
        is_dotted_identifier, is_plain_identifier, validate_identifier, validate_label_expression,
    };
    use crate::ast::LabelExpression;

    #[test]
    fn accepts_plain_identifiers_including_unicode_letters() {
        for value in [
            "Person",
            "_person_0",
            "p0",
            "Ünïcode",
            "日本語",
            "_",
            "a1",
            "λ",
        ] {
            assert!(
                is_plain_identifier(value),
                "expected valid identifier: {value:?}"
            );
        }
    }

    #[test]
    fn rejects_non_identifier_values() {
        for value in [
            "",
            "1abc",
            "a b",
            "a-b",
            "a.b",
            "a$b",
            "a;b",
            "`a`",
            "a()",
            "a{b}",
            "a/b",
            "Person) DETACH DELETE p //",
        ] {
            assert!(
                !is_plain_identifier(value),
                "expected invalid identifier: {value:?}"
            );
        }
    }

    #[test]
    fn accepts_dotted_function_names() {
        for value in [
            "toLower",
            "date.truncate",
            "apoc.coll.sum",
            "db.index.vector.queryNodes",
        ] {
            assert!(
                is_dotted_identifier(value),
                "expected valid function name: {value:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_function_names() {
        for value in [
            "",
            ".",
            "a.",
            ".a",
            "a..b",
            "toLower(1)) RETURN 1 //",
            "to-lower",
            "to lower",
            "$toLower",
        ] {
            assert!(
                !is_dotted_identifier(value),
                "expected invalid function name: {value:?}"
            );
        }
    }

    #[test]
    fn validates_label_expressions_recursively() {
        let valid = LabelExpression::parse("(Person | Company) & !Inactive").unwrap();
        validate_label_expression("node label", &valid).unwrap();

        let wildcard = LabelExpression::wildcard();
        validate_label_expression("node label", &wildcard).unwrap();

        let invalid = LabelExpression::label("Person) DETACH DELETE p //");
        let error = validate_label_expression("node label", &invalid).unwrap_err();
        assert_eq!(error.code(), crate::error::ErrorCode::InvalidClause);
        assert!(error.to_string().contains("node label"));
        assert!(error.to_string().contains("Person) DETACH DELETE p //"));
    }

    #[test]
    fn identifier_expressions_allow_dotted_variable_property_paths() {
        // The Python `col("n.uuid")` helper is emitted verbatim as a dotted identifier.
        assert!(is_dotted_identifier("n.uuid"));
        assert!(is_dotted_identifier("n.address.city"));
        assert!(!is_dotted_identifier("n.uuid) DETACH DELETE n //"));
    }

    #[test]
    fn reports_the_offending_value_and_kind() {
        let error = validate_identifier("parameter name", "x RETURN 1 //").unwrap_err();
        assert_eq!(error.code(), crate::error::ErrorCode::InvalidClause);
        assert!(error.to_string().contains("parameter name"));
        assert!(error.to_string().contains("x RETURN 1 //"));
    }
}
