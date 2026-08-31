//! Ergonomic Fluent Query Builder for constructing Voyager OGM ASTs.
//!
//! Designed after openCypher, ISO GQL, and Memgraph/GQLAlchemy chained traversal models:
//!
//! ```rust
//! use voyager_core::builder::QueryBuilder;
//! use voyager_core::ast::BinaryOp;
//!
//! let mut builder = QueryBuilder::new();
//! builder
//!     .r#match()
//!     .node(Some("p"), vec!["Person"])
//!     .to(vec!["ACTED_IN"], Some("r"))
//!     .node(Some("m"), vec!["Movie"])
//!     .from(vec!["DIRECTED"], Some("d_rel"))
//!     .node(Some("d"), vec!["Director"])
//!     .where_gt("p", "age", 21)
//!     .r#return()
//!     .field("p", "name", Some("actor_name"))
//!     .field("m", "title", Some("movie_title"))
//!     .order_by_desc("m", "released")
//!     .limit(10);
//!
//! let (arena, root_handle) = builder.build();
//! ```

use crate::ast::{
    AggregationFunc, AstNode, BinaryOp, Direction, LiteralValue, NodeHandle, ProjectionItem,
    QueryAstArena,
};

#[derive(Debug, Clone)]
struct PendingEdge {
    direction: Direction,
    edge_types: Vec<String>,
    variable: Option<String>,
    min_hops: Option<u32>,
    max_hops: Option<u32>,
}

/// Fluent query builder for assembling ASTs with type safety and zero pointer indirection.
#[derive(Debug, Default, Clone)]
pub struct QueryBuilder {
    arena: QueryAstArena,
    match_clauses: Vec<NodeHandle>,
    is_optional_match: bool,
    current_path_start: Option<NodeHandle>,
    current_edges: Vec<NodeHandle>,
    pending_edge: Option<PendingEdge>,
    current_where_predicates: Vec<NodeHandle>,
    projections: Vec<ProjectionItem>,
    order_bys: Vec<(NodeHandle, bool)>,
    distinct: bool,
    limit: Option<u64>,
    skip: Option<u64>,
}

impl QueryBuilder {
    /// Creates a fresh query builder with pre-allocated arena capacity.
    pub fn new() -> Self {
        Self {
            arena: QueryAstArena::new(),
            ..Default::default()
        }
    }

    /// Allocates an identifier variable node in the arena.
    #[inline(always)]
    pub fn ident(&mut self, name: impl Into<String>) -> NodeHandle {
        self.arena.alloc(AstNode::Identifier(name.into()))
    }

    /// Allocates a literal value node in the arena.
    #[inline(always)]
    pub fn literal(&mut self, val: impl Into<LiteralValue>) -> NodeHandle {
        self.arena.alloc(AstNode::Literal(val.into()))
    }

    /// Allocates a property access expression node: `variable.property`.
    #[inline(always)]
    pub fn prop(&mut self, var_name: impl Into<String>, property: impl Into<String>) -> NodeHandle {
        let target = self.ident(var_name);
        self.arena.alloc(AstNode::PropertyAccess {
            target,
            property: property.into(),
        })
    }

    /// Allocates a binary comparison or logical expression.
    #[inline(always)]
    pub fn binary_expr(&mut self, left: NodeHandle, op: BinaryOp, right: NodeHandle) -> NodeHandle {
        self.arena
            .alloc(AstNode::BinaryExpression { left, op, right })
    }

    // ========================================================
    // MATCH & Path Chaining (Memgraph & GQLAlchemy Pattern)
    // ========================================================

