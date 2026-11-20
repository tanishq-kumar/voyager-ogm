//! PostgreSQL / Apache AGE parameter binding preparation.
//!
//! The native client must never splice parameter values into SQL text. This
//! module rewrites statement placeholders into PostgreSQL extended-protocol
//! positional placeholders and encodes each value for `Bind` (text format).
//!
//! Two placeholder styles are supported:
//!
//! * Apache AGE compiled statements keep a single driver `%s` placeholder for
//!   the agtype parameter map outside their `$$ ... $$` Cypher body. It becomes
//!   `$1` and the entire parameter map is bound as one JSON text value.
//! * Everything else uses the compiled `$name` placeholders (`$p0`, `$batch`,
//!   ...). They become `$1`, `$2`, ... in order of first appearance. Repeated
//!   names reuse the same positional placeholder.
//!
//! Scanning skips PostgreSQL string literals, quoted identifiers, dollar-quoted
//! bodies (`$$ ... $$`), and comments, so `%s` and `$name` text inside a Cypher
//! body is left untouched.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::error::{NetError, Result};

/// Placeholder convention detected in a compiled statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterPlaceholderStyle {
    /// No parameters were bound.
    None,
    /// Apache AGE `%s` placeholder for the agtype parameter map.
    AgeParameterMap,
    /// Named `$name` placeholders bound positionally.
    Named,
}

/// A statement ready for a PostgreSQL extended-query `Parse` / `Bind` cycle.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedPostgresStatement {
    /// SQL text with placeholders rewritten to `$1`, `$2`, ...
    pub statement: String,
    /// Parameter type OIDs supplied to `Parse` (0 lets the server infer).
    pub parameter_oids: Vec<i32>,
    /// Text-format parameter values supplied to `Bind` (`None` is SQL NULL).
    pub parameter_values: Vec<Option<Vec<u8>>>,
    /// Placeholder convention that was detected.
    pub placeholder_style: ParameterPlaceholderStyle,
}

impl PreparedPostgresStatement {
    /// Returns `true` when at least one parameter must be sent through `Bind`.
    pub fn is_parameterized(&self) -> bool {
        !self.parameter_values.is_empty()
    }
}

fn unparameterized(statement: &str) -> PreparedPostgresStatement {
    PreparedPostgresStatement {
        statement: statement.to_string(),
        parameter_oids: Vec::new(),
        parameter_values: Vec::new(),
        placeholder_style: ParameterPlaceholderStyle::None,
    }
}

/// Prepares a PostgreSQL / Apache AGE statement for the extended query protocol.
///
/// The returned SQL text never contains a caller-supplied parameter value.
pub fn prepare_postgres_statement(
    query: &str,
    params: &HashMap<String, Value>,
) -> Result<PreparedPostgresStatement> {
    if params.is_empty() {
        return Ok(unparameterized(query));
    }

    if looks_like_age_statement(query) {
        let (rewritten, age_placeholders) = rewrite_age_parameter_placeholders(query);
        if age_placeholders > 0 {
            return Ok(PreparedPostgresStatement {
                statement: rewritten,
                // OID 0 lets PostgreSQL resolve the third `cypher()` argument to
                // agtype, exactly as the psycopg bridge's unknown text value does.
                parameter_oids: vec![0],
                parameter_values: vec![Some(age_parameter_map(params)?)],
                placeholder_style: ParameterPlaceholderStyle::AgeParameterMap,
            });
        }
    }

    let (statement, values) = rewrite_named_placeholders(query, params);
    if values.is_empty() {
        return Ok(unparameterized(query));
    }

    Ok(PreparedPostgresStatement {
        statement,
        parameter_oids: vec![0; values.len()],
        parameter_values: values,
        placeholder_style: ParameterPlaceholderStyle::Named,
    })
}

/// Returns `true` for statements emitted by [`voyager_ogm::emitters::AgeEmitter`].
fn looks_like_age_statement(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    lower.contains("cypher(") || lower.contains("ag_catalog.cypher")
}

