//! Graph entity and path topology reconstruction engine.
//!
//! Extracts structured node and edge topologies directly from the AST arena
//! or parses raw query statements across openCypher, ISO GQL, and SQL:2023 PGQ.

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
        Self {
            id: id_str,
            label: label_str,
            labels: Vec::new(),
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

/// Extracted graph topology containing deduplicated nodes and connected edges.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GraphTopology {
    /// Nodes extracted from path patterns.
    pub nodes: Vec<TopologyNode>,
    /// Directed and undirected edges extracted from path patterns.
    pub edges: Vec<TopologyEdge>,
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

        Self { nodes, edges }
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

        Self { nodes, edges }
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
                matches, mutations, ..
            } => {
                for &m_h in matches {
                    if let Ok(AstNode::MatchClause {
                        paths: match_paths,
                        where_clause,
                        ..
                    }) = arena.get(m_h)
                    {
                        paths.extend_from_slice(match_paths);
                        if let Some(wh) = where_clause {
                            wheres.push(*wh);
                        }
                    }
                }
                for &mut_h in mutations {
                    if let Ok(AstNode::CreateClause {
                        paths: create_paths,
                    }) = arena.get(mut_h)
                    {
                        paths.extend_from_slice(create_paths);
                    } else if let Ok(AstNode::MergeClause {
                        path: merge_path, ..
                    }) = arena.get(mut_h)
                    {
                        paths.push(*merge_path);
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
                }
            }
            AstNode::CreateClause { paths: match_paths } => {
                paths.extend_from_slice(match_paths);
            }
            AstNode::MergeClause { path, .. } => {
                paths.push(*path);
            }
            AstNode::PathChain { .. } | AstNode::NodePattern { .. } => {
                paths.push(root);
            }
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

        if !nodes_map.contains_key(&node_id) {
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

    while i < len {
        let ch = chars[i];

        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            out.push(ch);
            i += 1;
            continue;
        }

        if !in_single_quote && !in_double_quote {
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

fn parse_where_predicates_body(body: &str, nodes_map: &mut HashMap<String, TopologyNode>) {
    for part in body.split(['\n', '\r']) {
        for expr in part.split(" AND ").flat_map(|s| s.split(" and ")) {
            let expr = expr.trim();
            let (left, right) = if let Some((l, r)) = expr.split_once("==") {
                (l.trim(), r.trim())
            } else if let Some((l, r)) = expr.split_once('=') {
                (l.trim(), r.trim())
            } else {
                continue;
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
                if !clean_lbl.is_empty() {
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

/// Parses the inner string of an edge pattern: `var:TYPE1|TYPE2*1..3 {prop: val}`
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

    let (ident_part, props_part) = if let Some(idx) = trimmed.find('{') {
        (&trimmed[..idx], Some(&trimmed[idx..]))
    } else {
        (trimmed, None)
    };
    let inline_props = props_part.map(parse_inline_properties).unwrap_or_default();

    let mut variable = None;
    let mut types = Vec::new();
    let mut min_hops = None;
    let mut max_hops = None;

    // Check for hop quantifiers: `*1..3`, `*..5`, `*2`, `*`
    let (ident_clean, hop_part) = if let Some(star_idx) = ident_part.find('*') {
        (&ident_part[..star_idx], Some(&ident_part[star_idx + 1..]))
    } else {
        (ident_part, None)
    };

    if let Some(hops) = hop_part {
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
}
