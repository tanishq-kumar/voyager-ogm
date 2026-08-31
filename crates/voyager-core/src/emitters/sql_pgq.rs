//! SQL:2023 Property Graph Queries (PGQ) & DuckPGQ Emitter.

use crate::ast::{
    AggregationFunc, AstNode, BinaryOp, Direction, LiteralValue, NodeHandle, ProjectionItem,
    QueryAstArena,
};
use crate::error::{Error, Result};
use crate::visitor::{AstVisitor, CompiledQuery};
use std::collections::HashMap;

/// Emits standardized ISO/IEC 9075-16:2023 SQL:PGQ `GRAPH_TABLE` queries.
#[derive(Debug)]
pub struct SqlPgqEmitter {
    graph_name: String,
    param_counter: usize,
    parameters: HashMap<String, LiteralValue>,
    buffer: String,
}

impl Default for SqlPgqEmitter {
    fn default() -> Self {
        Self::new("graph_name")
    }
}

impl SqlPgqEmitter {
    /// Creates a fresh SQL:PGQ emitter targeting a specific property graph schema name.
    pub fn new(graph_name: impl Into<String>) -> Self {
        Self {
            graph_name: graph_name.into(),
            param_counter: 0,
            parameters: HashMap::new(),
            buffer: String::with_capacity(256),
        }
    }

    fn emit_node_pattern(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::NodePattern {
            variable,
            labels,
            predicates: _,
        } = node
        {
            self.buffer.push('(');
            if let Some(var) = variable {
                self.buffer.push_str(var);
            }
            for label in labels {
                self.buffer.push_str(" IS ");
                self.buffer.push_str(label);
            }
            self.buffer.push(')');
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected NodePattern, got {node:?}"
            )))
        }
    }

    fn emit_edge_pattern(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::EdgePattern {
            variable,
            edge_types,
            direction,
            min_hops,
            max_hops,
            predicates: _,
            target_node,
        } = node
        {
            match direction {
                Direction::Incoming => self.buffer.push_str(" <-["),
                Direction::Outgoing | Direction::Undirected => self.buffer.push_str(" -["),
            }

            if let Some(var) = variable {
                self.buffer.push_str(var);
            }

            for (i, edge_type) in edge_types.iter().enumerate() {
                if i == 0 {
                    self.buffer.push_str(" IS ");
                } else {
                    self.buffer.push_str(" | ");
                }
                self.buffer.push_str(edge_type);
            }

            if min_hops.is_some() || max_hops.is_some() {
                self.buffer.push('{');
                if let Some(min) = min_hops {
                    self.buffer.push_str(&min.to_string());
                } else {
                    self.buffer.push('1');
                }
                self.buffer.push(',');
                if let Some(max) = max_hops {
                    self.buffer.push_str(&max.to_string());
                }
                self.buffer.push('}');
            }

            match direction {
                Direction::Outgoing => self.buffer.push_str("]-> "),
                Direction::Incoming | Direction::Undirected => self.buffer.push_str("]- "),
            }

            self.emit_node_pattern(arena, *target_node)?;
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected EdgePattern, got {node:?}"
            )))
        }
    }

    fn emit_path(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        match node {
            AstNode::NodePattern { .. } => self.emit_node_pattern(arena, handle),
            AstNode::PathChain { start_node, edges } => {
                self.emit_node_pattern(arena, *start_node)?;
                for &edge_handle in edges {
                    self.emit_edge_pattern(arena, edge_handle)?;
                }
                Ok(())
            }
            other => Err(Error::AstInvariantViolation(format!(
                "Expected PathChain or NodePattern, got {other:?}"
            ))),
        }
    }

    fn emit_expression(
        &mut self,
        arena: &QueryAstArena,
        handle: NodeHandle,
        nested: bool,
    ) -> Result<()> {
        let node = arena.get(handle)?;
        match node {
            AstNode::Identifier(id) => {
                self.buffer.push_str(id);
                Ok(())
            }
            AstNode::PropertyAccess { target, property } => {
                self.emit_expression(arena, *target, false)?;
                self.buffer.push('.');
                self.buffer.push_str(property);
                Ok(())
            }
            AstNode::Literal(lit) => {
                let param_name = format!("p{}", self.param_counter);
                self.param_counter += 1;
                self.buffer.push('$');
                self.buffer.push_str(&param_name);
                self.parameters.insert(param_name, lit.clone());
                Ok(())
            }
            AstNode::BinaryExpression { left, op, right } => {
                if nested {
                    self.buffer.push('(');
                }
                self.emit_expression(arena, *left, true)?;
                match op {
                    BinaryOp::Contains => {
                        self.buffer.push_str(" LIKE '%' || ");
                        self.emit_expression(arena, *right, true)?;
                        self.buffer.push_str(" || '%'");
                    }
                    BinaryOp::StartsWith => {
                        self.buffer.push_str(" LIKE ");
                        self.emit_expression(arena, *right, true)?;
                        self.buffer.push_str(" || '%'");
                    }
                    BinaryOp::EndsWith => {
                        self.buffer.push_str(" LIKE '%' || ");
                        self.emit_expression(arena, *right, true)?;
                    }
                    other => {
                        self.buffer.push(' ');
                        self.buffer.push_str(&other.to_string());
                        self.buffer.push(' ');
                        self.emit_expression(arena, *right, true)?;
                    }
                }
                if nested {
                    self.buffer.push(')');
                }
                Ok(())
            }
            other => Err(Error::AstInvariantViolation(format!(
                "Unsupported expression node: {other:?}"
            ))),
        }
    }

    fn emit_where(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::WhereClause { root_predicate } = node {
            self.buffer.push_str(" WHERE ");
            self.emit_expression(arena, *root_predicate, false)?;
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected WhereClause, got {node:?}"
            )))
        }
    }

    fn emit_columns(
        &mut self,
        arena: &QueryAstArena,
        projections: &[ProjectionItem],
    ) -> Result<()> {
        self.buffer.push_str(" COLUMNS (");
        for (i, proj) in projections.iter().enumerate() {
            if i > 0 {
                self.buffer.push_str(", ");
            }
            if let Some(func) = proj.aggregation {
                match func {
                    AggregationFunc::Count => self.buffer.push_str("COUNT("),
                    AggregationFunc::CountDistinct => self.buffer.push_str("COUNT(DISTINCT "),
                    AggregationFunc::Sum => self.buffer.push_str("SUM("),
                    AggregationFunc::Avg => self.buffer.push_str("AVG("),
                    AggregationFunc::Min => self.buffer.push_str("MIN("),
                    AggregationFunc::Max => self.buffer.push_str("MAX("),
                    AggregationFunc::Collect => self.buffer.push_str("ARRAY_AGG("),
                }
                self.emit_expression(arena, proj.expression, false)?;
                self.buffer.push(')');
            } else {
                self.emit_expression(arena, proj.expression, false)?;
            }

            if let Some(alias) = &proj.alias {
                self.buffer.push_str(" AS ");
                self.buffer.push_str(alias);
            }
        }
        self.buffer.push(')');
        Ok(())
    }
}