/// Serializes the full AGE parameter map into a deterministic JSON object.
fn age_parameter_map(params: &HashMap<String, Value>) -> Result<Vec<u8>> {
    let sorted: BTreeMap<&str, &Value> = params
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect();
    serde_json::to_vec(&sorted)
        .map_err(|e| NetError::ProtocolError(format!("Failed to encode AGE parameter map: {e}")))
}

/// Rewrites the AGE `%s` driver placeholder (outside `$$ ... $$`) to `$1`.
///
/// AGE text is left untouched. Returns the statement and the number of
/// placeholders rewritten.
fn rewrite_age_parameter_placeholders(query: &str) -> (String, usize) {
    let bytes = query.as_bytes();
    let mut out = String::with_capacity(query.len());
    let mut count = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                let end = skip_single_quoted(bytes, i);
                push_slice(&mut out, query, i, end);
                i = end;
            }
            b'"' => {
                let end = skip_double_quoted(bytes, i);
                push_slice(&mut out, query, i, end);
                i = end;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                let end = skip_line_comment(bytes, i);
                push_slice(&mut out, query, i, end);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = skip_block_comment(bytes, i);
                push_slice(&mut out, query, i, end);
                i = end;
            }
            b'$' => {
                if let Some((opener_len, delimiter)) = dollar_quote_opener(bytes, i) {
                    let end = dollar_quote_end(query, i + opener_len, &delimiter);
                    push_slice(&mut out, query, i, end);
                    i = end;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            b'%' if bytes.get(i + 1) == Some(&b's') => {
                out.push_str("$1");
                count += 1;
                i += 2;
            }
            _ => {
                push_char(&mut out, query, i);
                i += char_len(query, i);
            }
        }
    }

    (out, count)
}

/// Rewrites named `$name` placeholders to positional `$1`, `$2`, ... values.
fn rewrite_named_placeholders(
    query: &str,
    params: &HashMap<String, Value>,
) -> (String, Vec<Option<Vec<u8>>>) {
    let bytes = query.as_bytes();
    let mut out = String::with_capacity(query.len());
    let mut values: Vec<Option<Vec<u8>>> = Vec::new();
    let mut indices: HashMap<String, usize> = HashMap::new();
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                let end = skip_single_quoted(bytes, i);
                push_slice(&mut out, query, i, end);
                i = end;
            }
            b'"' => {
                let end = skip_double_quoted(bytes, i);
                push_slice(&mut out, query, i, end);
                i = end;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                let end = skip_line_comment(bytes, i);
                push_slice(&mut out, query, i, end);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = skip_block_comment(bytes, i);
                push_slice(&mut out, query, i, end);
                i = end;
            }
            b'$' => {
                if let Some((opener_len, delimiter)) = dollar_quote_opener(bytes, i) {
                    let end = dollar_quote_end(query, i + opener_len, &delimiter);
                    push_slice(&mut out, query, i, end);
                    i = end;
                    continue;
                }

                // A `$` inside an unquoted identifier (PostgreSQL permits
                // `col$name`) is not a parameter placeholder.
                if i > 0 && (is_placeholder_name_byte(bytes[i - 1]) || bytes[i - 1] == b'$') {
                    out.push('$');
                    i += 1;
                    continue;
                }

                let name_start = i + 1;
                let mut name_end = name_start;
                while name_end < bytes.len() && is_placeholder_name_byte(bytes[name_end]) {
                    name_end += 1;
                }

                if name_end > name_start {
                    let name = &query[name_start..name_end];
                    if let Some(value) = params.get(name) {
                        let index = match indices.get(name) {
                            Some(index) => *index,
                            None => {
                                let index = values.len() + 1;
                                indices.insert(name.to_string(), index);
                                values.push(value_to_text(value));
                                index
                            }
                        };
                        out.push('$');
                        out.push_str(&index.to_string());
                    } else {
                        push_slice(&mut out, query, i, name_end);
                    }
                    i = name_end;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            _ => {
                push_char(&mut out, query, i);
                i += char_len(query, i);
            }
        }
    }

    (out, values)
}

