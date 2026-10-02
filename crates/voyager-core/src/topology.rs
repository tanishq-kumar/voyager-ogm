//! Graph entity and path topology reconstruction engine.
//!
//! Extracts structured node and edge topologies directly from the AST arena
//! or parses raw query statements across openCypher, ISO GQL, and SQL:2023 PGQ.
//!
//! # Architectural Note & Roadmap
//!
//! `from_query_str` is an interim heuristic parser (keyword scanning, depth tracking,
//! pattern matching). Its purpose is to provide immediate topology extraction for
//! visualization, tooling, and adapter harnesses without requiring full grammar parsers.
//!
//! **TODO**: Once the unified AST parser (openCypher/ISO GQL/PGQ) is fully operational,
//! all raw query strings will be parsed into `QueryAstArena` and feed directly into
//! `from_arena`. The topology extraction engine should have exactly one canonical
//! implementation long-term (`from_arena`).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ast::{AstNode, BinaryOp, Direction, LiteralValue, NodeHandle, QueryAstArena};
use crate::error::Result;

/// A graph node extracted from query path patterns.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TopologyNode {
    /// Unique identifier for the node (variable name or generated ID).
    pub id: String,
    /// Primary display label (first label, variable, or node ID).
    pub label: String,
    /// All labels attached to the node.
    pub labels: Vec<String>,
    /// Visual grouping category (primary label or "Entity").
    pub group: String,
    /// Visualization size token.
    pub size: u32,
    /// Inline property predicates or attributes.
    pub properties: BTreeMap<String, LiteralValue>,
    /// Additional metadata payload.
    pub data: BTreeMap<String, LiteralValue>,
}

impl TopologyNode {
    /// Creates a new topology node.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        let id_str = id.into();
        let label_str = label.into();
        let group = if label_str.is_empty() {
            "Entity".to_string()
        } else {
            label_str.clone()
        };
        let labels = if label_str.is_empty() {
            Vec::new()
        } else {
            vec![label_str.clone()]
        };
        Self {
            id: id_str,
            label: label_str,
            labels,
            group,
            size: 11,
            properties: BTreeMap::new(),
            data: BTreeMap::new(),
        }
    }
}

/// A graph edge/relationship extracted from query path patterns.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TopologyEdge {
    /// Unique identifier for the edge (e.g. `rel_0` or variable name).
    pub id: String,
    /// Source node identifier.
    pub source: String,
    /// Target node identifier.
    pub target: String,
    /// Primary relationship type label (e.g. `TRANSFERRED` or `CONNECTED_TO`).
    pub label: String,
    /// All relationship types for disjunctive edges (`[:KNOWS|FOLLOWS]`).
    pub types: Vec<String>,
    /// Traversal direction (`Outgoing`, `Incoming`, `Undirected`).
    pub direction: Direction,
    /// Hex color code for rendering.
    pub color: String,
    /// Minimum hops for variable-length traversal (e.g. `*1..3`).
    pub min_hops: Option<u32>,
    /// Maximum hops for variable-length traversal.
    pub max_hops: Option<u32>,
    /// Inline property predicates or attributes.
    pub properties: BTreeMap<String, LiteralValue>,
    /// Additional metadata payload.
    pub data: BTreeMap<String, LiteralValue>,
}

impl TopologyEdge {
    /// Creates a new topology edge.
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        target: impl Into<String>,
        label: impl Into<String>,
        direction: Direction,
    ) -> Self {
        let label_str = label.into();
        let types = if label_str.is_empty() {
            Vec::new()
        } else {
            vec![label_str.clone()]
        };
        Self {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            label: if label_str.is_empty() {
                "CONNECTED_TO".to_string()
            } else {
                label_str
            },
            types,
            direction,
            color: "#64748b".to_string(),
            min_hops: None,
            max_hops: None,
            properties: BTreeMap::new(),
            data: BTreeMap::new(),
        }
    }
}

fn default_topology_version() -> u32 {
    1
}

/// Extracted graph topology containing deduplicated nodes and connected edges.
///
/// # Scoping Behavior & Invariant Semantics
///
/// In graph visualization and structural invariant validation, variables with identical
/// names across distinct subquery scopes (e.g. `EXISTS { MATCH (p) }` or `WITH p`)
/// are coalesced into the same topological entity representation.
///
/// While lexical scopes are isolated during database execution and runtime binding,
/// visual graph topology reconstruction intentionally merges pattern variables by identifier
/// to depict the unified graph data model spanned by the query.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GraphTopology {
    /// Topology schema payload version for forward/backward compatibility.
    #[cfg_attr(feature = "serde", serde(default = "default_topology_version"))]
    pub version: u32,
    /// Nodes extracted from path patterns.
    pub nodes: Vec<TopologyNode>,
    /// Directed and undirected edges extracted from path patterns.
    pub edges: Vec<TopologyEdge>,
}

