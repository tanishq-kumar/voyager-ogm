//! High-throughput bulk ingestion query template compiler.
//!
//! Generates parameterized batch queries leveraging `UNWIND $batch AS row`
//! for million-row bulk creations and upserts across graph dialects.

use crate::ast::{AstNode, Direction, ExecutionMode, LabelExpression, QueryAstArena};
use crate::emitters::{AgeEmitter, CypherEmitter, DEFAULT_AGE_GRAPH, IsoGqlEmitter};
use crate::error::Result;
use crate::visitor::{AstVisitor, CompiledQuery};

fn emit_bulk_query(
    dialect: &str,
    graph_name: Option<&str>,
    arena: &QueryAstArena,
    root_handle: crate::ast::NodeHandle,
) -> Result<CompiledQuery> {
    match dialect.to_lowercase().as_str() {
        "iso_gql" | "gql" => {
            let mut emitter = IsoGqlEmitter::new();
            emitter.visit_query(arena, root_handle)
        }
        "age" | "apache_age" | "postgres_age" => {
            let name = graph_name.unwrap_or(DEFAULT_AGE_GRAPH);
            let mut emitter = AgeEmitter::new(name);
            emitter.visit_query(arena, root_handle)
        }
        _ => {
            let mut emitter = CypherEmitter::new();
            emitter.visit_query(arena, root_handle)
        }
    }
}

/// Compiles a high-throughput `UNWIND $batch AS row CREATE (n:Label) SET n.prop = row.prop` query.
///
/// # Examples
/// ```rust
/// use voyager_ogm::bulk::compile_bulk_create;
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
    compile_bulk_create_with_graph(label, properties, batch_param, row_alias, dialect, None)
}

/// Compiles a high-throughput `UNWIND $batch AS row CREATE (n:Label) SET n.prop = row.prop` query with optional graph name.
pub fn compile_bulk_create_with_graph(
    label: &str,
    properties: &[&str],
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
    graph_name: Option<&str>,
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
        label_expression: Some(LabelExpression::Label(label.to_string())),
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
        execution_mode: ExecutionMode::Normal,
        load_csv: None,
        unwinds: vec![unwind_handle],
        matches: Vec::new(),
        linear_clauses: Vec::new(),
        with_clauses: Vec::new(),
        mutations,
        return_clause: None,
    });

    emit_bulk_query(dialect, graph_name, &arena, root_handle)
}

/// Compiles a high-throughput `UNWIND $batch AS row MERGE (n:Label {id: row.id}) ON CREATE/MATCH SET` upsert query.
///
/// # Examples
/// ```rust
/// use voyager_ogm::bulk::compile_bulk_merge;
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
    compile_bulk_merge_split(&BulkMergeConfig {
        label,
        key_property,
        properties,
        create_only_properties: &[],
        batch_param,
        row_alias,
        dialect,
        graph_name: None,
    })
}

/// Compiles a high-throughput `UNWIND $batch AS row MERGE (n:Label {id: row.id}) ON CREATE/MATCH SET` upsert query with optional graph name.
pub fn compile_bulk_merge_with_graph(
    label: &str,
    key_property: &str,
    properties: &[&str],
    batch_param: &str,
    row_alias: &str,
    dialect: &str,
    graph_name: Option<&str>,
) -> Result<CompiledQuery> {
    compile_bulk_merge_split(&BulkMergeConfig {
        label,
        key_property,
        properties,
        create_only_properties: &[],
        batch_param,
        row_alias,
        dialect,
        graph_name,
    })
}

/// Configuration options for bulk MERGE query compilation.
#[derive(Debug, Clone)]
pub struct BulkMergeConfig<'a> {
    /// Node label to merge.
    pub label: &'a str,
    /// Property used to match an existing node.
    pub key_property: &'a str,
    /// Property names assigned on create and match.
    pub properties: &'a [&'a str],
    /// Property names assigned only on create.
    pub create_only_properties: &'a [&'a str],
    /// Batch parameter identifier name.
    pub batch_param: &'a str,
    /// Row alias identifier in the UNWIND clause.
    pub row_alias: &'a str,
    /// Target query dialect.
    pub dialect: &'a str,
    /// Optional graph name for Apache AGE or SQL:2023 PGQ.
    pub graph_name: Option<&'a str>,
}

/// Compiles a bulk upsert where some properties are assigned only on creation.
///
/// Properties listed in `create_only_properties` appear in `ON CREATE SET` but
/// never in `ON MATCH SET`, so an existing node keeps its stored values. They
/// are also excluded from the MERGE key by the caller.
pub fn compile_bulk_merge_split(config: &BulkMergeConfig<'_>) -> Result<CompiledQuery> {
    let label = config.label;
    let key_property = config.key_property;
    let properties = config.properties;
    let create_only_properties = config.create_only_properties;
    let batch_param = config.batch_param;
    let row_alias = config.row_alias;
    let dialect = config.dialect;
    let graph_name = config.graph_name;

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
        label_expression: Some(LabelExpression::Label(label.to_string())),
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

    for &prop in create_only_properties {
        let target_prop = arena.alloc(AstNode::PropertyAccess {
            target: node_var_ident,
            property: prop.to_string(),
        });
        let row_prop = arena.alloc(AstNode::PropertyAccess {
            target: row_alias_ident,
            property: prop.to_string(),
        });
        on_create_set.push(arena.alloc(AstNode::SetItem {
            target: target_prop,
            value: row_prop,
            is_merge: false,
        }));
    }

    let merge_handle = arena.alloc(AstNode::MergeClause {
        path: node_pattern_handle,
        on_create_set,
        on_match_set,
    });

    let root_handle = arena.alloc(AstNode::QueryStatement {
        execution_mode: ExecutionMode::Normal,
        load_csv: None,
        unwinds: vec![unwind_handle],
        matches: Vec::new(),
        linear_clauses: Vec::new(),
        with_clauses: Vec::new(),
        mutations: vec![merge_handle],
        return_clause: None,
    });

    emit_bulk_query(dialect, graph_name, &arena, root_handle)
}