/// Encodes a JSON parameter as PostgreSQL text-format `Bind` bytes.
fn value_to_text(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Bool(b) => Some(if *b {
            b"true".to_vec()
        } else {
            b"false".to_vec()
        }),
        Value::Number(n) => Some(n.to_string().into_bytes()),
        // Nested maps and lists are sent as JSON text. With an unspecified
        // parameter OID the server infers (and parses) the target type.
        other => serde_json::to_vec(other).ok(),
    }
}

fn is_placeholder_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn push_slice(out: &mut String, query: &str, start: usize, end: usize) {
    if let Some(slice) = query.get(start..end) {
        out.push_str(slice);
    }
}

fn char_len(query: &str, index: usize) -> usize {
    query[index..].chars().next().map_or(1, char::len_utf8)
}

fn push_char(out: &mut String, query: &str, index: usize) {
    if let Some(ch) = query[index..].chars().next() {
        out.push(ch);
    }
}

fn skip_single_quoted(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if bytes.get(i + 1) == Some(&b'\'') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_double_quoted(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if bytes.get(i + 1) == Some(&b'"') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    let mut i = start + 2;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// Detects a PostgreSQL dollar-quote opener (`$$` or `$tag$`).
fn dollar_quote_opener(bytes: &[u8], start: usize) -> Option<(usize, String)> {
    if bytes.get(start) != Some(&b'$') {
        return None;
    }
    let mut i = start + 1;
    if bytes.get(i) == Some(&b'$') {
        return Some((2, "$$".to_string()));
    }
    if !bytes
        .get(i)
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
    {
        return None;
    }
    i += 1;
    while i < bytes.len() && is_placeholder_name_byte(bytes[i]) {
        i += 1;
    }
    if bytes.get(i) == Some(&b'$') {
        let delimiter = String::from_utf8_lossy(&bytes[start..=i]).into_owned();
        return Some((i + 1 - start, delimiter));
    }
    None
}

/// Returns the byte index just past the closing dollar-quote delimiter.
fn dollar_quote_end(query: &str, body_start: usize, delimiter: &str) -> usize {
    match query[body_start..].find(delimiter) {
        Some(offset) => body_start + offset + delimiter.len(),
        None => query.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn params(entries: &[(&str, Value)]) -> HashMap<String, Value> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn age_placeholder_becomes_positional_and_binds_json_map() {
        let query = "SELECT * FROM cypher('g', $$ MATCH (n) WHERE n.name = $p0 RETURN n $$, %s) AS (n agtype)";
        let prepared = prepare_postgres_statement(query, &params(&[("p0", json!("Alice"))]))
            .expect("prepare should succeed");

        assert_eq!(
            prepared.statement,
            "SELECT * FROM cypher('g', $$ MATCH (n) WHERE n.name = $p0 RETURN n $$, $1) AS (n agtype)"
        );
        assert_eq!(
            prepared.placeholder_style,
            ParameterPlaceholderStyle::AgeParameterMap
        );
        assert_eq!(prepared.parameter_oids, vec![0]);
        let value = prepared.parameter_values[0].as_ref().expect("bound value");
        assert_eq!(value, br#"{"p0":"Alice"}"#);
    }

    #[test]
    fn age_rewrite_ignores_percent_s_inside_body_strings_and_comments() {
        let query = "SELECT * FROM cypher('g', $$ RETURN '$p0 %s' $$, %s) AS (v agtype) -- %s\n";
        let prepared = prepare_postgres_statement(query, &params(&[("p0", json!("x"))]))
            .expect("prepare should succeed");

        assert!(prepared.statement.contains("RETURN '$p0 %s'"));
        assert!(prepared.statement.contains(", $1)"));
        assert!(prepared.statement.ends_with("-- %s\n"));
        assert!(!prepared.statement.contains(", %s)"));
    }

    #[test]
    fn age_parameter_map_supports_quotes_dollars_backslashes_unicode_and_nulls() {
        let query = "SELECT * FROM cypher('g', $$ RETURN $p0 $$, %s) AS (v agtype)";
        let prepared = prepare_postgres_statement(
            query,
            &params(&[
                ("p0", json!("x') ; DROP TABLE t; --")),
                ("p1", json!("$$ \\ ünïcode")),
                ("p2", json!({"nested": {"list": [1, null, "a'b"]}})),
            ]),
        )
        .expect("prepare should succeed");

        let value = prepared.parameter_values[0].as_ref().expect("bound value");
        let decoded: Value = serde_json::from_slice(value).expect("valid JSON");
        assert_eq!(
            decoded,
            json!({
                "p0": "x') ; DROP TABLE t; --",
                "p1": "$$ \\ ünïcode",
                "p2": {"nested": {"list": [1, null, "a'b"]}},
            })
        );
        // The raw value must never appear in the statement text.
        assert!(!prepared.statement.contains("DROP TABLE"));
        assert!(!prepared.statement.contains("ünïcode"));
    }

    #[test]
    fn named_placeholders_bind_positionally_without_prefix_collisions() {
        let query = "SELECT * FROM t WHERE a = $p10 AND b = $p1 AND c = $p10 AND d = $p2";
        let prepared = prepare_postgres_statement(
            query,
            &params(&[
                ("p1", json!(42)),
                ("p10", json!(100)),
                ("p2", json!("O'Reilly")),
            ]),
        )
        .expect("prepare should succeed");

        assert_eq!(
            prepared.statement,
            "SELECT * FROM t WHERE a = $1 AND b = $2 AND c = $1 AND d = $3"
        );
        assert_eq!(
            prepared.parameter_values,
            vec![
                Some(b"100".to_vec()),
                Some(b"42".to_vec()),
                Some(b"O'Reilly".to_vec()),
            ]
        );
        assert_eq!(prepared.parameter_oids, vec![0, 0, 0]);
        assert_eq!(prepared.placeholder_style, ParameterPlaceholderStyle::Named);
    }

    #[test]
    fn named_rewrite_skips_dollar_quoted_bodies_strings_and_comments() {
        let query = "SELECT $$ $p0 $$, '$p0', \"$p0\", /* $p0 */ $p0, my$p0 -- $p0\n";
        let prepared = prepare_postgres_statement(query, &params(&[("p0", json!("bound"))]))
            .expect("prepare should succeed");

        assert_eq!(
            prepared.statement,
            "SELECT $$ $p0 $$, '$p0', \"$p0\", /* $p0 */ $1, my$p0 -- $p0\n"
        );
        assert_eq!(prepared.parameter_values, vec![Some(b"bound".to_vec())]);
    }

    #[test]
    fn null_maps_and_lists_are_encoded_for_bind() {
        let query = "SELECT $a, $b, $c";
        let prepared = prepare_postgres_statement(
            query,
            &params(&[
                ("a", Value::Null),
                ("b", json!({"k": "v"})),
                ("c", json!([1, 2, 3])),
            ]),
        )
        .expect("prepare should succeed");

        assert_eq!(
            prepared.parameter_values,
            vec![
                None,
                Some(br#"{"k":"v"}"#.to_vec()),
                Some(b"[1,2,3]".to_vec()),
            ]
        );
    }

    #[test]
    fn empty_parameter_map_leaves_statement_untouched() {
        let query = "SELECT * FROM cypher('g', $$ RETURN 1 $$) AS (v agtype)";
        let prepared =
            prepare_postgres_statement(query, &HashMap::new()).expect("prepare should succeed");
        assert_eq!(prepared.statement, query);
        assert!(!prepared.is_parameterized());
        assert_eq!(prepared.placeholder_style, ParameterPlaceholderStyle::None);
    }
}