impl Default for GraphTopology {
    fn default() -> Self {
        Self {
            version: default_topology_version(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

impl GraphTopology {
    /// Creates an empty graph topology container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Extracts graph topology directly from a [`QueryAstArena`].
    ///
    /// Inspects match, create, and merge patterns within the arena with zero regex parsing.
    pub fn from_arena(arena: &QueryAstArena, root: Option<NodeHandle>) -> Self {
        let mut nodes_map: HashMap<String, TopologyNode> = HashMap::new();
        let mut nodes_order: Vec<String> = Vec::new();
        let mut edges: Vec<TopologyEdge> = Vec::new();
        let mut node_counter: usize = 0;
        let mut edge_counter: usize = 0;

        // Collect all pattern paths and where filters to inspect
        let mut path_handles: Vec<NodeHandle> = Vec::new();
        let mut where_handles: Vec<NodeHandle> = Vec::new();

        if let Some(r) = root {
            collect_paths_and_wheres_from_root(arena, r, &mut path_handles, &mut where_handles);
        } else {
            // No root specified: inspect all match, create, merge, and path chain clauses in arena
            for (idx, node) in arena.nodes().iter().enumerate() {
                let h = NodeHandle(idx as u32);
                match node {
                    AstNode::MatchClause {
                        paths,
                        where_clause,
                        ..
                    } => {
                        path_handles.extend_from_slice(paths);
                        if let Some(wh) = where_clause {
                            where_handles.push(*wh);
                        }
                    }
                    AstNode::CreateClause { paths } => {
                        path_handles.extend_from_slice(paths);
                    }
                    AstNode::MergeClause { path, .. } => {
                        path_handles.push(*path);
                    }
                    AstNode::PathChain { .. } => {
                        path_handles.push(h);
                    }
                    AstNode::WhereClause { .. } => {
                        where_handles.push(h);
                    }
                    _ => {}
                }
            }
        }

        // Deduplicate path handles while preserving order
        let mut seen_paths = HashSet::new();
        path_handles.retain(|h| seen_paths.insert(*h));

        for path_h in path_handles {
            if let Ok(ast_node) = arena.get(path_h) {
                match ast_node {
                    AstNode::NodePattern { .. } => {
                        extract_node_from_handle(
                            arena,
                            path_h,
                            &mut nodes_map,
                            &mut nodes_order,
                            &mut node_counter,
                        );
                    }
                    AstNode::PathChain {
                        start_node,
                        edges: edge_handles,
                        ..
                    } => {
                        let mut current_id = extract_node_from_handle(
                            arena,
                            *start_node,
                            &mut nodes_map,
                            &mut nodes_order,
                            &mut node_counter,
                        );

                        for &edge_h in edge_handles {
                            if let Ok(AstNode::EdgePattern {
                                variable,
                                edge_types,
                                direction,
                                min_hops,
                                max_hops,
                                predicates,
                                target_node,
                            }) = arena.get(edge_h)
                            {
                                let next_id = extract_node_from_handle(
                                    arena,
                                    *target_node,
                                    &mut nodes_map,
                                    &mut nodes_order,
                                    &mut node_counter,
                                );

                                edge_counter += 1;
                                let edge_id = variable
                                    .clone()
                                    .unwrap_or_else(|| format!("rel_{edge_counter}"));

                                let primary_label = edge_types
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| "CONNECTED_TO".to_string());

                                let (src, tgt) = match direction {
                                    Direction::Outgoing => (current_id.clone(), next_id.clone()),
                                    Direction::Incoming => (next_id.clone(), current_id.clone()),
                                    Direction::Undirected => (current_id.clone(), next_id.clone()),
                                };

                                let mut edge_props = BTreeMap::new();
                                extract_predicates_to_props(arena, predicates, &mut edge_props);

                                let mut edge_data = edge_props.clone();
                                if let Some(v) = variable {
                                    edge_data.insert(
                                        "variable".to_string(),
                                        LiteralValue::String(v.clone()),
                                    );
                                }

                                if let Some(existing) = edges.iter_mut().find(|e| e.id == edge_id) {
                                    for t in edge_types {
                                        if !existing.types.contains(t) {
                                            existing.types.push(t.clone());
                                        }
                                    }
                                    for (k, v) in edge_props {
                                        existing.properties.insert(k.clone(), v.clone());
                                        existing.data.insert(k, v);
                                    }
                                } else {
                                    edges.push(TopologyEdge {
                                        id: edge_id,
                                        source: src,
                                        target: tgt,
                                        label: primary_label,
                                        types: edge_types.clone(),
                                        direction: *direction,
                                        color: "#64748b".to_string(),
                                        min_hops: *min_hops,
                                        max_hops: *max_hops,
                                        properties: edge_props,
                                        data: edge_data,
                                    });
                                }

                                current_id = next_id;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        for wh in where_handles {
            extract_where_predicates_to_nodes(arena, wh, &mut nodes_map);
        }

        let nodes = nodes_order
            .into_iter()
            .filter_map(|id| nodes_map.remove(&id))
            .collect();

        Self {
            version: 1,
            nodes,
            edges,
        }
    }

    /// Parses raw query string statements (openCypher, ISO GQL, SQL:2023 PGQ)
    /// and extracts graph path topology without requiring external parser dependencies.
    pub fn from_query_str(query: &str) -> Self {
        let clean = strip_query_comments(query);
        let pattern_sections = extract_pattern_sections(&clean);

        let mut nodes_map: HashMap<String, TopologyNode> = HashMap::new();
        let mut nodes_order: Vec<String> = Vec::new();
        let mut edges: Vec<TopologyEdge> = Vec::new();
        let mut node_counter: usize = 0;
        let mut edge_counter: usize = 0;

        for section in pattern_sections {
            parse_path_sequence(
                &section,
                &mut nodes_map,
                &mut nodes_order,
                &mut edges,
                &mut node_counter,
                &mut edge_counter,
            );
        }

        extract_and_apply_where_predicates(&clean, &mut nodes_map);

        let nodes = nodes_order
            .into_iter()
            .filter_map(|id| nodes_map.remove(&id))
            .collect();

        Self {
            version: 1,
            nodes,
            edges,
        }
    }

    /// Serializes the graph topology into a JSON string.
    #[cfg(feature = "serde")]
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| crate::error::Error::EmissionError(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// AST Arena Walker Helpers
// ---------------------------------------------------------------------------

fn collect_paths_and_wheres_from_root(
    arena: &QueryAstArena,
    root: NodeHandle,
    paths: &mut Vec<NodeHandle>,
    wheres: &mut Vec<NodeHandle>,
) {
    if let Ok(ast_node) = arena.get(root) {
        match ast_node {
            AstNode::QueryStatement {
                matches,
                mutations,
                with_clauses,
                ..
            } => {
                for &m_h in matches {
                    collect_paths_and_wheres_from_root(arena, m_h, paths, wheres);
                }
                for &mut_h in mutations {
                    collect_paths_and_wheres_from_root(arena, mut_h, paths, wheres);
                }
                for &with_h in with_clauses {
                    if let Ok(AstNode::WithClause {
                        where_clause: Some(wh),
                        ..
                    }) = arena.get(with_h)
                    {
                        wheres.push(*wh);
                        descend_expression_for_subqueries(arena, *wh, paths, wheres);
                    }
                }
            }
            AstNode::MatchClause {
                paths: match_paths,
                where_clause,
                ..
            } => {
                paths.extend_from_slice(match_paths);
                if let Some(wh) = where_clause {
                    wheres.push(*wh);
                    descend_expression_for_subqueries(arena, *wh, paths, wheres);
                }
            }
            AstNode::CreateClause {
                paths: create_paths,
            } => {
                paths.extend_from_slice(create_paths);
            }
            AstNode::MergeClause { path, .. } => {
                paths.push(*path);
            }
            AstNode::PathChain { .. } | AstNode::NodePattern { .. } => {
                paths.push(root);
            }
            AstNode::WhereClause { root_predicate } => {
                wheres.push(root);
                descend_expression_for_subqueries(arena, *root_predicate, paths, wheres);
            }
            AstNode::ExistsSubquery { subquery } | AstNode::CountSubquery { subquery } => {
                collect_paths_and_wheres_from_root(arena, *subquery, paths, wheres);
            }
            _ => {}
        }
    }
}

fn descend_expression_for_subqueries(
    arena: &QueryAstArena,
    handle: NodeHandle,
    paths: &mut Vec<NodeHandle>,
    wheres: &mut Vec<NodeHandle>,
) {
    if let Ok(node) = arena.get(handle) {
        match node {
            AstNode::WhereClause { root_predicate } => {
                descend_expression_for_subqueries(arena, *root_predicate, paths, wheres);
            }
            AstNode::BinaryExpression { left, right, .. } => {
                descend_expression_for_subqueries(arena, *left, paths, wheres);
                descend_expression_for_subqueries(arena, *right, paths, wheres);
            }
            AstNode::UnaryExpression { operand, .. } => {
                descend_expression_for_subqueries(arena, *operand, paths, wheres);
            }
            AstNode::FunctionCall { arguments, .. } => {
                for &arg in arguments {
                    descend_expression_for_subqueries(arena, arg, paths, wheres);
                }
            }
            AstNode::ListLiteral(items) => {
                for &item in items {
                    descend_expression_for_subqueries(arena, item, paths, wheres);
                }
            }
            AstNode::CaseExpression {
                operand,
                when_then_branches,
                else_branch,
            } => {
                if let Some(op) = operand {
                    descend_expression_for_subqueries(arena, *op, paths, wheres);
                }
                for &(when_h, then_h) in when_then_branches {
                    descend_expression_for_subqueries(arena, when_h, paths, wheres);
                    descend_expression_for_subqueries(arena, then_h, paths, wheres);
                }
                if let Some(el) = else_branch {
                    descend_expression_for_subqueries(arena, *el, paths, wheres);
                }
            }
            AstNode::ListComprehension {
                list_expression,
                where_filter,
                map_expression,
                ..
            } => {
                descend_expression_for_subqueries(arena, *list_expression, paths, wheres);
                if let Some(wh) = where_filter {
                    descend_expression_for_subqueries(arena, *wh, paths, wheres);
                }
                if let Some(m) = map_expression {
                    descend_expression_for_subqueries(arena, *m, paths, wheres);
                }
            }
            AstNode::PatternComprehension {
                path,
                where_filter,
                projection,
            } => {
                collect_paths_and_wheres_from_root(arena, *path, paths, wheres);
                if let Some(wh) = where_filter {
                    descend_expression_for_subqueries(arena, *wh, paths, wheres);
                }
                descend_expression_for_subqueries(arena, *projection, paths, wheres);
            }
            AstNode::PropertyAccess { target, .. } => {
                descend_expression_for_subqueries(arena, *target, paths, wheres);
            }
            AstNode::SetItem { value, .. } => {
                descend_expression_for_subqueries(arena, *value, paths, wheres);
            }
            AstNode::UnwindClause { expression, .. } => {
                descend_expression_for_subqueries(arena, *expression, paths, wheres);
            }
            AstNode::ProcedureCall { arguments, .. } => {
                for &arg in arguments {
                    descend_expression_for_subqueries(arena, arg, paths, wheres);
                }
            }
            AstNode::ExistsSubquery { subquery } | AstNode::CountSubquery { subquery } => {
                collect_paths_and_wheres_from_root(arena, *subquery, paths, wheres);
            }
            // Explicitly terminal AST leaves without nested expression handles:
            // Literal, Identifier, Parameter, DeleteClause, RemoveClause, LoadCsvClause
            _ => {}
        }
    }
}

fn extract_where_predicates_to_nodes(
    arena: &QueryAstArena,
    handle: NodeHandle,
    nodes_map: &mut HashMap<String, TopologyNode>,
) {
    if let Ok(node) = arena.get(handle) {
        match node {
            AstNode::WhereClause { root_predicate } => {
                extract_where_predicates_to_nodes(arena, *root_predicate, nodes_map);
            }
            AstNode::BinaryExpression { left, op, right } => {
                if *op == BinaryOp::And {
                    extract_where_predicates_to_nodes(arena, *left, nodes_map);
                    extract_where_predicates_to_nodes(arena, *right, nodes_map);
                } else if *op == BinaryOp::Eq {
                    extract_eq_predicate(arena, *left, *right, nodes_map);
                }
            }
            _ => {}
        }
    }
}

fn extract_eq_predicate(
    arena: &QueryAstArena,
    left: NodeHandle,
    right: NodeHandle,
    nodes_map: &mut HashMap<String, TopologyNode>,
) {
    if let (Ok(left_node), Ok(right_node)) = (arena.get(left), arena.get(right)) {
        match (left_node, right_node) {
            (AstNode::PropertyAccess { target, property }, AstNode::Literal(lit)) => {
                if let Ok(AstNode::Identifier(var)) = arena.get(*target)
                    && let Some(topo_node) = nodes_map.get_mut(var)
                {
                    topo_node.properties.insert(property.clone(), lit.clone());
                    topo_node.data.insert(property.clone(), lit.clone());
                }
            }
            (AstNode::Literal(lit), AstNode::PropertyAccess { target, property }) => {
                if let Ok(AstNode::Identifier(var)) = arena.get(*target)
                    && let Some(topo_node) = nodes_map.get_mut(var)
                {
                    topo_node.properties.insert(property.clone(), lit.clone());
                    topo_node.data.insert(property.clone(), lit.clone());
                }
            }
            _ => {}
        }
    }
}

fn extract_node_from_handle(
    arena: &QueryAstArena,
    handle: NodeHandle,
    nodes_map: &mut HashMap<String, TopologyNode>,
    nodes_order: &mut Vec<String>,
    counter: &mut usize,
) -> String {
    if let Ok(AstNode::NodePattern {
        variable,
        labels,
        predicates,
    }) = arena.get(handle)
    {
        *counter += 1;
        let primary_label = labels.first().cloned();
        let node_id = match variable {
            Some(v) if !v.is_empty() => v.clone(),
            _ => match &primary_label {
                Some(lbl) => format!("{}_{}", lbl.to_lowercase(), *counter),
                None => format!("n{}", *counter),
            },
        };

        if let Some(node) = nodes_map.get_mut(&node_id) {
            for l in labels {
                if !node.labels.contains(l) {
                    node.labels.push(l.clone());
                }
            }
            let mut props = BTreeMap::new();
            extract_predicates_to_props(arena, predicates, &mut props);
            for (k, v) in props {
                node.properties.insert(k.clone(), v.clone());
                node.data.insert(k, v);
            }
        } else {
            let display_label = primary_label
                .clone()
                .or_else(|| variable.clone())
                .unwrap_or_else(|| node_id.clone());

            let group = primary_label.unwrap_or_else(|| "Entity".to_string());

            let mut props = BTreeMap::new();
            extract_predicates_to_props(arena, predicates, &mut props);

            let mut data = props.clone();
            if let Some(v) = variable {
                data.insert("variable".to_string(), LiteralValue::String(v.clone()));
            }

            nodes_map.insert(
                node_id.clone(),
                TopologyNode {
                    id: node_id.clone(),
                    label: display_label,
                    labels: labels.clone(),
                    group,
                    size: 11,
                    properties: props,
                    data,
                },
            );
            nodes_order.push(node_id.clone());
        }

        node_id
    } else {
        *counter += 1;
        let id = format!("n{}", *counter);
        if !nodes_map.contains_key(&id) {
            nodes_map.insert(id.clone(), TopologyNode::new(id.clone(), id.clone()));
            nodes_order.push(id.clone());
        }
        id
    }
}

fn extract_predicates_to_props(
    arena: &QueryAstArena,
    predicates: &[NodeHandle],
    props: &mut BTreeMap<String, LiteralValue>,
) {
    for &p_h in predicates {
        if let Ok(AstNode::BinaryExpression { left, op, right }) = arena.get(p_h)
            && *op == BinaryOp::Eq
        {
            if let (Ok(AstNode::PropertyAccess { property, .. }), Ok(AstNode::Literal(lit))) =
                (arena.get(*left), arena.get(*right))
            {
                props.insert(property.clone(), lit.clone());
            } else if let (
                Ok(AstNode::Literal(lit)),
                Ok(AstNode::PropertyAccess { property, .. }),
            ) = (arena.get(*left), arena.get(*right))
            {
                props.insert(property.clone(), lit.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw Query String Parser Helpers
// ---------------------------------------------------------------------------

/// Removes line comments (`//`, `--`) and block comments (`/* ... */`) from query text.
fn strip_query_comments(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;

    while i < len {
        let ch = chars[i];

        if ch == '`' && !in_single_quote && !in_double_quote {
            in_backtick = !in_backtick;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '\'' && !in_double_quote && !in_backtick {
            in_single_quote = !in_single_quote;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '"' && !in_single_quote && !in_backtick {
            in_double_quote = !in_double_quote;
            out.push(ch);
            i += 1;
            continue;
        }

        if !in_single_quote && !in_double_quote && !in_backtick {
            // Line comment // or --
            if (ch == '/' && i + 1 < len && chars[i + 1] == '/')
                || (ch == '-' && i + 1 < len && chars[i + 1] == '-')
            {
                i += 2;
                while i < len && chars[i] != '\n' && chars[i] != '\r' {
                    i += 1;
                }
                continue;
            }

            // Block comment /* ... */
            if ch == '/' && i + 1 < len && chars[i + 1] == '*' {
                i += 2;
                while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
        }

        out.push(ch);
        i += 1;
    }

    out
}

/// Parses simple literal values from query strings.
fn parse_simple_literal(s: &str) -> Option<LiteralValue> {
    let trimmed = s.trim();
    if (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
        || (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
    {
        return Some(LiteralValue::String(
            trimmed[1..trimmed.len() - 1].to_string(),
        ));
    }
    if let Ok(i) = trimmed.parse::<i64>() {
        return Some(LiteralValue::Int64(i));
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        return Some(LiteralValue::Float64(f));
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return Some(LiteralValue::Bool(true));
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Some(LiteralValue::Bool(false));
    }
    if trimmed.eq_ignore_ascii_case("null") {
        return Some(LiteralValue::Null);
    }
    None
}

/// Extracts WHERE predicates and attaches equality filters to node properties.
fn extract_and_apply_where_predicates(query: &str, nodes_map: &mut HashMap<String, TopologyNode>) {
    let upper = query.to_uppercase();
    let mut search_pos = 0;

    let stop_keywords = [
        "RETURN", "WITH", "ORDER BY", "SKIP", "LIMIT", "SET", "DELETE", "REMOVE", "MATCH",
        "CREATE", "MERGE",
    ];

    while let Some(where_pos) = upper[search_pos..].find("WHERE") {
        let abs_where = search_pos + where_pos;
        let prev_ok = abs_where == 0 || !query.as_bytes()[abs_where - 1].is_ascii_alphanumeric();
        let next_idx = abs_where + 5;
        let next_ok =
            next_idx >= query.len() || !query.as_bytes()[next_idx].is_ascii_alphanumeric();

        if !prev_ok || !next_ok {
            search_pos = next_idx;
            continue;
        }

        let content_start = next_idx;
        let mut stop_pos = query.len();

        for skw in &stop_keywords {
            if let Some(pos) = upper[content_start..].find(skw) {
                let abs_pos = content_start + pos;
                let prev_ok =
                    abs_pos == 0 || !query.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
                let next_idx = abs_pos + skw.len();
                let next_ok =
                    next_idx >= query.len() || !query.as_bytes()[next_idx].is_ascii_alphanumeric();

                if prev_ok && next_ok && abs_pos < stop_pos {
                    stop_pos = abs_pos;
                }
            }
        }

        let where_body = &query[content_start..stop_pos];
        parse_where_predicates_body(where_body, nodes_map);

        search_pos = stop_pos;
    }
}

fn split_eq_outside_quotes(s: &str) -> Option<(&str, &str)> {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if b == b'\'' && !in_double {
            in_single = !in_single;
        } else if b == b'"' && !in_single {
            in_double = !in_double;
        } else if !in_single && !in_double && b == b'=' {
            if i + 1 < len && bytes[i + 1] == b'=' {
                return Some((s[..i].trim(), s[i + 2..].trim()));
            } else {
                return Some((s[..i].trim(), s[i + 1..].trim()));
            }
        }
        i += 1;
    }
    None
}

fn split_and_outside_quotes(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut last = 0;
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if b == b'\'' && !in_double {
            in_single = !in_single;
        } else if b == b'"' && !in_single {
            in_double = !in_double;
        } else if !in_single && !in_double && i + 5 <= len {
            let chunk = &s[i..i + 5];
            if chunk.eq_ignore_ascii_case(" AND ") {
                out.push(&s[last..i]);
                last = i + 5;
                i += 4;
            }
        }
        i += 1;
    }
    out.push(&s[last..]);
    out
}

fn parse_where_predicates_body(body: &str, nodes_map: &mut HashMap<String, TopologyNode>) {
    for part in body.split(['\n', '\r']) {
        for expr in split_and_outside_quotes(part) {
            let expr = expr.trim();
            let (left, right) = match split_eq_outside_quotes(expr) {
                Some(pair) => pair,
                None => continue,
            };

            let (var, prop) = if let Some((v, p)) = left.split_once('.') {
                (v.trim(), p.trim())
            } else if let Some((v, p)) = right.split_once('.') {
                (v.trim(), p.trim())
            } else {
                continue;
            };

            let lit_str = if left.contains('.') { right } else { left };
            if let Some(lit) = parse_simple_literal(lit_str)
                && let Some(node) = nodes_map.get_mut(var)
            {
                node.properties.insert(prop.to_string(), lit.clone());
                node.data.insert(prop.to_string(), lit);
            }
        }
    }
}

/// Extracts pattern clauses (between MATCH / CREATE / MERGE / INSERT / UPSERT and subsequent query clauses).
fn extract_pattern_sections(query: &str) -> Vec<String> {
    let keywords = [
        "OPTIONAL MATCH",
        "MATCH",
        "INSERT",
        "UPSERT",
        "CREATE",
        "MERGE",
    ];

    let stop_keywords = [
        "WHERE",
        "RETURN",
        "WITH",
        "SET",
        "DELETE",
        "DETACH DELETE",
        "REMOVE",
        "ORDER BY",
        "SKIP",
        "LIMIT",
        "OFFSET",
        "COLUMNS",
        "YIELD",
    ];

    let mut sections = Vec::new();
    let upper = query.to_uppercase();
    let chars: Vec<char> = query.chars().collect();
    let len = chars.len();

    let mut search_pos = 0;
    while search_pos < len {
        // Find next starting keyword
        let mut earliest_match: Option<(usize, usize)> = None; // (start_idx, keyword_len)

        for kw in &keywords {
            if let Some(pos) = upper[search_pos..].find(kw) {
                let abs_pos = search_pos + pos;
                // Verify word boundary
                let prev_ok =
                    abs_pos == 0 || !query.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
                let next_idx = abs_pos + kw.len();
                let next_ok =
                    next_idx >= query.len() || !query.as_bytes()[next_idx].is_ascii_alphanumeric();

                if prev_ok && next_ok {
                    match earliest_match {
                        None => earliest_match = Some((abs_pos, kw.len())),
                        Some((best_pos, _)) if abs_pos < best_pos => {
                            earliest_match = Some((abs_pos, kw.len()));
                        }
                        _ => {}
                    }
                }
            }
        }

        let (kw_start, kw_len) = match earliest_match {
            Some(m) => m,
            None => break,
        };

        let content_start = kw_start + kw_len;
        let mut pos = content_start;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut stop_pos = len;

        while pos < len {
            let ch = chars[pos];
            if ch == '(' {
                paren_depth += 1;
            } else if ch == ')' {
                paren_depth = paren_depth.saturating_sub(1);
            } else if ch == '[' {
                bracket_depth += 1;
            } else if ch == ']' {
                bracket_depth = bracket_depth.saturating_sub(1);
            } else if paren_depth == 0 && bracket_depth == 0 {
                let remaining = &upper[pos..];
                let is_stop = stop_keywords.iter().any(|skw| {
                    if remaining.starts_with(skw) {
                        let prev_ok =
                            pos == 0 || !query.as_bytes()[pos - 1].is_ascii_alphanumeric();
                        let next_idx = pos + skw.len();
                        let next_ok =
                            next_idx >= len || !query.as_bytes()[next_idx].is_ascii_alphanumeric();
                        prev_ok && next_ok
                    } else {
                        false
                    }
                });
                let is_start = keywords.iter().any(|skw| {
                    if remaining.starts_with(skw) {
                        let prev_ok =
                            pos == 0 || !query.as_bytes()[pos - 1].is_ascii_alphanumeric();
                        let next_idx = pos + skw.len();
                        let next_ok =
                            next_idx >= len || !query.as_bytes()[next_idx].is_ascii_alphanumeric();
                        prev_ok && next_ok
                    } else {
                        false
                    }
                });

                if is_stop || is_start {
                    stop_pos = pos;
                    break;
                }
            }
            pos += 1;
        }

        let section = query[content_start..stop_pos].trim();
        if !section.is_empty() {
            sections.push(section.to_string());
        }

        search_pos = stop_pos;
    }

    if sections.is_empty() && query.contains('(') && query.contains(')') {
        // Fallback: entire query string may be an un-prefixed path pattern
        sections.push(query.to_string());
    }

    sections
}

/// Parses a path pattern string: `(node)-[edge]->(node), (isolated)`
fn parse_path_sequence(
    text: &str,
    nodes_map: &mut HashMap<String, TopologyNode>,
    nodes_order: &mut Vec<String>,
    edges: &mut Vec<TopologyEdge>,
    node_counter: &mut usize,
    edge_counter: &mut usize,
) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut pos = 0;
    let mut last_node_id: Option<String> = None;

    while pos < len {
        // Skip whitespace and path-separating commas
        while pos < len && (chars[pos].is_whitespace() || chars[pos] == ',') {
            if chars[pos] == ',' {
                last_node_id = None; // Comma resets path chain
            }
            pos += 1;
        }

        if pos >= len {
            break;
        }

        // 1. Try matching a Node pattern: `(`
        if chars[pos] == '(' {
            let start = pos;
            pos += 1;
            let mut depth = 1;
            while pos < len && depth > 0 {
                if chars[pos] == '(' {
                    depth += 1;
                } else if chars[pos] == ')' {
                    depth -= 1;
                }
                pos += 1;
            }
            let node_body: String = chars[start + 1..pos - 1].iter().collect();
            let parsed_node_id = parse_node_body(&node_body, nodes_map, nodes_order, node_counter);
            last_node_id = Some(parsed_node_id);
            continue;
        }

        // 2. Try matching an Edge pattern: `<-...-`, `-...->`, `-...-`, `<--`, `-->`, `--`
        if chars[pos] == '<' || chars[pos] == '-' {
            let left_arrow = chars[pos] == '<';
            if left_arrow {
                pos += 1;
            }

            if pos < len && chars[pos] == '-' {
                pos += 1;
            } else {
                pos += 1;
                continue;
            }

            // Optional bracketed edge spec: `[ ... ]`
            let mut edge_body = String::new();
            if pos < len && chars[pos] == '[' {
                let start = pos;
                pos += 1;
                let mut depth = 1;
                while pos < len && depth > 0 {
                    if chars[pos] == '[' {
                        depth += 1;
                    } else if chars[pos] == ']' {
                        depth -= 1;
                    }
                    pos += 1;
                }
                edge_body = chars[start + 1..pos - 1].iter().collect();
            }

            // Trailing quantifier before dash (e.g. `[r:TYPE]{1, 3}-`)
            let mut gql_quantifier: Option<(Option<u32>, Option<u32>)> = None;
            if pos < len && chars[pos] == '{' {
                let start = pos;
                pos += 1;
                let mut depth = 1;
                while pos < len && depth > 0 {
                    if chars[pos] == '{' {
                        depth += 1;
                    } else if chars[pos] == '}' {
                        depth -= 1;
                    }
                    pos += 1;
                }
                let inner: String = chars[start + 1..pos - 1].iter().collect();
                if !inner.contains(':')
                    && (inner.contains(',') || inner.trim().chars().all(|c| c.is_ascii_digit()))
                {
                    if let Some((low_s, high_s)) = inner.split_once(',') {
                        let low = low_s.trim().parse::<u32>().ok();
                        let high = high_s.trim().parse::<u32>().ok();
                        gql_quantifier = Some((low, high));
                    } else if let Ok(n) = inner.trim().parse::<u32>() {
                        gql_quantifier = Some((Some(n), Some(n)));
                    }
                } else {
                    pos = start;
                }
            }

            // Trailing dash
            if pos < len && chars[pos] == '-' {
                pos += 1;
            }

            // Optional right arrow
            let right_arrow = pos < len && chars[pos] == '>';
            if right_arrow {
                pos += 1;
            }

            // Trailing quantifier after arrow (e.g. `-[r:TYPE]->{1, 3}`)
            if gql_quantifier.is_none() && pos < len && chars[pos] == '{' {
                let start = pos;
                pos += 1;
                let mut depth = 1;
                while pos < len && depth > 0 {
                    if chars[pos] == '{' {
                        depth += 1;
                    } else if chars[pos] == '}' {
                        depth -= 1;
                    }
                    pos += 1;
                }
                let inner: String = chars[start + 1..pos - 1].iter().collect();
                if !inner.contains(':')
                    && (inner.contains(',') || inner.trim().chars().all(|c| c.is_ascii_digit()))
                {
                    if let Some((low_s, high_s)) = inner.split_once(',') {
                        let low = low_s.trim().parse::<u32>().ok();
                        let high = high_s.trim().parse::<u32>().ok();
                        gql_quantifier = Some((low, high));
                    } else if let Ok(n) = inner.trim().parse::<u32>() {
                        gql_quantifier = Some((Some(n), Some(n)));
                    }
                } else {
                    pos = start;
                }
            }

            let direction = if left_arrow {
                Direction::Incoming
            } else if right_arrow {
                Direction::Outgoing
            } else {
                Direction::Undirected
            };

            let (edge_var, edge_types, mut min_h, mut max_h, edge_props) =
                parse_edge_body(&edge_body);
            if let Some((q_min, q_max)) = gql_quantifier {
                min_h = q_min;
                max_h = q_max;
            }

            // Expect next node
            while pos < len && chars[pos].is_whitespace() {
                pos += 1;
            }

            if pos < len && chars[pos] == '(' {
                let start = pos;
                pos += 1;
                let mut depth = 1;
                while pos < len && depth > 0 {
                    if chars[pos] == '(' {
                        depth += 1;
                    } else if chars[pos] == ')' {
                        depth -= 1;
                    }
                    pos += 1;
                }
                let next_node_body: String = chars[start + 1..pos - 1].iter().collect();
                let next_node_id =
                    parse_node_body(&next_node_body, nodes_map, nodes_order, node_counter);

                *edge_counter += 1;
                let edge_id = edge_var
                    .clone()
                    .unwrap_or_else(|| format!("rel_{edge_counter}"));

                let primary_label = edge_types
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "CONNECTED_TO".to_string());

                let prev_id = last_node_id.unwrap_or_else(|| {
                    *node_counter += 1;
                    let synthetic_id = format!("n{}", *node_counter);
                    nodes_map.insert(
                        synthetic_id.clone(),
                        TopologyNode::new(synthetic_id.clone(), synthetic_id.clone()),
                    );
                    nodes_order.push(synthetic_id.clone());
                    synthetic_id
                });

                let (src, tgt) = match direction {
                    Direction::Outgoing => (prev_id.clone(), next_node_id.clone()),
                    Direction::Incoming => (next_node_id.clone(), prev_id.clone()),
                    Direction::Undirected => (prev_id.clone(), next_node_id.clone()),
                };

                let mut data = BTreeMap::new();
                if let Some(v) = edge_var {
                    data.insert("variable".to_string(), LiteralValue::String(v));
                }
                for (k, v) in &edge_props {
                    data.insert(k.clone(), v.clone());
                }

                if let Some(existing) = edges.iter_mut().find(|e| e.id == edge_id) {
                    for t in edge_types {
                        if !existing.types.contains(&t) {
                            existing.types.push(t);
                        }
                    }
                    for (k, v) in edge_props {
                        existing.properties.insert(k.clone(), v.clone());
                        existing.data.insert(k, v);
                    }
                } else {
                    edges.push(TopologyEdge {
                        id: edge_id,
                        source: src,
                        target: tgt,
                        label: primary_label,
                        types: edge_types,
                        direction,
                        color: "#64748b".to_string(),
                        min_hops: min_h,
                        max_hops: max_h,
                        properties: edge_props,
                        data,
                    });
                }

                last_node_id = Some(next_node_id);
                continue;
            }
        }

        pos += 1;
    }
}

/// Parses inline key-value property map `{key: val, key2: val2}` into a BTreeMap of LiteralValue.
fn parse_inline_properties(props_str: &str) -> BTreeMap<String, LiteralValue> {
    let mut props = BTreeMap::new();
    let trimmed = props_str.trim();
    if !trimmed.starts_with('{') {
        return props;
    }
    let end_idx = trimmed.rfind('}').unwrap_or(trimmed.len());
    let inner = &trimmed[1..end_idx];
    for pair in inner.split(',') {
        if let Some((k, v)) = pair.split_once(':') {
            let key = k
                .trim()
                .trim_matches('`')
                .trim_matches('\'')
                .trim_matches('"');
            if !key.is_empty()
                && let Some(lit) = parse_simple_literal(v)
            {
                props.insert(key.to_string(), lit);
            }
        }
    }
    props
}

/// Parses the inner string of a node pattern: `var:Label1:Label2 {prop: val}`
fn parse_node_body(
    body: &str,
    nodes_map: &mut HashMap<String, TopologyNode>,
    nodes_order: &mut Vec<String>,
    counter: &mut usize,
) -> String {
    let trimmed = body.trim();

    // Check for ISO GQL inline WHERE: `e:Employee WHERE e.salary >= 100000`
    let (node_spec, inline_where) = {
        let upper = trimmed.to_uppercase();
        if let Some(w_pos) = upper.find(" WHERE ") {
            (&trimmed[..w_pos], Some(&trimmed[w_pos + 7..]))
        } else {
            (trimmed, None)
        }
    };

    // Separate pattern from inline property map `{ ... }`
    let (ident_part, props_part) = if let Some(idx) = node_spec.find('{') {
        (&node_spec[..idx], Some(&node_spec[idx..]))
    } else {
        (node_spec, None)
    };
    let mut inline_props = props_part.map(parse_inline_properties).unwrap_or_default();

    if let Some(wh) = inline_where {
        for part in wh.split(['\n', '\r']) {
            for expr in part.split(" AND ").flat_map(|s| s.split(" and ")) {
                if let Some((l, r)) = expr.split_once('=') {
                    let prop = if let Some((_, p)) = l.trim().split_once('.') {
                        p.trim().trim_end_matches(['>', '<', '!', '=']).trim()
                    } else if let Some((_, p)) = r.trim().split_once('.') {
                        p.trim().trim_end_matches(['>', '<', '!', '=']).trim()
                    } else {
                        l.trim().trim_end_matches(['>', '<', '!', '=']).trim()
                    };
                    let lit_s = if l.contains('.') { r.trim() } else { l.trim() };
                    if !prop.is_empty()
                        && let Some(lit) = parse_simple_literal(lit_s)
                    {
                        inline_props.insert(prop.to_string(), lit);
                    }
                }
            }
        }
    }

    let mut variable = None;
    let mut labels = Vec::new();

    if !ident_part.is_empty() {
        let parts: Vec<&str> = ident_part.split(':').collect();
        let var_candidate = parts[0].trim();
        if !var_candidate.is_empty() {
            variable = Some(var_candidate.trim_matches('`').to_string());
        }

        for &lbl in &parts[1..] {
            // Also handle label disjunctions or conjunctions e.g. `Label1&Label2` or `Label1|Label2`
            for sub_lbl in lbl.split(&['&', '|'][..]) {
                let clean_lbl = sub_lbl.trim().trim_matches('`');
                if !clean_lbl.is_empty() && !clean_lbl.starts_with('!') {
                    labels.push(clean_lbl.to_string());
                }
            }
        }
    }

    *counter += 1;
    let primary_label = labels.first().cloned();
    let node_id = match &variable {
        Some(v) if !v.is_empty() => v.clone(),
        _ => match &primary_label {
            Some(lbl) => format!("{}_{}", lbl.to_lowercase(), *counter),
            None => format!("n{}", *counter),
        },
    };

    if let Some(node) = nodes_map.get_mut(&node_id) {
        for (k, v) in inline_props {
            node.properties.insert(k.clone(), v.clone());
            node.data.insert(k, v);
        }
        for l in labels {
            if !node.labels.contains(&l) {
                node.labels.push(l);
            }
        }
    } else {
        let display_label = primary_label
            .clone()
            .or_else(|| variable.clone())
            .unwrap_or_else(|| node_id.clone());

        let group = primary_label.unwrap_or_else(|| "Entity".to_string());

        let mut data = BTreeMap::new();
        if let Some(v) = &variable {
            data.insert("variable".to_string(), LiteralValue::String(v.clone()));
        }
        for (k, v) in &inline_props {
            data.insert(k.clone(), v.clone());
        }

        nodes_map.insert(
            node_id.clone(),
            TopologyNode {
                id: node_id.clone(),
                label: display_label,
                labels,
                group,
                size: 11,
                properties: inline_props,
                data,
            },
        );
        nodes_order.push(node_id.clone());
    }

    node_id
}

/// Parses the inner string of an edge pattern: `var:TYPE1|TYPE2*1..3 {prop: val}` or `[:T {1,3}]`
#[allow(clippy::type_complexity)]
fn parse_edge_body(
    body: &str,
) -> (
    Option<String>,
    Vec<String>,
    Option<u32>,
    Option<u32>,
    BTreeMap<String, LiteralValue>,
) {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return (None, Vec::new(), None, None, BTreeMap::new());
    }

    let mut inline_props = BTreeMap::new();
    let mut min_hops = None;
    let mut max_hops = None;

    let mut non_bracket_parts = String::new();
    let chars: Vec<char> = trimmed.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '{' {
            let start = i;
            let mut depth = 1;
            i += 1;
            while i < len && depth > 0 {
                if chars[i] == '{' {
                    depth += 1;
                } else if chars[i] == '}' {
                    depth -= 1;
                }
                i += 1;
            }
            let block: String = chars[start..i].iter().collect();
            let inner = if block.len() >= 2 {
                block[1..block.len() - 1].trim()
            } else {
                ""
            };

            // Check if this block is a quantifier {min, max} (e.g. {1,3}, {1,}, {,3}, {2})
            if !inner.contains(':')
                && (inner.contains(',') || inner.chars().all(|c| c.is_ascii_digit()))
                && !inner.is_empty()
            {
                if let Some((low_s, high_s)) = inner.split_once(',') {
                    min_hops = low_s.trim().parse::<u32>().ok();
                    max_hops = high_s.trim().parse::<u32>().ok();
                } else if let Ok(n) = inner.parse::<u32>() {
                    min_hops = Some(n);
                    max_hops = Some(n);
                }
            } else {
                let parsed_props = parse_inline_properties(&block);
                for (k, v) in parsed_props {
                    inline_props.insert(k, v);
                }
            }
        } else {
            non_bracket_parts.push(chars[i]);
            i += 1;
        }
    }

    let mut variable = None;
    let mut types = Vec::new();

    // Check for Cypher hop quantifiers: `*1..3`, `*..5`, `*2`, `*`
    let (ident_clean, hop_part) = if let Some(star_idx) = non_bracket_parts.find('*') {
        (
            &non_bracket_parts[..star_idx],
            Some(&non_bracket_parts[star_idx + 1..]),
        )
    } else {
        (non_bracket_parts.as_str(), None)
    };

    if min_hops.is_none()
        && max_hops.is_none()
        && let Some(hops) = hop_part
    {
        let h_trim = hops.trim();
        if h_trim.is_empty() {
            min_hops = Some(1);
        } else if let Some((low, high)) = h_trim.split_once("..") {
            min_hops = low.trim().parse::<u32>().ok();
            max_hops = high.trim().parse::<u32>().ok();
        } else if let Ok(n) = h_trim.parse::<u32>() {
            min_hops = Some(n);
            max_hops = Some(n);
        }
    }

    if !ident_clean.is_empty() {
        let parts: Vec<&str> = ident_clean.split(':').collect();
        let var_candidate = parts[0].trim();
        if !var_candidate.is_empty() {
            variable = Some(var_candidate.trim_matches('`').to_string());
        }

        for &t_spec in &parts[1..] {
            for sub_t in t_spec.split('|') {
                let clean_t = sub_t.trim().trim_matches('`');
                if !clean_t.is_empty() {
                    types.push(clean_t.to_string());
                }
            }
        }
    }

    (variable, types, min_hops, max_hops, inline_props)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::QueryBuilder;

    #[test]
    fn test_extract_topology_from_ast_arena() {
        let mut b = QueryBuilder::new();
        b.r#match()
            .node(Some("p"), vec!["Person"])
            .where_eq("p", "name", "Alice")
            .to(vec!["ACTED_IN"], Some("r"))
            .node(Some("m"), vec!["Movie"])
            .where_eq("m", "released", 1999)
            .from(vec!["DIRECTED"], Some("d"))
            .node(Some("dir"), vec!["Director"]);

        let (arena, root) = b.build();
        let topology = GraphTopology::from_arena(&arena, Some(root));

        assert_eq!(topology.nodes.len(), 3);
        assert_eq!(topology.nodes[0].id, "p");
        assert_eq!(topology.nodes[0].label, "Person");
        assert_eq!(
            topology.nodes[0].properties.get("name"),
            Some(&LiteralValue::String("Alice".to_string()))
        );

        assert_eq!(topology.nodes[1].id, "m");
        assert_eq!(topology.nodes[1].label, "Movie");
        assert_eq!(
            topology.nodes[1].properties.get("released"),
            Some(&LiteralValue::Int64(1999))
        );

        assert_eq!(topology.nodes[2].id, "dir");
        assert_eq!(topology.nodes[2].label, "Director");

        assert_eq!(topology.edges.len(), 2);
        assert_eq!(topology.edges[0].source, "p");
        assert_eq!(topology.edges[0].target, "m");
        assert_eq!(topology.edges[0].label, "ACTED_IN");
        assert_eq!(topology.edges[0].direction, Direction::Outgoing);

        assert_eq!(topology.edges[1].source, "dir");
        assert_eq!(topology.edges[1].target, "m");
        assert_eq!(topology.edges[1].label, "DIRECTED");
        assert_eq!(topology.edges[1].direction, Direction::Incoming);
    }

    #[test]
    fn test_extract_topology_from_raw_query_string() {
        let query = "MATCH (a:Account)-[r:TRANSFERRED]->(b:Account) RETURN a, r, b";
        let topology = GraphTopology::from_query_str(query);

        assert_eq!(topology.nodes.len(), 2);
        assert_eq!(topology.nodes[0].id, "a");
        assert_eq!(topology.nodes[0].label, "Account");
        assert_eq!(topology.nodes[1].id, "b");
        assert_eq!(topology.nodes[1].label, "Account");

        assert_eq!(topology.edges.len(), 1);
        assert_eq!(topology.edges[0].source, "a");
        assert_eq!(topology.edges[0].target, "b");
        assert_eq!(topology.edges[0].label, "TRANSFERRED");
    }

    #[test]
    fn test_extract_topology_raw_with_comments_and_hops() {
        let query = r#"
            // Find colleagues within 2 hops
            MATCH (u:User {name: 'Alice'})-[:KNOWS*1..2]->(f:User)
            /* optional following */
            OPTIONAL MATCH (f)<-[fl:FOLLOWS]-(other:User)
            WHERE f.age > 25
            RETURN u, f, other
        "#;
        let topology = GraphTopology::from_query_str(query);

        assert_eq!(topology.nodes.len(), 3);
        assert_eq!(topology.nodes[0].id, "u");
        assert_eq!(topology.nodes[0].label, "User");
        assert_eq!(topology.nodes[1].id, "f");
        assert_eq!(topology.nodes[2].id, "other");

        assert_eq!(topology.edges.len(), 2);
        assert_eq!(topology.edges[0].source, "u");
        assert_eq!(topology.edges[0].target, "f");
        assert_eq!(topology.edges[0].label, "KNOWS");
        assert_eq!(topology.edges[0].min_hops, Some(1));
        assert_eq!(topology.edges[0].max_hops, Some(2));

        assert_eq!(topology.edges[1].source, "other");
        assert_eq!(topology.edges[1].target, "f");
        assert_eq!(topology.edges[1].label, "FOLLOWS");
    }

    fn assert_topology_parity(topo_arena: &GraphTopology, topo_str: &GraphTopology) {
        assert_eq!(topo_arena.version, topo_str.version);
        assert_eq!(
            topo_arena.nodes.len(),
            topo_str.nodes.len(),
            "Node count mismatch: arena={:?} vs str={:?}",
            topo_arena.nodes,
            topo_str.nodes
        );
        assert_eq!(
            topo_arena.edges.len(),
            topo_str.edges.len(),
            "Edge count mismatch: arena={:?} vs str={:?}",
            topo_arena.edges,
            topo_str.edges
        );

        let nodes_arena: HashMap<&str, &TopologyNode> = topo_arena
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n))
            .collect();
        let nodes_str: HashMap<&str, &TopologyNode> =
            topo_str.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

        let mut ids_arena: Vec<&str> = nodes_arena.keys().copied().collect();
        let mut ids_str: Vec<&str> = nodes_str.keys().copied().collect();
        ids_arena.sort();
        ids_str.sort();
        assert_eq!(ids_arena, ids_str, "Node ID set mismatch");

        for (id, an) in &nodes_arena {
            let sn = nodes_str.get(id).expect("Node ID missing in topo_str");
            assert_eq!(an.label, sn.label, "Node {id} label mismatch");
            assert_eq!(an.labels, sn.labels, "Node {id} labels mismatch");
            assert_eq!(
                an.properties, sn.properties,
                "Node {id} properties mismatch"
            );
        }

        let mut edges_arena: Vec<(&str, &str, &str, &Direction)> = topo_arena
            .edges
            .iter()
            .map(|e| {
                (
                    e.source.as_str(),
                    e.target.as_str(),
                    e.label.as_str(),
                    &e.direction,
                )
            })
            .collect();
        edges_arena.sort();

        let mut edges_str: Vec<(&str, &str, &str, &Direction)> = topo_str
            .edges
            .iter()
            .map(|e| {
                (
                    e.source.as_str(),
                    e.target.as_str(),
                    e.label.as_str(),
                    &e.direction,
                )
            })
            .collect();
        edges_str.sort();

        assert_eq!(edges_arena, edges_str, "Edge signature mismatch");
    }

    #[test]
    fn test_topology_ast_string_parity() {
        // Archetype 1: Outgoing 1-hop with WHERE predicates
        let mut b1 = QueryBuilder::new();
        b1.r#match()
            .node(Some("p"), vec!["Person"])
            .where_eq("p", "name", "Alice")
            .to(vec!["KNOWS"], Some("r"))
            .node(Some("f"), vec!["Person"])
            .where_eq("f", "city", "London");
        let (arena1, root1) = b1.build();
        let topo_arena1 = GraphTopology::from_arena(&arena1, Some(root1));
        let query1 = "MATCH (p:Person)-[r:KNOWS]->(f:Person) WHERE p.name = 'Alice' AND f.city = 'London' RETURN p, r, f";
        let topo_str1 = GraphTopology::from_query_str(query1);
        assert_topology_parity(&topo_arena1, &topo_str1);

        // Archetype 2: Incoming 1-hop
        let mut b2 = QueryBuilder::new();
        b2.r#match()
            .node(Some("p"), vec!["Person"])
            .from(vec!["DIRECTED"], Some("d"))
            .node(Some("m"), vec!["Movie"]);
        let (arena2, root2) = b2.build();
        let topo_arena2 = GraphTopology::from_arena(&arena2, Some(root2));
        let query2 = "MATCH (p:Person)<-[d:DIRECTED]-(m:Movie) RETURN p, d, m";
        let topo_str2 = GraphTopology::from_query_str(query2);
        assert_topology_parity(&topo_arena2, &topo_str2);

        // Archetype 3: Undirected 1-hop
        let mut b3 = QueryBuilder::new();
        b3.r#match()
            .node(Some("a"), vec!["Account"])
            .edge(vec!["CONNECTED"], Some("c"))
            .node(Some("b"), vec!["Account"]);
        let (arena3, root3) = b3.build();
        let topo_arena3 = GraphTopology::from_arena(&arena3, Some(root3));
        let query3 = "MATCH (a:Account)-[c:CONNECTED]-(b:Account) RETURN a, c, b";
        let topo_str3 = GraphTopology::from_query_str(query3);
        assert_topology_parity(&topo_arena3, &topo_str3);

        // Archetype 4: 2-hop traversal chain
        let mut b4 = QueryBuilder::new();
        b4.r#match()
            .node(Some("a"), vec!["User"])
            .to(vec!["FOLLOWS"], Some("f1"))
            .node(Some("b"), vec!["User"])
            .to(vec!["FOLLOWS"], Some("f2"))
            .node(Some("c"), vec!["User"]);
        let (arena4, root4) = b4.build();
        let topo_arena4 = GraphTopology::from_arena(&arena4, Some(root4));
        let query4 = "MATCH (a:User)-[f1:FOLLOWS]->(b:User)-[f2:FOLLOWS]->(c:User) RETURN a, b, c";
        let topo_str4 = GraphTopology::from_query_str(query4);
        assert_topology_parity(&topo_arena4, &topo_str4);

        // Archetype 5: Reused source node / branching paths
        let mut b5 = QueryBuilder::new();
        b5.r#match()
            .node(Some("p"), vec!["Person"])
            .to(vec!["ACTED_IN"], Some("a"))
            .node(Some("m1"), vec!["Movie"]);
        b5.r#match()
            .node(Some("p"), vec!["Person"])
            .to(vec!["DIRECTED"], Some("d"))
            .node(Some("m2"), vec!["Movie"]);
        let (arena5, root5) = b5.build();
        let topo_arena5 = GraphTopology::from_arena(&arena5, Some(root5));
        let query5 = "MATCH (p:Person)-[a:ACTED_IN]->(m1:Movie) MATCH (p:Person)-[d:DIRECTED]->(m2:Movie) RETURN p";
        let topo_str5 = GraphTopology::from_query_str(query5);
        assert_topology_parity(&topo_arena5, &topo_str5);

        // Archetype 6: Standalone node with inline property
        let mut b6 = QueryBuilder::new();
        b6.r#match()
            .node(Some("u"), vec!["User"])
            .where_eq("u", "active", true);
        let (arena6, root6) = b6.build();
        let topo_arena6 = GraphTopology::from_arena(&arena6, Some(root6));
        let query6 = "MATCH (u:User {active: true}) RETURN u";
        let topo_str6 = GraphTopology::from_query_str(query6);
        assert_topology_parity(&topo_arena6, &topo_str6);

        // Archetype 7: CREATE mutation pattern
        let mut b7 = QueryBuilder::new();
        b7.create()
            .node(Some("n"), vec!["Item"])
            .to(vec!["CONTAINS"], Some("r"))
            .node(Some("sub"), vec!["Part"]);
        let (arena7, root7) = b7.build();
        let topo_arena7 = GraphTopology::from_arena(&arena7, Some(root7));
        let query7 = "CREATE (n:Item)-[r:CONTAINS]->(sub:Part)";
        let topo_str7 = GraphTopology::from_query_str(query7);
        assert_topology_parity(&topo_arena7, &topo_str7);

        // Archetype 8: MERGE mutation pattern
        let mut b8 = QueryBuilder::new();
        b8.merge()
            .node(Some("p"), vec!["Person"])
            .to(vec!["MEMBER_OF"], Some("m"))
            .node(Some("g"), vec!["Group"]);
        let (arena8, root8) = b8.build();
        let topo_arena8 = GraphTopology::from_arena(&arena8, Some(root8));
        let query8 = "MERGE (p:Person)-[m:MEMBER_OF]->(g:Group)";
        let topo_str8 = GraphTopology::from_query_str(query8);
        assert_topology_parity(&topo_arena8, &topo_str8);
    }

    #[test]
    fn test_topology_exists_subquery_descent() {
        let mut b = QueryBuilder::new();
        b.r#match().node(Some("u"), vec!["User"]);

        let sub_handle = b.subquery(|sub| {
            sub.r#match().node(Some("p"), vec!["Post"]);
        });
        let exists_expr = b.exists_subquery(sub_handle);
        b.where_predicate(exists_expr);

        let (arena, root) = b.build();
        let topology = GraphTopology::from_arena(&arena, Some(root));

        assert_eq!(topology.nodes.len(), 2);
        assert_eq!(topology.nodes[0].id, "u");
        assert_eq!(topology.nodes[0].label, "User");
        assert_eq!(topology.nodes[1].id, "p");
        assert_eq!(topology.nodes[1].label, "Post");
    }

    #[test]
    fn test_topology_subquery_inside_case_and_comprehension() {
        let mut b = QueryBuilder::new();
        b.r#match().node(Some("u"), vec!["User"]);

        let sub_handle = b.subquery(|sub| {
            sub.r#match().node(Some("p"), vec!["Post"]);
        });
        let exists_expr = b.exists_subquery(sub_handle);
        let true_lit = b.literal(true);
        let false_lit = b.literal(false);

        let case_handle = b.case_when(None, vec![(exists_expr, true_lit)], Some(false_lit));
        b.where_predicate(case_handle);

        let (arena, root) = b.build();
        let topology = GraphTopology::from_arena(&arena, Some(root));

        assert_eq!(topology.nodes.len(), 2);
        assert_eq!(topology.nodes[0].id, "u");
        assert_eq!(topology.nodes[1].id, "p");
    }

    #[test]
    fn test_topology_json_versioning() {
        let query = "MATCH (a:Account)-[r:TRANSFERRED]->(b:Account) RETURN a, r, b";
        let topo = GraphTopology::from_query_str(query);
        assert_eq!(topo.version, 1);
        #[cfg(feature = "serde")]
        {
            let json = topo.to_json().expect("JSON serialization failed");
            assert!(json.contains("\"version\":1"));
        }
    }

    #[test]
    fn test_strip_query_comments_backtick() {
        let query = "MATCH (n:`http://example.com/item`) // this is a line comment\nRETURN n";
        let cleaned = strip_query_comments(query);
        assert!(cleaned.contains("`http://example.com/item`"));
        assert!(!cleaned.contains("this is a line comment"));
    }

    #[test]
    fn test_extract_topology_gql_in_bracket_quantifier() {
        let query = "MATCH (a:Account)-[r:TRANSFERRED {1, 3}]->(b:Account) RETURN a, b";
        let topo = GraphTopology::from_query_str(query);

        assert_eq!(topo.nodes.len(), 2);
        assert_eq!(topo.edges.len(), 1);
        assert_eq!(topo.edges[0].label, "TRANSFERRED");
        assert_eq!(topo.edges[0].min_hops, Some(1));
        assert_eq!(topo.edges[0].max_hops, Some(3));
    }

    #[test]
    fn test_extract_topology_label_negation() {
        let query = "MATCH (n:!Internal) RETURN n";
        let topo = GraphTopology::from_query_str(query);

        assert_eq!(topo.nodes.len(), 1);
        assert_eq!(topo.nodes[0].id, "n");
        assert!(topo.nodes[0].labels.is_empty());
        assert_eq!(topo.nodes[0].label, "n");
    }

    #[test]
    fn test_extract_topology_quoted_equal() {
        let query = "MATCH (p:Person) WHERE p.name = \"foo=bar\" RETURN p";
        let topo = GraphTopology::from_query_str(query);

        assert_eq!(topo.nodes.len(), 1);
        assert_eq!(topo.nodes[0].id, "p");
        assert_eq!(
            topo.nodes[0].properties.get("name"),
            Some(&LiteralValue::String("foo=bar".to_string()))
        );
    }

    #[test]
    fn test_extract_topology_multi_label_and_merging() {
        // Multi-label node in raw string query
        let query = "MATCH (p:Person:Employee) RETURN p";
        let topo = GraphTopology::from_query_str(query);
        assert_eq!(topo.nodes.len(), 1);
        assert_eq!(topo.nodes[0].id, "p");
        assert_eq!(topo.nodes[0].label, "Person");
        assert_eq!(topo.nodes[0].labels, vec!["Person", "Employee"]);

        // Multi-label node in AST arena with subsequent pattern merging
        let mut b = QueryBuilder::new();
        b.r#match().node(Some("p"), vec!["Person"]);
        b.r#match().node(Some("p"), vec!["Employee"]);
        let (arena, root) = b.build();
        let topo_arena = GraphTopology::from_arena(&arena, Some(root));
        assert_eq!(topo_arena.nodes.len(), 1);
        assert_eq!(topo_arena.nodes[0].id, "p");
        assert_eq!(topo_arena.nodes[0].labels, vec!["Person", "Employee"]);
    }
}