/// Configuration options for bulk relationship query compilation.
#[derive(Debug, Clone)]
pub struct BulkRelConfig<'a> {
    /// Relationship type label (e.g. "KNOWS").
    pub rel_type: &'a str,
    /// Property names to extract from the batch records.
    pub properties: &'a [&'a str],
    /// Source node label.
    pub from_label: &'a str,
    /// Source node primary/matching key property name.
    pub from_key: &'a str,
    /// Target node label.
    pub to_label: &'a str,
    /// Target node primary/matching key property name.
    pub to_key: &'a str,
    /// Batch parameter identifier name.
    pub batch_param: &'a str,
    /// Row alias identifier in the UNWIND clause.
    pub row_alias: &'a str,
    /// Target query dialect.
    pub dialect: &'a str,
    /// Optional graph name for Apache AGE or SQL:2023 PGQ.
    pub graph_name: Option<&'a str>,
}

/// Compiles a high-throughput relationship bulk ingestion query with config.
pub fn compile_bulk_create_rel_with_config(config: &BulkRelConfig<'_>) -> Result<CompiledQuery> {
    let mut arena = QueryAstArena::new();
    let from_var = format!("_from_{}_0", config.from_label.to_lowercase());
    let to_var = format!("_to_{}_0", config.to_label.to_lowercase());
    let rel_var = format!("_{}_0", config.rel_type.to_lowercase());

    // 1. UNWIND $batch AS row
    let batch_param_handle = arena.alloc(AstNode::Parameter(config.batch_param.to_string()));
    let unwind_handle = arena.alloc(AstNode::UnwindClause {
        expression: batch_param_handle,
        alias: config.row_alias.to_string(),
    });

    // 2. MATCH from_node, to_node
    let from_var_ident = arena.alloc(AstNode::Identifier(from_var.clone()));
    let to_var_ident = arena.alloc(AstNode::Identifier(to_var.clone()));
    let row_alias_ident = arena.alloc(AstNode::Identifier(config.row_alias.to_string()));

    let from_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: from_var_ident,
        property: config.from_key.to_string(),
    });
    let row_from_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: row_alias_ident,
        property: format!("from_{}", config.from_key),
    });
    let from_pred = arena.alloc(AstNode::BinaryExpression {
        left: from_key_prop,
        op: crate::ast::BinaryOp::Eq,
        right: row_from_key_prop,
    });
    let from_node_handle = arena.alloc(AstNode::NodePattern {
        variable: Some(from_var.clone()),
        label_expression: Some(LabelExpression::Label(config.from_label.to_string())),
        predicates: vec![from_pred],
    });

    let to_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: to_var_ident,
        property: config.to_key.to_string(),
    });
    let row_to_key_prop = arena.alloc(AstNode::PropertyAccess {
        target: row_alias_ident,
        property: format!("to_{}", config.to_key),
    });
    let to_pred = arena.alloc(AstNode::BinaryExpression {
        left: to_key_prop,
        op: crate::ast::BinaryOp::Eq,
        right: row_to_key_prop,
    });
    let to_node_handle = arena.alloc(AstNode::NodePattern {
        variable: Some(to_var.clone()),
        label_expression: Some(LabelExpression::Label(config.to_label.to_string())),
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
        label_expression: None,
        predicates: Vec::new(),
    });
    let edge_pattern = arena.alloc(AstNode::EdgePattern {
        variable: Some(rel_var.clone()),
        label_expression: Some(LabelExpression::Label(config.rel_type.to_string())),
        direction: Direction::Outgoing,
        min_hops: None,
        max_hops: None,
        predicates: Vec::new(),
        target_node: dest_node_ref,
    });
    let src_node_ref = arena.alloc(AstNode::NodePattern {
        variable: Some(from_var),
        label_expression: None,
        predicates: Vec::new(),
    });
    let path_chain = arena.alloc(AstNode::PathChain {
        path_variable: None,
        path_mode: crate::ast::PathMode::None,
        start_node: src_node_ref,
        edges: vec![edge_pattern],
    });
    let create_handle = arena.alloc(AstNode::CreateClause {
        paths: vec![path_chain],
    });

    // 4. SET rel properties
    let mut set_items = Vec::new();
    let rel_var_ident = arena.alloc(AstNode::Identifier(rel_var));
    for &prop in config.properties {
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
        execution_mode: ExecutionMode::Normal,
        load_csv: None,
        unwinds: vec![unwind_handle],
        matches: vec![match_handle],
        linear_clauses: Vec::new(),
        with_clauses: Vec::new(),
        mutations,
        return_clause: None,
    });

    emit_bulk_query(config.dialect, config.graph_name, &arena, root_handle)
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
    compile_bulk_create_rel_with_config(&BulkRelConfig {
        rel_type,
        properties,
        from_label,
        from_key,
        to_label,
        to_key,
        batch_param,
        row_alias,
        dialect,
        graph_name: None,
    })
}