    /// Starts a new mandatory `MATCH` block.
    pub fn r#match(&mut self) -> &mut Self {
        self.flush_current_path();
        self.is_optional_match = false;
        self
    }

    /// Starts a new `OPTIONAL MATCH` block.
    pub fn optional_match(&mut self) -> &mut Self {
        self.flush_current_path();
        self.is_optional_match = true;
        self
    }

    /// Defines a node in the graph traversal path: `(variable:Label1:Label2)`.
    ///
    /// If an edge traversal (`.to()`, `.from()`, or `.edge()`) is currently pending,
    /// this node serves as the destination target for that relationship.
    pub fn node(
        &mut self,
        variable: Option<impl Into<String>>,
        labels: Vec<impl Into<String>>,
    ) -> &mut Self {
        let node_handle = self.arena.alloc(AstNode::NodePattern {
            variable: variable.map(Into::into),
            labels: labels.into_iter().map(Into::into).collect(),
            predicates: Vec::new(),
        });

        if let Some(pending) = self.pending_edge.take() {
            let edge_handle = self.arena.alloc(AstNode::EdgePattern {
                variable: pending.variable,
                edge_types: pending.edge_types,
                direction: pending.direction,
                min_hops: pending.min_hops,
                max_hops: pending.max_hops,
                predicates: Vec::new(),
                target_node: node_handle,
            });
            self.current_edges.push(edge_handle);
        } else if self.current_path_start.is_some() {
            self.flush_current_path();
            self.current_path_start = Some(node_handle);
        } else {
            self.current_path_start = Some(node_handle);
        }

        self
    }

    /// Alias for [`Self::node`] matching a primary node.
    #[inline(always)]
    pub fn match_node(
        &mut self,
        variable: Option<impl Into<String>>,
        labels: Vec<impl Into<String>>,
    ) -> &mut Self {
        self.node(variable, labels)
    }

    /// Convenient single-label node shortcut without variable: `(:Label)`.
    #[inline(always)]
    pub fn node_label(&mut self, label: impl Into<String>) -> &mut Self {
        self.node(None::<String>, vec![label])
    }

    /// Chains an **outgoing** relationship traversal: `(current)-[r:TYPE]->(next_node)`.
    pub fn to(
        &mut self,
        edge_types: Vec<impl Into<String>>,
        variable: Option<impl Into<String>>,
    ) -> &mut Self {
        self.pending_edge = Some(PendingEdge {
            direction: Direction::Outgoing,
            edge_types: edge_types.into_iter().map(Into::into).collect(),
            variable: variable.map(Into::into),
            min_hops: None,
            max_hops: None,
        });
        self
    }

    /// Alias for [`Self::to`] representing an outgoing edge.
    #[inline(always)]
    pub fn out_edge(
        &mut self,
        edge_types: Vec<impl Into<String>>,
        variable: Option<impl Into<String>>,
    ) -> &mut Self {
        self.to(edge_types, variable)
    }

    /// Chains an **incoming** relationship traversal: `(current)<-[r:TYPE]-(next_node)`.
    pub fn from(
        &mut self,
        edge_types: Vec<impl Into<String>>,
        variable: Option<impl Into<String>>,
    ) -> &mut Self {
        self.pending_edge = Some(PendingEdge {
            direction: Direction::Incoming,
            edge_types: edge_types.into_iter().map(Into::into).collect(),
            variable: variable.map(Into::into),
            min_hops: None,
            max_hops: None,
        });
        self
    }

    /// Alias for [`Self::from`] representing an incoming edge.
    #[inline(always)]
    pub fn in_edge(
        &mut self,
        edge_types: Vec<impl Into<String>>,
        variable: Option<impl Into<String>>,
    ) -> &mut Self {
        self.from(edge_types, variable)
    }

    /// Chains an **undirected** relationship traversal: `(current)-[r:TYPE]-(next_node)`.
    pub fn edge(
        &mut self,
        edge_types: Vec<impl Into<String>>,
        variable: Option<impl Into<String>>,
    ) -> &mut Self {
        self.pending_edge = Some(PendingEdge {
            direction: Direction::Undirected,
            edge_types: edge_types.into_iter().map(Into::into).collect(),
            variable: variable.map(Into::into),
            min_hops: None,
            max_hops: None,
        });
        self
    }

    /// Combined helper to append an edge and target node pattern in one call.
    pub fn to_edge(
        &mut self,
        direction: Direction,
        edge_types: Vec<impl Into<String>>,
        variable: Option<impl Into<String>>,
        target_variable: Option<impl Into<String>>,
        target_labels: Vec<impl Into<String>>,
    ) -> &mut Self {
        match direction {
            Direction::Outgoing => self.to(edge_types, variable),
            Direction::Incoming => self.from(edge_types, variable),
            Direction::Undirected => self.edge(edge_types, variable),
        };
        self.node(target_variable, target_labels)
    }

    /// Configures variable-length hop repetition (e.g. `*1..3`) on the active edge pattern.
    pub fn hops(&mut self, min: u32, max: u32) -> &mut Self {
        if let Some(pending) = self.pending_edge.as_mut() {
            pending.min_hops = Some(min);
            pending.max_hops = Some(max);
        } else if let Some(&edge_handle) = self.current_edges.last()
            && let Ok(AstNode::EdgePattern {
                min_hops, max_hops, ..
            }) = self.arena.get_mut(edge_handle)
        {
            *min_hops = Some(min);
            *max_hops = Some(max);
        }
        self
    }

    // ========================================================
    // WHERE Predicates & Filters
    // ========================================================

    /// Appends a WHERE predicate expression to the active match block.
    pub fn where_predicate(&mut self, predicate: NodeHandle) -> &mut Self {
        self.current_where_predicates.push(predicate);
        self
    }

    /// Fluent alias for [`Self::where_predicate`].
    #[inline(always)]
    pub fn r#where(&mut self, predicate: NodeHandle) -> &mut Self {
        self.where_predicate(predicate)
    }

    /// Fluent alias for [`Self::where_predicate`].
    #[inline(always)]
    pub fn filter(&mut self, predicate: NodeHandle) -> &mut Self {
        self.where_predicate(predicate)
    }

    /// Appends a property comparison WHERE condition: `var.prop OP literal`.
    pub fn where_property(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        op: BinaryOp,
        value: impl Into<LiteralValue>,
    ) -> &mut Self {
        let left = self.prop(var, prop);
        let right = self.literal(value);
        let expr = self.binary_expr(left, op, right);
        self.where_predicate(expr)
    }

    /// Fluent alias for [`Self::where_property`].
    #[inline(always)]
    pub fn where_field(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        op: BinaryOp,
        value: impl Into<LiteralValue>,
    ) -> &mut Self {
        self.where_property(var, prop, op, value)
    }

    /// Shortcut for equality WHERE condition: `var.prop = value`.
    #[inline(always)]
    pub fn where_eq(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        value: impl Into<LiteralValue>,
    ) -> &mut Self {
        self.where_property(var, prop, BinaryOp::Eq, value)
    }

    /// Shortcut for greater-than WHERE condition: `var.prop > value`.
    #[inline(always)]
    pub fn where_gt(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        value: impl Into<LiteralValue>,
    ) -> &mut Self {
        self.where_property(var, prop, BinaryOp::Gt, value)
    }

    /// Shortcut for greater-than-or-equal WHERE condition: `var.prop >= value`.
    #[inline(always)]
    pub fn where_gte(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        value: impl Into<LiteralValue>,
    ) -> &mut Self {
        self.where_property(var, prop, BinaryOp::Gte, value)
    }

    /// Shortcut for less-than WHERE condition: `var.prop < value`.
    #[inline(always)]
    pub fn where_lt(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        value: impl Into<LiteralValue>,
    ) -> &mut Self {
        self.where_property(var, prop, BinaryOp::Lt, value)
    }

    /// Shortcut for less-than-or-equal WHERE condition: `var.prop <= value`.
    #[inline(always)]
    pub fn where_lte(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        value: impl Into<LiteralValue>,
    ) -> &mut Self {
        self.where_property(var, prop, BinaryOp::Lte, value)
    }

    /// Shortcut for string substring WHERE condition: `var.prop CONTAINS value`.
    #[inline(always)]
    pub fn where_contains(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        value: impl Into<LiteralValue>,
    ) -> &mut Self {
        self.where_property(var, prop, BinaryOp::Contains, value)
    }

    // ========================================================
    // RETURN / Projections
    // ========================================================

    /// Starts or configures the `RETURN` clause.
    #[inline(always)]
    pub fn r#return(&mut self) -> &mut Self {
        self.flush_current_path();
        self
    }

    /// Adds expression projections to the RETURN clause.
    pub fn select(&mut self, items: Vec<ProjectionItem>) -> &mut Self {
        self.projections.extend(items);
        self
    }

    /// Fluent alias for [`Self::select`].
    #[inline(always)]
    pub fn project(&mut self, items: Vec<ProjectionItem>) -> &mut Self {
        self.select(items)
    }

    /// Appends an aliased column projection: `expr AS alias`.
    pub fn select_expr(
        &mut self,
        expression: NodeHandle,
        alias: Option<impl Into<String>>,
    ) -> &mut Self {
        self.projections.push(ProjectionItem {
            expression,
            alias: alias.map(Into::into),
            aggregation: None,
        });
        self
    }

    /// Appends a property column projection directly: `var.prop AS alias`.
    pub fn select_property(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        alias: Option<impl Into<String>>,
    ) -> &mut Self {
        let expr = self.prop(var, prop);
        self.select_expr(expr, alias)
    }

    /// Fluent alias for [`Self::select_property`].
    #[inline(always)]
    pub fn field(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        alias: Option<impl Into<String>>,
    ) -> &mut Self {
        self.select_property(var, prop, alias)
    }

    /// Appends an aggregated column projection: `COUNT(expr) AS alias`.
    pub fn select_aggregate(
        &mut self,
        expression: NodeHandle,
        func: AggregationFunc,
        alias: Option<impl Into<String>>,
    ) -> &mut Self {
        self.projections.push(ProjectionItem {
            expression,
            alias: alias.map(Into::into),
            aggregation: Some(func),
        });
        self
    }

    /// Appends an aggregated property column projection directly: `AVG(var.prop) AS alias`.
    pub fn select_property_aggregate(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        func: AggregationFunc,
        alias: Option<impl Into<String>>,
    ) -> &mut Self {
        let expr = self.prop(var, prop);
        self.select_aggregate(expr, func, alias)
    }

    /// Enables distinct result projection (`RETURN DISTINCT`).
    pub fn distinct(&mut self, is_distinct: bool) -> &mut Self {
        self.distinct = is_distinct;
        self
    }

    // ========================================================
    // ORDER BY & Pagination
    // ========================================================

    /// Adds an ORDER BY sort specification.
    pub fn order_by(&mut self, expr: NodeHandle, ascending: bool) -> &mut Self {
        self.order_bys.push((expr, ascending));
        self
    }

    /// Adds an ORDER BY sort on a property: `ORDER BY var.prop ASC/DESC`.
    pub fn order_by_property(
        &mut self,
        var: impl Into<String>,
        prop: impl Into<String>,
        ascending: bool,
    ) -> &mut Self {
        let expr = self.prop(var, prop);
        self.order_by(expr, ascending)
    }

    /// Sorts ascending by property: `ORDER BY var.prop ASC`.
    #[inline(always)]
    pub fn order_by_asc(&mut self, var: impl Into<String>, prop: impl Into<String>) -> &mut Self {
        self.order_by_property(var, prop, true)
    }

    /// Sorts descending by property: `ORDER BY var.prop DESC`.
    #[inline(always)]
    pub fn order_by_desc(&mut self, var: impl Into<String>, prop: impl Into<String>) -> &mut Self {
        self.order_by_property(var, prop, false)
    }

    /// Sets query result limit.
    pub fn limit(&mut self, limit: u64) -> &mut Self {
        self.limit = Some(limit);
        self
    }

    /// Sets query pagination offset / skip.
    pub fn skip(&mut self, skip: u64) -> &mut Self {
        self.skip = Some(skip);
        self
    }

    fn flush_current_path(&mut self) {
        if let Some(start_node) = self.current_path_start.take() {
            let path_handle = if self.current_edges.is_empty() {
                start_node
            } else {
                let edges = std::mem::take(&mut self.current_edges);
                self.arena.alloc(AstNode::PathChain { start_node, edges })
            };

            let where_clause = if self.current_where_predicates.is_empty() {
                None
            } else {
                let preds = std::mem::take(&mut self.current_where_predicates);
                let root_pred = if preds.len() == 1 {
                    preds[0]
                } else {
                    let mut combined = preds[0];
                    for &next_pred in &preds[1..] {
                        combined = self.arena.alloc(AstNode::BinaryExpression {
                            left: combined,
                            op: BinaryOp::And,
                            right: next_pred,
                        });
                    }
                    combined
                };
                Some(self.arena.alloc(AstNode::WhereClause {
                    root_predicate: root_pred,
                }))
            };

            let match_handle = self.arena.alloc(AstNode::MatchClause {
                optional: self.is_optional_match,
                paths: vec![path_handle],
                where_clause,
            });

            self.match_clauses.push(match_handle);
        }
    }

    /// Finalizes the AST and returns the completed arena alongside the root statement handle.
    pub fn build(mut self) -> (QueryAstArena, NodeHandle) {
        self.flush_current_path();

        let return_clause = if self.projections.is_empty() {
            None
        } else {
            let ret = self.arena.alloc(AstNode::ReturnClause {
                distinct: self.distinct,
                projections: self.projections,
                order_by: self.order_bys,
                skip: self.skip,
                limit: self.limit,
            });
            Some(ret)
        };

        let root_handle = self.arena.alloc(AstNode::QueryStatement {
            matches: self.match_clauses,
            return_clause,
        });

        (self.arena, root_handle)
    }
}

// Convert from Rust primitive types into LiteralValue
impl From<bool> for LiteralValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i32> for LiteralValue {
    fn from(v: i32) -> Self {
        Self::Int64(v as i64)
    }
}

impl From<i64> for LiteralValue {
    fn from(v: i64) -> Self {
        Self::Int64(v)
    }
}

impl From<f64> for LiteralValue {
    fn from(v: f64) -> Self {
        Self::Float64(v)
    }
}

impl From<&str> for LiteralValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<String> for LiteralValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}
