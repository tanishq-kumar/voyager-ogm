//! Dialect Query Emitters for openCypher, ISO GQL, SQL:2023 PGQ, and Apache AGE.

pub mod age;
pub mod cypher;
pub mod iso_gql;
pub mod sql_pgq;

pub use age::AgeEmitter;
pub use cypher::CypherEmitter;
pub use iso_gql::IsoGqlEmitter;
pub use sql_pgq::SqlPgqEmitter;

// TODO(RFC-0004): Migrate string-based label expressions in `labels: Vec<String>` to a first-class
// `AstNode::LabelExpression` AST node once the RFC-0004 AST grammar is fully implemented across FFI.
pub(crate) fn emit_label_expression(buffer: &mut String, labels: &[String], is_cypher: bool) {
    if labels.is_empty() {
        return;
    }
    if is_cypher {
        let has_expr = labels
            .iter()
            .any(|l| l.contains('|') || l.contains('&') || l.contains('!'));
        if !has_expr {
            for label in labels {
                buffer.push(':');
                buffer.push_str(label);
            }
            return;
        }
    }
    buffer.push(':');
    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            buffer.push('&');
        }
        let trimmed = label.trim();
        if let Some(inner) = trimmed.strip_prefix('!') {
            let inner_trimmed = inner.trim();
            if inner_trimmed.contains('|') && !inner_trimmed.starts_with('(') {
                let parts: Vec<&str> = inner_trimmed.split('|').map(|s| s.trim()).collect();
                buffer.push_str("!(");
                buffer.push_str(&parts.join("|"));
                buffer.push(')');
            } else {
                buffer.push('!');
                buffer.push_str(inner_trimmed);
            }
        } else if trimmed.contains('|') && !trimmed.starts_with('(') {
            let parts: Vec<&str> = trimmed.split('|').map(|s| s.trim()).collect();
            buffer.push('(');
            buffer.push_str(&parts.join("|"));
            buffer.push(')');
        } else {
            buffer.push_str(trimmed);
        }
    }
}
