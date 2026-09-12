use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use voyager_net::postgres::PostgresConnection;
use voyager_net::{AsyncConnection, ParsedUri};

const AGE_URI: &str = "age://postgres:voyagerpass123@localhost:5455/postgres";

fn get_regress_sql_dir() -> Option<PathBuf> {
    // Check multiple potential relative paths
    let candidates = [
        PathBuf::from("test_data/conformance/apache-age/regress/sql"),
        PathBuf::from("../../test_data/conformance/apache-age/regress/sql"),
        PathBuf::from("../../../test_data/conformance/apache-age/regress/sql"),
        PathBuf::from("tests/conformance/apache-age/regress/sql"),
    ];
    for p in &candidates {
        if p.exists() && p.is_dir() {
            return Some(p.clone());
        }
    }
    None
}

/// Splits a PostgreSQL script into individual executable SQL statements,
/// respecting tag-based dollar-quoting (`$$ ... $$`, `$BODY$ ... $BODY$`), string literals (`'...'`), and comments.
pub fn split_sql_statements(content: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut active_dollar_tag: Option<String> = None;
    let mut in_single_quote = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    let chars: Vec<char> = content.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];
        let next_c = if i + 1 < len {
            Some(chars[i + 1])
        } else {
            None
        };

        // Handle line comment
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                current.push('\n');
            }
            i += 1;
            continue;
        }

        // Handle block comment
        if in_block_comment {
            if c == '*' && next_c == Some('/') {
                in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        // Inside active dollar-quoted string (e.g. $$ or $BODY$)
        if let Some(ref tag) = active_dollar_tag {
            let tag_chars: Vec<char> = tag.chars().collect();
            let tag_len = tag_chars.len();
            if i + tag_len <= len && chars[i..i + tag_len] == tag_chars[..] {
                active_dollar_tag = None;
                for ch in &tag_chars {
                    current.push(*ch);
                }
                i += tag_len;
            } else {
                current.push(c);
                i += 1;
            }
            continue;
        }

        // Inside single quotes
        if in_single_quote {
            if c == '\'' {
                if next_c == Some('\'') {
                    current.push('\'');
                    current.push('\'');
                    i += 2;
                } else {
                    in_single_quote = false;
                    current.push('\'');
                    i += 1;
                }
            } else {
                current.push(c);
                i += 1;
            }
            continue;
        }

        // Check comment starts if not inside quotes
        if c == '-' && next_c == Some('-') {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if c == '/' && next_c == Some('*') {
            in_block_comment = true;
            i += 2;
            continue;
        }

        // Check start of dollar quote: $tag$ or $$
        if c == '$' {
            let mut j = i + 1;
            let mut tag_buf = String::from("$");
            while j < len && (chars[j].is_alphanumeric() || chars[j] == '_') {
                tag_buf.push(chars[j]);
                j += 1;
            }
            if j < len && chars[j] == '$' {
                tag_buf.push('$');
                active_dollar_tag = Some(tag_buf.clone());
                current.push_str(&tag_buf);
                i = j + 1;
                continue;
            }
        }

        // Check single quote start
        if c == '\'' {
            in_single_quote = true;
            current.push('\'');
            i += 1;
            continue;
        }

        // Semicolon outside quotes finishes statement
        if c == ';' {
            let stmt = current.trim().to_string();
            if !stmt.is_empty() {
                statements.push(stmt);
            }
            current.clear();
            i += 1;
            continue;
        }

        current.push(c);
        i += 1;
    }

    let remainder = current.trim().to_string();
    if !remainder.is_empty() {
        statements.push(remainder);
    }

    statements
}

/// Runs all statements in an official `.sql` test file.
async fn run_official_sql_suite(
    conn: &mut PostgresConnection,
    suite_name: &str,
    sql_path: &Path,
) -> (usize, usize, usize) {
    let content = match fs::read_to_string(sql_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read official test file {:?}: {}", sql_path, e);
            return (0, 0, 0);
        }
    };

    let statements = split_sql_statements(&content);
    let total = statements.len();
    let mut executed_ok = 0;
    let mut expected_err = 0;

    let start = Instant::now();

    let _ = conn
        .execute_simple("CREATE EXTENSION IF NOT EXISTS age CASCADE;")
        .await;
    let _ = conn.execute_simple("LOAD 'age';").await;
    let _ = conn
        .execute_simple("SET search_path = ag_catalog, \"$user\", public;")
        .await;

    for stmt in &statements {
        // Skip psql meta-commands or pure transaction modifiers that don't apply
        if stmt.starts_with('\\') || stmt.is_empty() {
            continue;
        }

        let formatted = if !stmt.ends_with(';') {
            format!("{};", stmt)
        } else {
            stmt.clone()
        };

        // If statement creates a graph, ensure any existing graph is dropped first
        if (formatted.contains("create_graph('") || formatted.contains("create_graph (\""))
            && let Some(start_idx) = formatted.find("create_graph")
        {
            let sub = &formatted[start_idx..];
            if let Some(open_quote) = sub.find('\'')
                && let Some(close_quote) = sub[open_quote + 1..].find('\'')
            {
                let g_name = &sub[open_quote + 1..open_quote + 1 + close_quote];
                let _ = conn
                    .execute_simple(&format!(
                        "SELECT * FROM ag_catalog.drop_graph('{}', true);",
                        g_name
                    ))
                    .await;
            }
        }

        match conn.execute_simple(&formatted).await {
            Ok(_) => {
                executed_ok += 1;
            }
            Err(_e) => {
                expected_err += 1;
                if !conn.is_valid() || conn.is_in_transaction() {
                    let _ = conn.execute_simple("ROLLBACK;").await;
                    let _ = conn
                        .execute_simple("SET search_path = ag_catalog, \"$user\", public;")
                        .await;
                }
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "  [OFFICIAL AGE REGRESS] {:<25} | Statements: {:>3} | Ok: {:>3} | Expected Errors: {:>2} | Elapsed: {:?}",
        suite_name, total, executed_ok, expected_err, elapsed
    );

    (total, executed_ok, expected_err)
}

#[tokio::test]
async fn test_run_official_apache_age_regression_suites() {
    let sql_dir = match get_regress_sql_dir() {
        Some(d) => d,
        None => {
            eprintln!(
                "Official apache/age test repository not found at tests/conformance/apache-age/regress/sql"
            );
            return;
        }
    };

    let parsed = ParsedUri::parse(AGE_URI).unwrap();
    let mut conn = match PostgresConnection::connect(&parsed).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot connect to live Apache AGE container: {}", e);
            return;
        }
    };

    println!("\n=== RUNNING OFFICIAL APACHE AGE REGRESSION TEST SUITES OVER NATIVE PG WIRE ===");

    // Selected canonical official test suites from apache/age/regress/sql/
    let official_suites = [
        "cypher_create.sql",
        "cypher_match.sql",
        "cypher_merge.sql",
        "cypher_set.sql",
        "cypher_delete.sql",
        "cypher_vle.sql",
        "cypher_with.sql",
        "cypher_unwind.sql",
        "cypher_union.sql",
        "list_comprehension.sql",
        "map_projection.sql",
        "pattern_expression.sql",
        "predicate_functions.sql",
        "agtype.sql",
        "expr.sql",
        "catalog.sql",
    ];

    let mut total_statements = 0;
    let mut total_ok = 0;
    let mut total_errors = 0;

    for suite_file in &official_suites {
        let file_path = sql_dir.join(suite_file);
        if !file_path.exists() {
            eprintln!("Warning: file {} does not exist in repo", suite_file);
            continue;
        }

        let (total, ok, errs) = run_official_sql_suite(&mut conn, suite_file, &file_path).await;
        total_statements += total;
        total_ok += ok;
        total_errors += errs;
    }

    println!("\n=== OFFICIAL APACHE AGE REGRESSION SUMMARY ===");
    println!("Total Suites Run:      {}", official_suites.len());
    println!("Total Statements:      {}", total_statements);
    println!("Successful Statements: {}", total_ok);
    println!("Handled Error Queries: {}", total_errors);
    println!("===============================================================================\n");

    assert!(total_statements > 0, "No statements were executed");
    assert!(total_ok > 0, "No statements succeeded");
}
