//! Dialect Query Emitters for openCypher, ISO GQL, SQL:2023 PGQ, and Apache AGE.

pub mod age;
pub mod cypher;
pub mod ddl;
pub mod iso_gql;
pub mod sql_pgq;

pub use age::AgeEmitter;
pub use cypher::CypherEmitter;
pub use ddl::{
    emit_cypher_drop_node_ddl, emit_cypher_drop_rel_ddl, emit_cypher_node_ddl, emit_cypher_rel_ddl,
    emit_cypher25_drop_graph_type_ddl, emit_cypher25_graph_type_ddl, emit_gql_alter_node_ddl,
    emit_gql_alter_rel_ddl, emit_gql_drop_graph_type_ddl, emit_gql_graph_type_ddl,
    emit_node_constraint_ddl, emit_node_drop_constraint_ddl, emit_node_drop_index_ddl,
    emit_node_index_ddl, emit_pgq_drop_property_graph_ddl, emit_pgq_property_graph_ddl,
    emit_rel_constraint_ddl, emit_rel_drop_constraint_ddl, emit_rel_drop_index_ddl,
    emit_rel_index_ddl,
};
pub use iso_gql::IsoGqlEmitter;
pub use sql_pgq::SqlPgqEmitter;

use crate::ast::LabelExpression;

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
        format_cypher_label_expr(buffer, expr);
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
        format_cypher_label_expr(buffer, expr);
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
            buffer.push_str("NOT ");
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