impl AstVisitor for SqlPgqEmitter {
    fn visit_query(&mut self, arena: &QueryAstArena, root: NodeHandle) -> Result<CompiledQuery> {
        self.buffer.clear();
        self.parameters.clear();
        self.param_counter = 0;

        let root_node = arena.get(root)?;
        if let AstNode::QueryStatement {
            unwinds: _,
            matches,
            mutations: _,
            return_clause,
        } = root_node
        {
            self.buffer.push_str("SELECT * FROM GRAPH_TABLE (");
            self.buffer.push_str(&self.graph_name);
            self.buffer.push_str(" MATCH ");

            for (m_idx, &match_handle) in matches.iter().enumerate() {
                if m_idx > 0 {
                    self.buffer.push_str(", ");
                }
                let match_node = arena.get(match_handle)?;
                if let AstNode::MatchClause {
                    paths,
                    where_clause,
                    ..
                } = match_node
                {
                    for (p_idx, &path_handle) in paths.iter().enumerate() {
                        if p_idx > 0 {
                            self.buffer.push_str(", ");
                        }
                        self.emit_path(arena, path_handle)?;
                    }

                    if let Some(wh) = where_clause {
                        self.emit_where(arena, *wh)?;
                    }
                }
            }

            let mut order_by_clause = None;
            let mut limit_clause = None;
            let mut skip_clause = None;

            if let Some(ret_handle) = return_clause {
                let ret_node = arena.get(*ret_handle)?;
                if let AstNode::ReturnClause {
                    distinct: _,
                    projections,
                    order_by,
                    skip,
                    limit,
                } = ret_node
                {
                    self.emit_columns(arena, projections)?;
                    order_by_clause = Some(order_by);
                    limit_clause = *limit;
                    skip_clause = *skip;
                }
            }

            self.buffer.push(')');

            if let Some(order_by) = order_by_clause
                && !order_by.is_empty()
            {
                self.buffer.push_str(" ORDER BY ");
                for (i, (order_expr, is_asc)) in order_by.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.emit_expression(arena, *order_expr, false)?;
                    if *is_asc {
                        self.buffer.push_str(" ASC");
                    } else {
                        self.buffer.push_str(" DESC");
                    }
                }
            }

            if let Some(limit) = limit_clause {
                self.buffer.push_str(&format!(" LIMIT {limit}"));
            }

            if let Some(skip) = skip_clause {
                self.buffer.push_str(&format!(" OFFSET {skip}"));
            }

            Ok(CompiledQuery::new(
                std::mem::take(&mut self.buffer),
                std::mem::take(&mut self.parameters),
            ))
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Invalid root AST node for SQL:PGQ: {root_node:?}"
            )))
        }
    }
}
