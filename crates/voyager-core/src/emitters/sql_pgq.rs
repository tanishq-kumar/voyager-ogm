//! SQL:2023 Property Graph Queries (PGQ) & DuckPGQ Emitter.

use crate::ast::{
    AggregationFunc, AstNode, BinaryOp, Direction, LiteralValue, NodeHandle, ProjectionItem,
    QueryAstArena, UnaryOp,
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
            predicates,
        } = node
        {
            self.buffer.push('(');
            let mut has_prefix = false;
            if let Some(var) = variable {
                self.buffer.push_str(var);
                has_prefix = true;
            }
            for label in labels {
                self.buffer.push_str(" IS ");
                self.buffer.push_str(label);
                has_prefix = true;
            }
            if !predicates.is_empty() {
                if has_prefix {
                    self.buffer.push(' ');
                }
                self.buffer.push_str("WHERE ");
                for (i, &pred_handle) in predicates.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(" AND ");
                    }
                    self.emit_expression(arena, pred_handle, false)?;
                }
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
            predicates,
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

            if !predicates.is_empty() {
                self.buffer.push_str(" WHERE ");
                for (i, &pred_handle) in predicates.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(" AND ");
                    }
                    self.emit_expression(arena, pred_handle, false)?;
                }
            }

            self.buffer.push(']');

            match direction {
                Direction::Outgoing => self.buffer.push_str("->"),
                Direction::Incoming | Direction::Undirected => self.buffer.push('-'),
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

            self.buffer.push(' ');

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
            AstNode::NodePattern {
                variable: Some(var),
                ..
            } => {
                self.buffer.push_str(var);
                Ok(())
            }
            AstNode::EdgePattern {
                variable: Some(var),
                ..
            } => {
                self.buffer.push_str(var);
                Ok(())
            }
            AstNode::PropertyAccess { target, property } => {
                self.emit_expression(arena, *target, false)?;
                if !property.is_empty() {
                    self.buffer.push('.');
                    self.buffer.push_str(property);
                }
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
                    BinaryOp::Concat => {
                        self.buffer.push_str(" || ");
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
            AstNode::UnaryExpression { op, operand } => match op {
                UnaryOp::Not => {
                    self.buffer.push_str("NOT (");
                    self.emit_expression(arena, *operand, false)?;
                    self.buffer.push(')');
                    Ok(())
                }
                UnaryOp::Neg => {
                    self.buffer.push('-');
                    self.emit_expression(arena, *operand, true)
                }
                UnaryOp::IsNull => {
                    self.emit_expression(arena, *operand, true)?;
                    self.buffer.push_str(" IS NULL");
                    Ok(())
                }
                UnaryOp::IsNotNull => {
                    self.emit_expression(arena, *operand, true)?;
                    self.buffer.push_str(" IS NOT NULL");
                    Ok(())
                }
            },
            AstNode::FunctionCall { name, arguments } => {
                let sql_func = match name.to_ascii_lowercase().as_str() {
                    "tolower" | "lower" => "LOWER",
                    "toupper" | "upper" => "UPPER",
                    "size" | "length" | "char_length" | "character_length" => "LENGTH",
                    "cardinality" => "CARDINALITY",
                    "coalesce" => "COALESCE",
                    "trim" => "TRIM",
                    "ltrim" => "LTRIM",
                    "rtrim" => "RTRIM",
                    "split" => "STRING_SPLIT",
                    "substring" => "SUBSTRING",
                    "replace" => "REPLACE",
                    "reverse" => "REVERSE",
                    "left" => "LEFT",
                    "right" => "RIGHT",
                    "abs" => "ABS",
                    "sqrt" => "SQRT",
                    "ceil" => "CEIL",
                    "floor" => "FLOOR",
                    "round" => "ROUND",
                    "log" => "LN",
                    "log10" => "LOG10",
                    "exp" => "EXP",
                    "sin" => "SIN",
                    "cos" => "COS",
                    "tan" => "TAN",
                    "asin" => "ASIN",
                    "acos" => "ACOS",
                    "atan" => "ATAN",
                    _ => name.as_str(),
                };
                self.buffer.push_str(sql_func);
                self.buffer.push('(');
                for (i, &arg) in arguments.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.emit_expression(arena, arg, false)?;
                }
                self.buffer.push(')');
                Ok(())
            }
            AstNode::CaseExpression {
                operand,
                when_then_branches,
                else_branch,
            } => {
                self.buffer.push_str("CASE");
                if let Some(op) = operand {
                    self.buffer.push(' ');
                    self.emit_expression(arena, *op, false)?;
                }
                for (when_expr, then_expr) in when_then_branches {
                    self.buffer.push_str(" WHEN ");
                    self.emit_expression(arena, *when_expr, false)?;
                    self.buffer.push_str(" THEN ");
                    self.emit_expression(arena, *then_expr, false)?;
                }
                if let Some(else_expr) = else_branch {
                    self.buffer.push_str(" ELSE ");
                    self.emit_expression(arena, *else_expr, false)?;
                }
                self.buffer.push_str(" END");
                Ok(())
            }
            AstNode::ExistsSubquery { subquery } => {
                self.buffer.push_str("EXISTS (");
                self.emit_expression(arena, *subquery, false)?;
                self.buffer.push(')');
                Ok(())
            }
            AstNode::CountSubquery { subquery } => {
                self.buffer.push_str("COUNT (");
                self.emit_expression(arena, *subquery, false)?;
                self.buffer.push(')');
                Ok(())
            }
            AstNode::ListLiteral(items) => {
                self.buffer.push('[');
                for (i, &item) in items.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.emit_expression(arena, item, false)?;
                }
                self.buffer.push(']');
                Ok(())
            }
            other => Err(Error::AstInvariantViolation(format!(
                "Unsupported expression node: {other:?}"
            ))),
        }
    }

    fn render_expression_to_string(
        &mut self,
        arena: &QueryAstArena,
        handle: NodeHandle,
    ) -> Result<String> {
        let saved_buffer = std::mem::take(&mut self.buffer);
        let res = self.emit_expression(arena, handle, false);
        let expr_str = std::mem::replace(&mut self.buffer, saved_buffer);
        res.map(|_| expr_str)
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

    fn expressions_match(&self, arena: &QueryAstArena, a: NodeHandle, b: NodeHandle) -> bool {
        if a == b {
            return true;
        }
        match (arena.get(a), arena.get(b)) {
            (
                Ok(AstNode::PropertyAccess {
                    target: t1,
                    property: p1,
                }),
                Ok(AstNode::PropertyAccess {
                    target: t2,
                    property: p2,
                }),
            ) => p1 == p2 && self.expressions_match(arena, *t1, *t2),
            (Ok(AstNode::Identifier(id1)), Ok(AstNode::Identifier(id2))) => id1 == id2,
            (
                Ok(AstNode::NodePattern {
                    variable: Some(v1), ..
                }),
                Ok(AstNode::Identifier(id2)),
            ) => v1 == id2,
            (
                Ok(AstNode::Identifier(id1)),
                Ok(AstNode::NodePattern {
                    variable: Some(v2), ..
                }),
            ) => id1 == v2,
            (
                Ok(AstNode::NodePattern {
                    variable: Some(v1), ..
                }),
                Ok(AstNode::NodePattern {
                    variable: Some(v2), ..
                }),
            ) => v1 == v2,
            _ => false,
        }
    }

    fn resolve_projected_column_name(
        &self,
        arena: &QueryAstArena,
        expr_handle: NodeHandle,
        projections: &[ProjectionItem],
    ) -> String {
        for proj in projections {
            if self.expressions_match(arena, proj.expression, expr_handle) {
                if let Some(alias) = &proj.alias {
                    return alias.clone();
                } else if let Ok(AstNode::PropertyAccess { property, .. }) =
                    arena.get(proj.expression)
                {
                    return property.clone();
                }
            }
        }
        if let Ok(AstNode::PropertyAccess { property, .. }) = arena.get(expr_handle) {
            return property.clone();
        }
        if let Ok(AstNode::Identifier(id)) = arena.get(expr_handle) {
            return id.clone();
        }
        String::new()
    }

    fn get_expr_leaf_name(&self, arena: &QueryAstArena, expr_handle: NodeHandle) -> String {
        match arena.get(expr_handle) {
            Ok(AstNode::PropertyAccess { property, .. }) => property.clone(),
            Ok(AstNode::Identifier(id)) => id.clone(),
            _ => String::new(),
        }
    }

    fn emit_matches_and_where(
        &mut self,
        arena: &QueryAstArena,
        matches: &[NodeHandle],
    ) -> Result<()> {
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
        Ok(())
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
            load_csv: _,
            unwinds,
            matches,
            with_clauses,
            mutations,
            return_clause,
        } = root_node
        {
            if !mutations.is_empty() {
                return Err(Error::UnsupportedFeature {
                    dialect: "sql_pgq".to_string(),
                    feature: "DML mutations (CREATE, MERGE, SET, DELETE) - GRAPH_TABLE is a read-only query operator".to_string(),
                });
            }
            if !unwinds.is_empty() {
                return Err(Error::UnsupportedFeature {
                    dialect: "sql_pgq".to_string(),
                    feature: "UNWIND clauses - GRAPH_TABLE is a read-only query operator"
                        .to_string(),
                });
            }
            if !with_clauses.is_empty() {
                return Err(Error::UnsupportedFeature {
                    dialect: "sql_pgq".to_string(),
                    feature: "WITH clauses - SQL:PGQ GRAPH_TABLE operator does not support Cypher-style intermediate WITH projection pipelines"
                        .to_string(),
                });
            }

            let mut order_by_clause = None;
            let mut limit_clause = None;
            let mut skip_clause = None;
            let mut projections_list: Vec<ProjectionItem> = Vec::new();

            if let Some(ret_handle) = return_clause {
                let ret_node = arena.get(*ret_handle)?;
                if let AstNode::ReturnClause {
                    projections,
                    order_by,
                    skip,
                    limit,
                    ..
                } = ret_node
                {
                    projections_list = projections.clone();
                    order_by_clause = Some(order_by);
                    limit_clause = *limit;
                    skip_clause = *skip;
                }
            }

            let has_aggregates = projections_list.iter().any(|p| p.aggregation.is_some());

            if has_aggregates {
                let mut inner_columns: Vec<(String, String)> = Vec::new();
                let mut outer_select_items: Vec<String> = Vec::new();
                let mut group_by_keys: Vec<String> = Vec::new();

                for (idx, proj) in projections_list.iter().enumerate() {
                    let expr_sql = self.render_expression_to_string(arena, proj.expression)?;
                    let fallback_name = self.get_expr_leaf_name(arena, proj.expression);
                    if let Some(func) = proj.aggregation {
                        let inner_col_name = if let Some((_, existing_alias)) =
                            inner_columns.iter().find(|(e, _)| e == &expr_sql)
                        {
                            existing_alias.clone()
                        } else {
                            let base_name = if !fallback_name.is_empty() {
                                fallback_name
                            } else {
                                format!("col_{idx}")
                            };
                            let col_alias = if inner_columns.iter().any(|(_, a)| a == &base_name) {
                                format!("{base_name}_{idx}")
                            } else {
                                base_name
                            };
                            inner_columns.push((expr_sql, col_alias.clone()));
                            col_alias
                        };

                        let func_call = match func {
                            AggregationFunc::Count => format!("COUNT({inner_col_name})"),
                            AggregationFunc::CountDistinct => {
                                format!("COUNT(DISTINCT {inner_col_name})")
                            }
                            AggregationFunc::Sum => format!("SUM({inner_col_name})"),
                            AggregationFunc::Avg => format!("AVG({inner_col_name})"),
                            AggregationFunc::Min => format!("MIN({inner_col_name})"),
                            AggregationFunc::Max => format!("MAX({inner_col_name})"),
                            AggregationFunc::Collect => format!("ARRAY_AGG({inner_col_name})"),
                        };

                        if let Some(alias) = &proj.alias {
                            outer_select_items.push(format!("{func_call} AS {alias}"));
                        } else {
                            outer_select_items.push(func_call);
                        }
                    } else {
                        let col_alias = proj.alias.clone().unwrap_or_else(|| {
                            if !fallback_name.is_empty() {
                                fallback_name
                            } else {
                                format!("col_{idx}")
                            }
                        });

                        if let Some((_, existing_alias)) =
                            inner_columns.iter().find(|(e, _)| e == &expr_sql)
                        {
                            outer_select_items.push(existing_alias.clone());
                            group_by_keys.push(existing_alias.clone());
                        } else {
                            inner_columns.push((expr_sql, col_alias.clone()));
                            outer_select_items.push(col_alias.clone());
                            group_by_keys.push(col_alias);
                        }
                    }
                }

                self.buffer.push_str("SELECT ");
                for (i, item) in outer_select_items.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.buffer.push_str(item);
                }
                self.buffer.push_str(" FROM GRAPH_TABLE (");
                self.buffer.push_str(&self.graph_name);
                self.buffer.push_str(" MATCH ");

                self.emit_matches_and_where(arena, matches)?;

                self.buffer.push_str(" COLUMNS (");
                for (i, (expr_sql, alias)) in inner_columns.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.buffer.push_str(expr_sql);
                    self.buffer.push_str(" AS ");
                    self.buffer.push_str(alias);
                }
                self.buffer.push_str("))");

                if !group_by_keys.is_empty() {
                    self.buffer.push_str(" GROUP BY ");
                    for (i, key) in group_by_keys.iter().enumerate() {
                        if i > 0 {
                            self.buffer.push_str(", ");
                        }
                        self.buffer.push_str(key);
                    }
                }
            } else {
                self.buffer.push_str("SELECT * FROM GRAPH_TABLE (");
                self.buffer.push_str(&self.graph_name);
                self.buffer.push_str(" MATCH ");

                self.emit_matches_and_where(arena, matches)?;

                self.emit_columns(arena, &projections_list)?;
                self.buffer.push(')');
            }

            if let Some(order_by) = order_by_clause
                && !order_by.is_empty()
            {
                self.buffer.push_str(" ORDER BY ");
                for (i, (order_expr, is_asc)) in order_by.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    let resolved =
                        self.resolve_projected_column_name(arena, *order_expr, &projections_list);
                    if !resolved.is_empty() {
                        self.buffer.push_str(&resolved);
                    } else {
                        self.emit_expression(arena, *order_expr, false)?;
                    }
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
