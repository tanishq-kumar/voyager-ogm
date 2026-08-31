//! High-throughput bulk ingestion query template compiler.
//!
//! Generates parameterized batch queries leveraging `UNWIND $batch AS row`
//! for million-row bulk creations and upserts across graph dialects.

use crate::ast::{AstNode, Direction, QueryAstArena};
use crate::emitters::{CypherEmitter, IsoGqlEmitter};
use crate::error::Result;
use crate::visitor::{AstVisitor, CompiledQuery};

/// Compiles a high-throughput `UNWIND $batch AS row CREATE (n:Label) SET n.prop = row.prop` query.
///
/// # Examples
/// ```rust
/// use voyager_core::bulk::compile_bulk_create;
///
/// let query = compile_bulk_create("Person", &["name", "age"], "batch", "row", "cypher").unwrap();
/// assert_eq!(
///     query.statement,
///     "UNWIND $batch AS row CREATE (_person_0:Person) SET _person_0.name = row.name, _person_0.age = row.age"
/// );
/// ```
pub fn compile_bulk_create(
    label: &str,
    properties: &[&str],
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
) -> Result<CompiledQuery> {
    let mut arena = QueryAstArena::new();
    let node_var = format!("_{}_0", label.to_lowercase());

    // 1. UNWIND $batch AS row
    let batch_param_handle = arena.alloc(AstNode::Parameter(batch_param.to_string()));
    let unwind_handle = arena.alloc(AstNode::UnwindClause {
        expression: batch_param_handle,
        alias: row_alias.to_string(),
    });

    // 2. CREATE (node_var:Label)
    let node_pattern_handle = arena.alloc(AstNode::NodePattern {
        variable: Some(node_var.clone()),
        labels: vec![label.to_string()],
        predicates: Vec::new(),
    });
    let create_handle = arena.alloc(AstNode::CreateClause {
        paths: vec![node_pattern_handle],
    });

    // 3. SET node_var.prop = row.prop
    let mut set_items = Vec::with_capacity(properties.len());
    let node_var_ident = arena.alloc(AstNode::Identifier(node_var.clone()));
    let row_alias_ident = arena.alloc(AstNode::Identifier(row_alias.to_string()));

    for &prop in properties {
        let target_prop = arena.alloc(AstNode::PropertyAccess {
            target: node_var_ident,
            property: prop.to_string(),
        });
        let row_prop = arena.alloc(AstNode::PropertyAccess {
            target: row_alias_ident,
            property: prop.to_string(),
        });
        let set_item = arena.alloc(AstNode::SetItem {
            target: target_prop,
            value: row_prop,
            is_merge: false,
        });
        set_items.push(set_item);
    }

    let mut mutations = vec![create_handle];
    if !set_items.is_empty() {
        let set_clause = arena.alloc(AstNode::SetClause { items: set_items });
        mutations.push(set_clause);
    }

    let root_handle = arena.alloc(AstNode::QueryStatement {
        unwinds: vec![unwind_handle],
        matches: Vec::new(),
        mutations,
        return_clause: None,
    });

    match dialect.to_lowercase().as_str() {
        "iso_gql" | "gql" => {
            let mut emitter = IsoGqlEmitter::new();
            emitter.visit_query(&arena, root_handle)
        }
        _ => {
            let mut emitter = CypherEmitter::new();
            emitter.visit_query(&arena, root_handle)
        }
    }
}

/// Compiles a high-throughput `UNWIND $batch AS row MERGE (n:Label {id: row.id}) ON CREATE/MATCH SET` upsert query.
///
/// # Examples
/// ```rust
/// use voyager_core::bulk::compile_bulk_merge;
///
/// let query = compile_bulk_merge("Person", "id", &["name", "age"], "batch", "row", "cypher").unwrap();
/// assert!(query.statement.starts_with("UNWIND $batch AS row MERGE (_person_0:Person"));
/// ```
pub fn compile_bulk_merge(
    label: &str,
    key_property: &str,
    properties: &[&str],
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
) -> Result<CompiledQuery> {
    let mut arena = QueryAstArena::new();
    let node_var = format!("_{}_0", label.to_lowercase());

    // 1. UNWIND $batch AS row
    let batch_param_handle = arena.alloc(AstNode::Parameter(batch_param.to_string()));
    let unwind_handle = arena.alloc(AstNode::UnwindClause {
        expression: batch_param_handle,
        alias: row_alias.to_string(),
    });

    // 2. Node Pattern with key property predicate: `(node_var:Label {node_var.id = row.id})`
    let node_var_ident = arena.alloc(AstNode::Identifier(node_var.clone()));
    let row_alias_ident = arena.alloc(AstNode::Identifier(row_alias.to_string()));

    let node_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: node_var_ident,
        property: key_property.to_string(),
    });
    let row_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: row_alias_ident,
        property: key_property.to_string(),
    });
    let key_pred = arena.alloc(AstNode::BinaryExpression {
        left: node_key_prop,
        op: crate::ast::BinaryOp::Eq,
        right: row_key_prop,
    });

    let node_pattern_handle = arena.alloc(AstNode::NodePattern {
        variable: Some(node_var.clone()),
        labels: vec![label.to_string()],
        predicates: vec![key_pred],
    });

    // 3. ON CREATE / ON MATCH SET items
    let mut on_create_set = Vec::new();
    let mut on_match_set = Vec::new();

    for &prop in properties {
        let target_prop = arena.alloc(AstNode::PropertyAccess {
            target: node_var_ident,
            property: prop.to_string(),
        });
        let row_prop = arena.alloc(AstNode::PropertyAccess {
            target: row_alias_ident,
            property: prop.to_string(),
        });
        let item = arena.alloc(AstNode::SetItem {
            target: target_prop,
            value: row_prop,
            is_merge: false,
        });
        on_create_set.push(item);
        on_match_set.push(item);
    }

    let merge_handle = arena.alloc(AstNode::MergeClause {
        path: node_pattern_handle,
        on_create_set,
        on_match_set,
    });

    let root_handle = arena.alloc(AstNode::QueryStatement {
        unwinds: vec![unwind_handle],
        matches: Vec::new(),
        mutations: vec![merge_handle],
        return_clause: None,
    });

    match dialect.to_lowercase().as_str() {
        "iso_gql" | "gql" => {
            let mut emitter = IsoGqlEmitter::new();
            emitter.visit_query(&arena, root_handle)
        }
        _ => {
            let mut emitter = CypherEmitter::new();
            emitter.visit_query(&arena, root_handle)
        }
    }
}

/// Compiles a high-throughput relationship bulk ingestion query.
#[allow(clippy::too_many_arguments)]
pub fn compile_bulk_create_rel(
    rel_type: &str,
    properties: &[&str],
    from_label: &str,
    from_key: &str,
    to_label: &str,
    to_key: &str,
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
) -> Result<CompiledQuery> {
    let mut arena = QueryAstArena::new();
    let from_var = format!("_from_{}_0", from_label.to_lowercase());
    let to_var = format!("_to_{}_0", to_label.to_lowercase());
    let rel_var = format!("_{}_0", rel_type.to_lowercase());

    // 1. UNWIND $batch AS row
    let batch_param_handle = arena.alloc(AstNode::Parameter(batch_param.to_string()));
    let unwind_handle = arena.alloc(AstNode::UnwindClause {
        expression: batch_param_handle,
        alias: row_alias.to_string(),
    });

    // 2. MATCH from_node, to_node
    let from_var_ident = arena.alloc(AstNode::Identifier(from_var.clone()));
    let to_var_ident = arena.alloc(AstNode::Identifier(to_var.clone()));
    let row_alias_ident = arena.alloc(AstNode::Identifier(row_alias.to_string()));

    let from_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: from_var_ident,
        property: from_key.to_string(),
    });
    let row_from_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: row_alias_ident,
        property: format!("from_{from_key}"),
    });
    let from_pred = arena.alloc(AstNode::BinaryExpression {
        left: from_key_prop,
        op: crate::ast::BinaryOp::Eq,
        right: row_from_key_prop,
    });
    let from_node_handle = arena.alloc(AstNode::NodePattern {
        variable: Some(from_var.clone()),
        labels: vec![from_label.to_string()],
        predicates: vec![from_pred],
    });

    let to_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: to_var_ident,
        property: to_key.to_string(),
    });
    let row_to_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: row_alias_ident,
        property: format!("to_{to_key}"),
    });
    let to_pred = arena.alloc(AstNode::BinaryExpression {
        left: to_key_prop,
        op: crate::ast::BinaryOp::Eq,
        right: row_to_key_prop,
    });
    let to_node_handle = arena.alloc(AstNode::NodePattern {
        variable: Some(to_var.clone()),
        labels: vec![to_label.to_string()],
        predicates: vec![to_pred],
    });

    let match_handle = arena.alloc(AstNode::MatchClause {
        optional: false,
        paths: vec![from_node_handle, to_node_handle],
        where_clause: None,
    });

    // 3. CREATE (from)-[rel:TYPE]->(to)
    let dest_node_ref = arena.alloc(AstNode::NodePattern {
        variable: Some(to_var),
        labels: Vec::new(),
        predicates: Vec::new(),
    });
    let edge_pattern = arena.alloc(AstNode::EdgePattern {
        variable: Some(rel_var.clone()),
        edge_types: vec![rel_type.to_string()],
        direction: Direction::Outgoing,
        min_hops: None,
        max_hops: None,
        predicates: Vec::new(),
        target_node: dest_node_ref,
    });
    let src_node_ref = arena.alloc(AstNode::NodePattern {
        variable: Some(from_var),
        labels: Vec::new(),
        predicates: Vec::new(),
    });
    let path_chain = arena.alloc(AstNode::PathChain {
        start_node: src_node_ref,
        edges: vec![edge_pattern],
    });
    let create_handle = arena.alloc(AstNode::CreateClause {
        paths: vec![path_chain],
    });

    // 4. SET rel properties
    let mut set_items = Vec::new();
    let rel_var_ident = arena.alloc(AstNode::Identifier(rel_var));
    for &prop in properties {
        let target_prop = arena.alloc(AstNode::PropertyAccess {
            target: rel_var_ident,
            property: prop.to_string(),
        });
        let row_prop = arena.alloc(AstNode::PropertyAccess {
            target: row_alias_ident,
            property: prop.to_string(),
        });
        let item = arena.alloc(AstNode::SetItem {
            target: target_prop,
            value: row_prop,
            is_merge: false,
        });
        set_items.push(item);
    }

    let mut mutations = vec![create_handle];
    if !set_items.is_empty() {
        let set_clause = arena.alloc(AstNode::SetClause { items: set_items });
        mutations.push(set_clause);
    }

    let root_handle = arena.alloc(AstNode::QueryStatement {
        unwinds: vec![unwind_handle],
        matches: vec![match_handle],
        mutations,
        return_clause: None,
    });

    match dialect.to_lowercase().as_str() {
        "iso_gql" | "gql" => {
            let mut emitter = IsoGqlEmitter::new();
            emitter.visit_query(&arena, root_handle)
        }
        _ => {
            let mut emitter = CypherEmitter::new();
            emitter.visit_query(&arena, root_handle)
        }
    }
}
