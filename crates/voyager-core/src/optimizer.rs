//! Rule-based AST Query Optimizer for Voyager OGM.
//!
//! Provides multi-level rule-based optimizations over [`QueryAstArena`]:
//! - **Constant Folding**: Pre-evaluates arithmetic expressions (`10 + 20` -> `30`), float operations, relational comparisons (`10 > 5` -> `true`), boolean identities, and nested partial reassociation (`(x + 10) + 20` -> `x + 30`) at compile time.
//! - **Predicate Pushdown**: Hoists single-node equality filters from `WHERE` clauses into inline node property patterns (`(p:Person {city: $p0})`) to enable database index seeks before path traversal.
//! - **Boolean Simplification**: Flattens conjunction chains (`AND`) and strips redundant boolean constants.
//! - **Dead Variable Pruning**: In aggressive mode, eliminates unreferenced intermediate internal aliases.

use crate::ast::{AstNode, BinaryOp, LiteralValue, NodeHandle, QueryAstArena, UnaryOp};
use crate::error::Result;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Optimization level for the rule-based AST query optimizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum OptimizationLevel {
    /// No optimization passes applied. The AST is preserved exactly as constructed.
    None,
    /// Standard rule-based optimizations (Predicate pushdown, Boolean simplification).
    #[default]
    Standard,
    /// Aggressive optimizations (Standard passes + Dead variable pruning).
    Aggressive,
}

impl OptimizationLevel {
    /// Parses an optimization level from string ("none", "standard", "aggressive").
    pub fn from_str_opt(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "none" | "0" | "off" | "false" => Self::None,
            "aggressive" | "2" | "all" | "high" => Self::Aggressive,
            _ => Self::Standard,
        }
    }
}

/// Rule-based query optimizer.
#[derive(Debug, Clone)]
pub struct AstOptimizer {
    level: OptimizationLevel,
}

impl Default for AstOptimizer {
    fn default() -> Self {
        Self::new(OptimizationLevel::Standard)
    }
}

impl AstOptimizer {
    /// Creates a new optimizer instance with the specified optimization level.
    pub const fn new(level: OptimizationLevel) -> Self {
        Self { level }
    }

    /// Returns the current optimization level.
    pub const fn level(&self) -> OptimizationLevel {
        self.level
    }

    /// Optimizes an AST rooted at `root` in the given `arena`.
    ///
    /// Returns the handle to the optimized root node.
    pub fn optimize(&self, arena: &mut QueryAstArena, root: NodeHandle) -> Result<NodeHandle> {
        if self.level == OptimizationLevel::None {
            return Ok(root);
        }

        // Pass 1: Constant Folding & Expression Simplification
        self.fold_all_expressions(arena, root)?;

        // Pass 2: Predicate Pushdown & Conjunction Simplification
        self.optimize_statement(arena, root)?;

        // Pass 3: Dead Variable Pruning (in Aggressive mode)
        if self.level == OptimizationLevel::Aggressive {
            self.prune_dead_variables(arena, root)?;
        }

        Ok(root)
    }

    fn optimize_statement(&self, arena: &mut QueryAstArena, root: NodeHandle) -> Result<()> {
        let (match_handles, mutation_handles) = {
            let root_node = arena.get(root)?;
            if let AstNode::QueryStatement {
                matches, mutations, ..
            } = root_node
            {
                (matches.clone(), mutations.clone())
            } else {
                (Vec::new(), Vec::new())
            }
        };

        for (idx, &match_handle) in match_handles.iter().enumerate() {
            self.optimize_match_clause(arena, match_handle, &match_handles[..=idx])?;
        }

        for mut_handle in mutation_handles {
            self.optimize_mutation_clause(arena, mut_handle)?;
        }

        Ok(())
    }

    fn optimize_match_clause(
        &self,
        arena: &mut QueryAstArena,
        match_handle: NodeHandle,
        candidate_matches: &[NodeHandle],
    ) -> Result<()> {
        let (paths, where_clause, is_optional) = {
            let match_node = arena.get(match_handle)?;
            if let AstNode::MatchClause {
                paths,
                where_clause,
                optional,
                ..
            } = match_node
            {
                (paths.clone(), *where_clause, *optional)
            } else {
                return Ok(());
            }
        };

        if let Some(where_handle) = where_clause {
            let mut conjuncts = Vec::new();
            self.collect_conjuncts(arena, where_handle, &mut conjuncts)?;

            let mut remaining_conjuncts = Vec::new();

            for &conjunct_handle in &conjuncts {
                if let Some(target_var) =
                    self.extract_single_node_equality_var(arena, conjunct_handle)?
                {
                    // 1. Try to hoist this equality into a matching node pattern in this match clause
                    let mut hoisted = false;
                    for &path_handle in &paths {
                        if self.hoist_predicate_to_path(
                            arena,
                            path_handle,
                            &target_var,
                            conjunct_handle,
                        )? {
                            hoisted = true;
                            break;
                        }
                    }

                    // 2. If not found in current match clause and this is NOT an OPTIONAL MATCH,
                    // check preceding non-optional match clauses in the query statement
                    if !hoisted && !is_optional {
                        for &prev_match_h in candidate_matches.iter().rev().skip(1) {
                            let (prev_paths, prev_optional) = {
                                let prev_node = arena.get(prev_match_h)?;
                                if let AstNode::MatchClause {
                                    paths, optional, ..
                                } = prev_node
                                {
                                    (paths.clone(), *optional)
                                } else {
                                    (Vec::new(), true)
                                }
                            };

                            if !prev_optional {
                                for &prev_path in &prev_paths {
                                    if self.hoist_predicate_to_path(
                                        arena,
                                        prev_path,
                                        &target_var,
                                        conjunct_handle,
                                    )? {
                                        hoisted = true;
                                        break;
                                    }
                                }
                                if hoisted {
                                    break;
                                }
                            }
                        }
                    }

                    if !hoisted {
                        remaining_conjuncts.push(conjunct_handle);
                    }
                } else {
                    remaining_conjuncts.push(conjunct_handle);
                }
            }

            // Rebuild the where clause with remaining conjuncts
            let new_where = self.build_conjunction_tree(arena, &remaining_conjuncts);
            let match_node_mut = arena.get_mut(match_handle)?;
            if let AstNode::MatchClause {
                where_clause: wh, ..
            } = match_node_mut
            {
                *wh = new_where;
            }
        }

        Ok(())
    }

    /// Pass 1: Traverses the statement and recursively folds constant sub-expressions.
    fn fold_all_expressions(&self, arena: &mut QueryAstArena, root: NodeHandle) -> Result<()> {
        let root_node = arena.get(root)?.clone();
        if let AstNode::QueryStatement {
            load_csv,
            unwinds,
            matches,
            with_clauses,
            linear_clauses,
            mutations,
            return_clause,
            ..
        } = root_node
        {
            if let Some(load_h) = load_csv {
                let load_node = arena.get(load_h)?.clone();
                if let AstNode::LoadCsvClause { url, .. } = load_node {
                    let folded_url = self.fold_expression(arena, url)?;
                    if let AstNode::LoadCsvClause { url: u, .. } = arena.get_mut(load_h)? {
                        *u = folded_url;
                    }
                }
            }

            for unwind_h in unwinds {
                let unwind_node = arena.get(unwind_h)?.clone();
                if let AstNode::UnwindClause { expression, .. } = unwind_node {
                    let folded_expr = self.fold_expression(arena, expression)?;
                    if let AstNode::UnwindClause { expression: e, .. } = arena.get_mut(unwind_h)? {
                        *e = folded_expr;
                    }
                }
            }

            for match_h in matches {
                self.fold_match_clause(arena, match_h)?;
            }

            for with_h in with_clauses {
                self.fold_with_clause(arena, with_h)?;
            }

            for &lin_h in &linear_clauses {
                let lin_node = arena.get(lin_h)?.clone();
                match lin_node {
                    AstNode::LetClause { expression, .. } => {
                        let folded_expr = self.fold_expression(arena, expression)?;
                        if let AstNode::LetClause { expression: e, .. } = arena.get_mut(lin_h)? {
                            *e = folded_expr;
                        }
                    }
                    AstNode::FilterClause { predicate } => {
                        let folded_pred = self.fold_expression(arena, predicate)?;
                        if let AstNode::FilterClause { predicate: p } = arena.get_mut(lin_h)? {
                            *p = folded_pred;
                        }
                    }
                    _ => {}
                }
            }

            for mut_h in mutations {
                self.optimize_mutation_clause(arena, mut_h)?;
            }

            if let Some(ret_h) = return_clause {
                self.fold_return_clause(arena, ret_h)?;
            }
        }
        Ok(())
    }

    fn fold_match_clause(&self, arena: &mut QueryAstArena, match_handle: NodeHandle) -> Result<()> {
        let match_node = arena.get(match_handle)?.clone();
        if let AstNode::MatchClause {
            paths,
            where_clause,
            ..
        } = match_node
        {
            for path_h in paths {
                self.fold_path_predicates(arena, path_h)?;
            }
            if let Some(where_h) = where_clause {
                let where_node = arena.get(where_h)?.clone();
                if let AstNode::WhereClause { root_predicate } = where_node {
                    let folded_pred = self.fold_expression(arena, root_predicate)?;
                    if let AstNode::WhereClause { root_predicate: rp } = arena.get_mut(where_h)? {
                        *rp = folded_pred;
                    }
                }
            }
        }
        Ok(())
    }

    fn fold_with_clause(&self, arena: &mut QueryAstArena, with_handle: NodeHandle) -> Result<()> {
        let with_node = arena.get(with_handle)?.clone();
        if let AstNode::WithClause {
            projections,
            order_by,
            where_clause,
            ..
        } = with_node
        {
            let mut folded_projections = Vec::with_capacity(projections.len());
            for mut item in projections {
                item.expression = self.fold_expression(arena, item.expression)?;
                folded_projections.push(item);
            }

            let mut folded_order_by = Vec::with_capacity(order_by.len());
            for (expr_h, asc) in order_by {
                let folded_expr = self.fold_expression(arena, expr_h)?;
                folded_order_by.push((folded_expr, asc));
            }

            if let AstNode::WithClause {
                projections: proj,
                order_by: ord,
                ..
            } = arena.get_mut(with_handle)?
            {
                *proj = folded_projections;
                *ord = folded_order_by;
            }

            if let Some(where_h) = where_clause {
                let where_node = arena.get(where_h)?.clone();
                if let AstNode::WhereClause { root_predicate } = where_node {
                    let folded_pred = self.fold_expression(arena, root_predicate)?;
                    if let AstNode::WhereClause { root_predicate: rp } = arena.get_mut(where_h)? {
                        *rp = folded_pred;
                    }
                }
            }
        }
        Ok(())
    }

    fn fold_return_clause(&self, arena: &mut QueryAstArena, ret_h: NodeHandle) -> Result<()> {
        let ret_node = arena.get(ret_h)?.clone();
        if let AstNode::ReturnClause {
            projections,
            order_by,
            ..
        } = ret_node
        {
            let mut folded_projections = Vec::with_capacity(projections.len());
            for mut item in projections {
                item.expression = self.fold_expression(arena, item.expression)?;
                folded_projections.push(item);
            }

            let mut folded_order_by = Vec::with_capacity(order_by.len());
            for (expr_h, asc) in order_by {
                let folded_expr = self.fold_expression(arena, expr_h)?;
                folded_order_by.push((folded_expr, asc));
            }

            if let AstNode::ReturnClause {
                projections: proj,
                order_by: ord,
                ..
            } = arena.get_mut(ret_h)?
            {
                *proj = folded_projections;
                *ord = folded_order_by;
            }
        }
        Ok(())
    }

    fn fold_path_predicates(&self, arena: &mut QueryAstArena, path_h: NodeHandle) -> Result<()> {
        let path_node = arena.get(path_h)?.clone();
        match path_node {
            AstNode::NodePattern { predicates, .. } => {
                let mut folded_preds = Vec::with_capacity(predicates.len());
                for pred_h in predicates {
                    folded_preds.push(self.fold_expression(arena, pred_h)?);
                }
                if let AstNode::NodePattern { predicates: p, .. } = arena.get_mut(path_h)? {
                    *p = folded_preds;
                }
            }
            AstNode::PathChain {
                start_node, edges, ..
            } => {
                self.fold_path_predicates(arena, start_node)?;
                for edge_h in edges {
                    let edge_node = arena.get(edge_h)?.clone();
                    if let AstNode::EdgePattern {
                        predicates,
                        target_node,
                        ..
                    } = edge_node
                    {
                        let mut folded_preds = Vec::with_capacity(predicates.len());
                        for pred_h in predicates {
                            folded_preds.push(self.fold_expression(arena, pred_h)?);
                        }
                        if let AstNode::EdgePattern { predicates: p, .. } = arena.get_mut(edge_h)? {
                            *p = folded_preds;
                        }
                        self.fold_path_predicates(arena, target_node)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Recursively evaluates and folds constant sub-expressions into literal nodes.
    pub fn fold_expression(
        &self,
        arena: &mut QueryAstArena,
        handle: NodeHandle,
    ) -> Result<NodeHandle> {
        if handle.is_null() {
            return Ok(handle);
        }

        let node = arena.get(handle)?.clone();
        match node {
            AstNode::BinaryExpression { left, op, right } => {
                let left_folded = self.fold_expression(arena, left)?;
                let right_folded = self.fold_expression(arena, right)?;
                self.fold_binary_op(arena, left_folded, op, right_folded)
            }
            AstNode::UnaryExpression { op, operand } => {
                let operand_folded = self.fold_expression(arena, operand)?;
                self.fold_unary_op(arena, op, operand_folded)
            }
            AstNode::FunctionCall { name, arguments } => {
                let mut folded_args = Vec::with_capacity(arguments.len());
                for arg in arguments {
                    folded_args.push(self.fold_expression(arena, arg)?);
                }
                Ok(arena.alloc(AstNode::FunctionCall {
                    name,
                    arguments: folded_args,
                }))
            }
            AstNode::CaseExpression {
                operand,
                when_then_branches,
                else_branch,
            } => self.fold_case_expression(arena, operand, when_then_branches, else_branch),
            AstNode::ListComprehension {
                variable,
                list_expression,
                where_filter,
                map_expression,
            } => {
                let list_folded = self.fold_expression(arena, list_expression)?;
                let filter_folded = where_filter
                    .map(|f| self.fold_expression(arena, f))
                    .transpose()?;
                let map_folded = map_expression
                    .map(|m| self.fold_expression(arena, m))
                    .transpose()?;
                Ok(arena.alloc(AstNode::ListComprehension {
                    variable,
                    list_expression: list_folded,
                    where_filter: filter_folded,
                    map_expression: map_folded,
                }))
            }
            AstNode::PatternComprehension {
                path,
                where_filter,
                projection,
            } => {
                let filter_folded = where_filter
                    .map(|f| self.fold_expression(arena, f))
                    .transpose()?;
                let proj_folded = self.fold_expression(arena, projection)?;
                Ok(arena.alloc(AstNode::PatternComprehension {
                    path,
                    where_filter: filter_folded,
                    projection: proj_folded,
                }))
            }
            AstNode::ExistsSubquery { subquery } => {
                self.fold_subquery(arena, subquery)?;
                Ok(handle)
            }
            AstNode::CountSubquery { subquery } => {
                self.fold_subquery(arena, subquery)?;
                Ok(handle)
            }
            AstNode::ListLiteral(items) => {
                let mut folded_items = Vec::with_capacity(items.len());
                for item in items {
                    folded_items.push(self.fold_expression(arena, item)?);
                }
                Ok(arena.alloc(AstNode::ListLiteral(folded_items)))
            }
            _ => Ok(handle),
        }
    }

    #[allow(clippy::collapsible_if)]
    fn fold_binary_op(
        &self,
        arena: &mut QueryAstArena,
        left: NodeHandle,
        op: BinaryOp,
        right: NodeHandle,
    ) -> Result<NodeHandle> {
        let left_node = arena.get(left)?.clone();
        let right_node = arena.get(right)?.clone();

        // 1. Both are literals -> direct evaluation
        if let (AstNode::Literal(l_val), AstNode::Literal(r_val)) = (&left_node, &right_node) {
            if let Some(folded) = self.eval_binary_literals(l_val, op, r_val) {
                return Ok(arena.alloc(AstNode::Literal(folded)));
            }
        }

        // 2. Boolean identity and short-circuit laws
        match op {
            BinaryOp::And => {
                if let AstNode::Literal(LiteralValue::Bool(false)) = left_node {
                    return Ok(left); // false AND expr -> false
                }
                if let AstNode::Literal(LiteralValue::Bool(false)) = right_node {
                    return Ok(right); // expr AND false -> false
                }
                if let AstNode::Literal(LiteralValue::Bool(true)) = left_node {
                    return Ok(right); // true AND expr -> expr
                }
                if let AstNode::Literal(LiteralValue::Bool(true)) = right_node {
                    return Ok(left); // expr AND true -> expr
                }
            }
            BinaryOp::Or => {
                if let AstNode::Literal(LiteralValue::Bool(true)) = left_node {
                    return Ok(left); // true OR expr -> true
                }
                if let AstNode::Literal(LiteralValue::Bool(true)) = right_node {
                    return Ok(right); // expr OR true -> true
                }
                if let AstNode::Literal(LiteralValue::Bool(false)) = left_node {
                    return Ok(right); // false OR expr -> expr
                }
                if let AstNode::Literal(LiteralValue::Bool(false)) = right_node {
                    return Ok(left); // expr OR false -> expr
                }
            }
            // 3. Arithmetic identities
            BinaryOp::Add => {
                if let AstNode::Literal(LiteralValue::Int64(0)) = right_node {
                    return Ok(left); // x + 0 -> x
                }
                if let AstNode::Literal(LiteralValue::Int64(0)) = left_node {
                    return Ok(right); // 0 + x -> x
                }
            }
            BinaryOp::Sub => {
                if let AstNode::Literal(LiteralValue::Int64(0)) = right_node {
                    return Ok(left); // x - 0 -> x
                }
            }
            BinaryOp::Mul => {
                if let AstNode::Literal(LiteralValue::Int64(1)) = right_node {
                    return Ok(left); // x * 1 -> x
                }
                if let AstNode::Literal(LiteralValue::Int64(1)) = left_node {
                    return Ok(right); // 1 * x -> x
                }
                // Note: Do NOT fold `x * 0 -> 0` when `x` is non-literal, because in SQL and Cypher
                // Three-Valued Logic (3VL), `NULL * 0` yields `NULL`, not `0`. Folding it would
                // incorrectly include nodes/rows with NULL values in WHERE filters.
            }
            BinaryOp::Div => {
                if let AstNode::Literal(LiteralValue::Int64(1)) = right_node {
                    return Ok(left); // x / 1 -> x
                }
            }
            _ => {}
        }

        // 4. Associative reassociation for nested expressions (e.g. (x + 10) + 20 -> x + 30)
        if let AstNode::Literal(LiteralValue::Int64(r_const)) = right_node {
            match op {
                BinaryOp::Add => {
                    if let AstNode::BinaryExpression {
                        left: inner_l,
                        op: BinaryOp::Add,
                        right: inner_r,
                    } = left_node
                    {
                        let inner_r_node = arena.get(inner_r)?.clone();
                        if let AstNode::Literal(LiteralValue::Int64(inner_c)) = inner_r_node {
                            if let Some(sum) = inner_c.checked_add(r_const) {
                                let new_lit =
                                    arena.alloc(AstNode::Literal(LiteralValue::Int64(sum)));
                                return Ok(arena.alloc(AstNode::BinaryExpression {
                                    left: inner_l,
                                    op: BinaryOp::Add,
                                    right: new_lit,
                                }));
                            }
                        }
                        let inner_l_node = arena.get(inner_l)?.clone();
                        if let AstNode::Literal(LiteralValue::Int64(inner_c)) = inner_l_node {
                            if let Some(sum) = inner_c.checked_add(r_const) {
                                let new_lit =
                                    arena.alloc(AstNode::Literal(LiteralValue::Int64(sum)));
                                return Ok(arena.alloc(AstNode::BinaryExpression {
                                    left: inner_r,
                                    op: BinaryOp::Add,
                                    right: new_lit,
                                }));
                            }
                        }
                    }
                }
                BinaryOp::Sub => {
                    if let AstNode::BinaryExpression {
                        left: inner_l,
                        op: BinaryOp::Sub,
                        right: inner_r,
                    } = left_node
                    {
                        let inner_r_node = arena.get(inner_r)?.clone();
                        if let AstNode::Literal(LiteralValue::Int64(inner_c)) = inner_r_node {
                            if let Some(sum) = inner_c.checked_add(r_const) {
                                let new_lit =
                                    arena.alloc(AstNode::Literal(LiteralValue::Int64(sum)));
                                return Ok(arena.alloc(AstNode::BinaryExpression {
                                    left: inner_l,
                                    op: BinaryOp::Sub,
                                    right: new_lit,
                                }));
                            }
                        }
                    }
                }
                BinaryOp::Mul => {
                    if let AstNode::BinaryExpression {
                        left: inner_l,
                        op: BinaryOp::Mul,
                        right: inner_r,
                    } = left_node
                    {
                        let inner_r_node = arena.get(inner_r)?.clone();
                        if let AstNode::Literal(LiteralValue::Int64(inner_c)) = inner_r_node {
                            if let Some(prod) = inner_c.checked_mul(r_const) {
                                let new_lit =
                                    arena.alloc(AstNode::Literal(LiteralValue::Int64(prod)));
                                return Ok(arena.alloc(AstNode::BinaryExpression {
                                    left: inner_l,
                                    op: BinaryOp::Mul,
                                    right: new_lit,
                                }));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(arena.alloc(AstNode::BinaryExpression { left, op, right }))
    }

    fn eval_binary_literals(
        &self,
        left: &LiteralValue,
        op: BinaryOp,
        right: &LiteralValue,
    ) -> Option<LiteralValue> {
        match (left, op, right) {
            // --- Integer Arithmetic ---
            (LiteralValue::Int64(a), BinaryOp::Add, LiteralValue::Int64(b)) => {
                a.checked_add(*b).map(LiteralValue::Int64)
            }
            (LiteralValue::Int64(a), BinaryOp::Sub, LiteralValue::Int64(b)) => {
                a.checked_sub(*b).map(LiteralValue::Int64)
            }
            (LiteralValue::Int64(a), BinaryOp::Mul, LiteralValue::Int64(b)) => {
                a.checked_mul(*b).map(LiteralValue::Int64)
            }
            (LiteralValue::Int64(a), BinaryOp::Div, LiteralValue::Int64(b)) => {
                if *b != 0 {
                    a.checked_div(*b).map(LiteralValue::Int64)
                } else {
                    None // Do not divide by zero!
                }
            }
            (LiteralValue::Int64(a), BinaryOp::Mod, LiteralValue::Int64(b)) => {
                if *b != 0 {
                    a.checked_rem(*b).map(LiteralValue::Int64)
                } else {
                    None // Do not modulo by zero!
                }
            }

            // --- Integer Relational ---
            (LiteralValue::Int64(a), BinaryOp::Eq, LiteralValue::Int64(b)) => {
                Some(LiteralValue::Bool(a == b))
            }
            (LiteralValue::Int64(a), BinaryOp::Neq, LiteralValue::Int64(b)) => {
                Some(LiteralValue::Bool(a != b))
            }
            (LiteralValue::Int64(a), BinaryOp::Lt, LiteralValue::Int64(b)) => {
                Some(LiteralValue::Bool(a < b))
            }
            (LiteralValue::Int64(a), BinaryOp::Lte, LiteralValue::Int64(b)) => {
                Some(LiteralValue::Bool(a <= b))
            }
            (LiteralValue::Int64(a), BinaryOp::Gt, LiteralValue::Int64(b)) => {
                Some(LiteralValue::Bool(a > b))
            }
            (LiteralValue::Int64(a), BinaryOp::Gte, LiteralValue::Int64(b)) => {
                Some(LiteralValue::Bool(a >= b))
            }

            // --- Float Arithmetic ---
            (LiteralValue::Float64(a), BinaryOp::Add, LiteralValue::Float64(b)) => {
                let res = a + b;
                if res.is_finite() {
                    Some(LiteralValue::Float64(res))
                } else {
                    None
                }
            }
            (LiteralValue::Float64(a), BinaryOp::Sub, LiteralValue::Float64(b)) => {
                let res = a - b;
                if res.is_finite() {
                    Some(LiteralValue::Float64(res))
                } else {
                    None
                }
            }
            (LiteralValue::Float64(a), BinaryOp::Mul, LiteralValue::Float64(b)) => {
                let res = a * b;
                if res.is_finite() {
                    Some(LiteralValue::Float64(res))
                } else {
                    None
                }
            }
            (LiteralValue::Float64(a), BinaryOp::Div, LiteralValue::Float64(b)) => {
                if *b != 0.0 {
                    let res = a / b;
                    if res.is_finite() {
                        Some(LiteralValue::Float64(res))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            (LiteralValue::Float64(a), BinaryOp::Mod, LiteralValue::Float64(b)) => {
                if *b != 0.0 {
                    let res = a % b;
                    if res.is_finite() {
                        Some(LiteralValue::Float64(res))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }

            // --- Float Relational ---
            (LiteralValue::Float64(a), BinaryOp::Eq, LiteralValue::Float64(b)) => {
                Some(LiteralValue::Bool(a == b))
            }
            (LiteralValue::Float64(a), BinaryOp::Neq, LiteralValue::Float64(b)) => {
                Some(LiteralValue::Bool(a != b))
            }
            (LiteralValue::Float64(a), BinaryOp::Lt, LiteralValue::Float64(b)) => {
                Some(LiteralValue::Bool(a < b))
            }
            (LiteralValue::Float64(a), BinaryOp::Lte, LiteralValue::Float64(b)) => {
                Some(LiteralValue::Bool(a <= b))
            }
            (LiteralValue::Float64(a), BinaryOp::Gt, LiteralValue::Float64(b)) => {
                Some(LiteralValue::Bool(a > b))
            }
            (LiteralValue::Float64(a), BinaryOp::Gte, LiteralValue::Float64(b)) => {
                Some(LiteralValue::Bool(a >= b))
            }

            // --- Mixed Integer & Float Promotion ---
            (LiteralValue::Int64(a), op, LiteralValue::Float64(b)) => self.eval_binary_literals(
                &LiteralValue::Float64(*a as f64),
                op,
                &LiteralValue::Float64(*b),
            ),
            (LiteralValue::Float64(a), op, LiteralValue::Int64(b)) => self.eval_binary_literals(
                &LiteralValue::Float64(*a),
                op,
                &LiteralValue::Float64(*b as f64),
            ),

            // --- Boolean Operations ---
            (LiteralValue::Bool(a), BinaryOp::And, LiteralValue::Bool(b)) => {
                Some(LiteralValue::Bool(*a && *b))
            }
            (LiteralValue::Bool(a), BinaryOp::Or, LiteralValue::Bool(b)) => {
                Some(LiteralValue::Bool(*a || *b))
            }
            (LiteralValue::Bool(a), BinaryOp::Xor, LiteralValue::Bool(b)) => {
                Some(LiteralValue::Bool(*a ^ *b))
            }
            (LiteralValue::Bool(a), BinaryOp::Eq, LiteralValue::Bool(b)) => {
                Some(LiteralValue::Bool(a == b))
            }
            (LiteralValue::Bool(a), BinaryOp::Neq, LiteralValue::Bool(b)) => {
                Some(LiteralValue::Bool(a != b))
            }

            // --- String Operations ---
            (
                LiteralValue::String(a),
                BinaryOp::Add | BinaryOp::Concat,
                LiteralValue::String(b),
            ) => Some(LiteralValue::String(format!("{a}{b}"))),
            (LiteralValue::String(a), BinaryOp::Eq, LiteralValue::String(b)) => {
                Some(LiteralValue::Bool(a == b))
            }
            (LiteralValue::String(a), BinaryOp::Neq, LiteralValue::String(b)) => {
                Some(LiteralValue::Bool(a != b))
            }
            (LiteralValue::String(a), BinaryOp::Contains, LiteralValue::String(b)) => {
                Some(LiteralValue::Bool(a.contains(b.as_str())))
            }
            (LiteralValue::String(a), BinaryOp::StartsWith, LiteralValue::String(b)) => {
                Some(LiteralValue::Bool(a.starts_with(b.as_str())))
            }
            (LiteralValue::String(a), BinaryOp::EndsWith, LiteralValue::String(b)) => {
                Some(LiteralValue::Bool(a.ends_with(b.as_str())))
            }

            _ => None,
        }
    }

    fn fold_unary_op(
        &self,
        arena: &mut QueryAstArena,
        op: UnaryOp,
        operand: NodeHandle,
    ) -> Result<NodeHandle> {
        let operand_node = arena.get(operand)?.clone();
        match op {
            UnaryOp::Not => {
                if let AstNode::Literal(LiteralValue::Bool(b)) = operand_node {
                    return Ok(arena.alloc(AstNode::Literal(LiteralValue::Bool(!b))));
                }
                // Double negation: NOT(NOT(x)) -> x
                if let AstNode::UnaryExpression {
                    op: UnaryOp::Not,
                    operand: inner,
                } = operand_node
                {
                    return Ok(inner);
                }
            }
            UnaryOp::Neg => {
                if let AstNode::Literal(LiteralValue::Int64(i)) = operand_node {
                    if let Some(neg_i) = i.checked_neg() {
                        return Ok(arena.alloc(AstNode::Literal(LiteralValue::Int64(neg_i))));
                    }
                } else if let AstNode::Literal(LiteralValue::Float64(f)) = operand_node {
                    let neg_f = -f;
                    if neg_f.is_finite() {
                        return Ok(arena.alloc(AstNode::Literal(LiteralValue::Float64(neg_f))));
                    }
                }
                // Double negation: -(-x) -> x
                if let AstNode::UnaryExpression {
                    op: UnaryOp::Neg,
                    operand: inner,
                } = operand_node
                {
                    return Ok(inner);
                }
            }
            UnaryOp::IsNull => {
                if let AstNode::Literal(LiteralValue::Null) = operand_node {
                    return Ok(arena.alloc(AstNode::Literal(LiteralValue::Bool(true))));
                }
            }
            UnaryOp::IsNotNull => {
                if let AstNode::Literal(LiteralValue::Null) = operand_node {
                    return Ok(arena.alloc(AstNode::Literal(LiteralValue::Bool(false))));
                }
            }
        }
        Ok(arena.alloc(AstNode::UnaryExpression { op, operand }))
    }

    fn fold_case_expression(
        &self,
        arena: &mut QueryAstArena,
        operand: Option<NodeHandle>,
        when_then_branches: Vec<(NodeHandle, NodeHandle)>,
        else_branch: Option<NodeHandle>,
    ) -> Result<NodeHandle> {
        let folded_operand = operand
            .map(|op_h| self.fold_expression(arena, op_h))
            .transpose()?;

        let mut folded_branches = Vec::new();
        let mut constant_hit = None;

        for (when_h, then_h) in when_then_branches {
            let folded_when = self.fold_expression(arena, when_h)?;
            let folded_then = self.fold_expression(arena, then_h)?;

            if folded_operand.is_none() {
                let when_node = arena.get(folded_when)?.clone();
                if let AstNode::Literal(LiteralValue::Bool(true)) = when_node {
                    // Constant true branch reached - subsequent branches are unreachable
                    constant_hit = Some(folded_then);
                    break;
                } else if let AstNode::Literal(LiteralValue::Bool(false)) = when_node {
                    // Constant false condition - branch can never be taken, skip it
                    continue;
                }
            }

            folded_branches.push((folded_when, folded_then));
        }

        if let Some(hit) = constant_hit {
            return Ok(hit);
        }

        let folded_else = else_branch
            .map(|e_h| self.fold_expression(arena, e_h))
            .transpose()?;

        if folded_branches.is_empty() {
            if let Some(e) = folded_else {
                return Ok(e);
            }
            return Ok(arena.alloc(AstNode::Literal(LiteralValue::Null)));
        }

        Ok(arena.alloc(AstNode::CaseExpression {
            operand: folded_operand,
            when_then_branches: folded_branches,
            else_branch: folded_else,
        }))
    }

    fn fold_subquery(&self, arena: &mut QueryAstArena, subquery_h: NodeHandle) -> Result<()> {
        let node = arena.get(subquery_h)?.clone();
        match node {
            AstNode::QueryStatement { .. } => {
                self.fold_all_expressions(arena, subquery_h)?;
            }
            AstNode::MatchClause { .. } => {
                self.fold_match_clause(arena, subquery_h)?;
            }
            AstNode::PathChain { .. } | AstNode::NodePattern { .. } => {
                self.fold_path_predicates(arena, subquery_h)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn optimize_mutation_clause(
        &self,
        arena: &mut QueryAstArena,
        mut_handle: NodeHandle,
    ) -> Result<()> {
        let mut_node = arena.get(mut_handle)?.clone();
        match mut_node {
            AstNode::SetClause { items } => {
                for item_h in items {
                    let item_node = arena.get(item_h)?.clone();
                    if let AstNode::SetItem { value, .. } = item_node {
                        let folded_val = self.fold_expression(arena, value)?;
                        if let AstNode::SetItem { value: v, .. } = arena.get_mut(item_h)? {
                            *v = folded_val;
                        }
                    }
                }
            }
            AstNode::MergeClause {
                path,
                on_create_set,
                on_match_set,
            } => {
                self.fold_path_predicates(arena, path)?;
                for item_h in on_create_set {
                    let item_node = arena.get(item_h)?.clone();
                    if let AstNode::SetItem { value, .. } = item_node {
                        let folded_val = self.fold_expression(arena, value)?;
                        if let AstNode::SetItem { value: v, .. } = arena.get_mut(item_h)? {
                            *v = folded_val;
                        }
                    }
                }
                for item_h in on_match_set {
                    let item_node = arena.get(item_h)?.clone();
                    if let AstNode::SetItem { value, .. } = item_node {
                        let folded_val = self.fold_expression(arena, value)?;
                        if let AstNode::SetItem { value: v, .. } = arena.get_mut(item_h)? {
                            *v = folded_val;
                        }
                    }
                }
            }
            AstNode::CreateClause { paths } => {
                for p_h in paths {
                    self.fold_path_predicates(arena, p_h)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Recursively flattens binary `AND` conjunctions into a flat slice of handles.
    fn collect_conjuncts(
        &self,
        arena: &QueryAstArena,
        handle: NodeHandle,
        conjuncts: &mut Vec<NodeHandle>,
    ) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::BinaryExpression {
            left,
            op: BinaryOp::And,
            right,
        } = node
        {
            self.collect_conjuncts(arena, *left, conjuncts)?;
            self.collect_conjuncts(arena, *right, conjuncts)?;
        } else if let AstNode::WhereClause { root_predicate } = node {
            self.collect_conjuncts(arena, *root_predicate, conjuncts)?;
        } else {
            // Check for trivial TRUE constant which can be skipped
            if let AstNode::Literal(LiteralValue::Bool(true)) = node {
                return Ok(());
            }
            conjuncts.push(handle);
        }
        Ok(())
    }

    /// Checks if an expression is a single-node property equality (`p.prop = val` or `val = p.prop`).
    /// Returns `Some(var_name)` if it can be safely hoisted to node `var_name`.
    fn extract_single_node_equality_var(
        &self,
        arena: &QueryAstArena,
        handle: NodeHandle,
    ) -> Result<Option<String>> {
        let node = arena.get(handle)?;
        if let AstNode::BinaryExpression {
            left,
            op: BinaryOp::Eq,
            right,
        } = node
        {
            let left_node = arena.get(*left)?;
            let right_node = arena.get(*right)?;

            // Case A: Left is `PropertyAccess`, right is constant/literal/parameter expression
            if let AstNode::PropertyAccess { target, .. } = left_node
                && self.is_constant_expression(arena, *right)?
            {
                return self.resolve_variable_name(arena, *target);
            }

            // Case B: Right is `PropertyAccess`, left is constant/literal/parameter expression
            if let AstNode::PropertyAccess { target, .. } = right_node
                && self.is_constant_expression(arena, *left)?
            {
                return self.resolve_variable_name(arena, *target);
            }
        }
        Ok(None)
    }

    /// Returns true if the expression is a constant expression (literals, parameters,
    /// or deterministic functions/operations over literals and parameters) with no graph variable references.
    fn is_constant_expression(&self, arena: &QueryAstArena, handle: NodeHandle) -> Result<bool> {
        let node = arena.get(handle)?;
        match node {
            AstNode::Literal(_) | AstNode::Parameter(_) => Ok(true),
            AstNode::FunctionCall { arguments, .. } => {
                for &arg in arguments {
                    if !self.is_constant_expression(arena, arg)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            AstNode::UnaryExpression { operand, .. } => {
                self.is_constant_expression(arena, *operand)
            }
            AstNode::ListLiteral(items) => {
                for &item in items {
                    if !self.is_constant_expression(arena, item)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn resolve_variable_name(
        &self,
        arena: &QueryAstArena,
        handle: NodeHandle,
    ) -> Result<Option<String>> {
        let node = arena.get(handle)?;
        match node {
            AstNode::Identifier(ident) => Ok(Some(ident.clone())),
            AstNode::NodePattern { variable, .. } => Ok(variable.clone()),
            _ => Ok(None),
        }
    }

    /// Hoists a predicate into a node pattern with the given variable name in a path.
    fn hoist_predicate_to_path(
        &self,
        arena: &mut QueryAstArena,
        path_handle: NodeHandle,
        target_var: &str,
        pred_handle: NodeHandle,
    ) -> Result<bool> {
        let (is_match, is_chain, start_h, edge_handles) = {
            let path_node = arena.get(path_handle)?;
            match path_node {
                AstNode::NodePattern { variable, .. } => (
                    variable.as_deref() == Some(target_var),
                    false,
                    NodeHandle::NULL,
                    Vec::new(),
                ),
                AstNode::PathChain {
                    start_node, edges, ..
                } => (false, true, *start_node, edges.clone()),
                _ => (false, false, NodeHandle::NULL, Vec::new()),
            }
        };

        if is_match {
            let node_mut = arena.get_mut(path_handle)?;
            if let AstNode::NodePattern { predicates, .. } = node_mut {
                predicates.push(pred_handle);
                return Ok(true);
            }
        }

        if is_chain {
            if self.hoist_predicate_to_path(arena, start_h, target_var, pred_handle)? {
                return Ok(true);
            }

            for edge_h in edge_handles {
                let target_h = {
                    let edge_node = arena.get(edge_h)?;
                    if let AstNode::EdgePattern { target_node, .. } = edge_node {
                        *target_node
                    } else {
                        NodeHandle::NULL
                    }
                };

                if !target_h.is_null()
                    && self.hoist_predicate_to_path(arena, target_h, target_var, pred_handle)?
                {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Reconstructs a binary `AND` expression tree wrapped in a `WhereClause` from a slice of conjunct handles.
    fn build_conjunction_tree(
        &self,
        arena: &mut QueryAstArena,
        conjuncts: &[NodeHandle],
    ) -> Option<NodeHandle> {
        if conjuncts.is_empty() {
            return None;
        }

        let mut current = conjuncts[0];
        for &next in &conjuncts[1..] {
            current = arena.alloc(AstNode::BinaryExpression {
                left: current,
                op: BinaryOp::And,
                right: next,
            });
        }
        Some(arena.alloc(AstNode::WhereClause {
            root_predicate: current,
        }))
    }

    /// Prunes unreferenced auto-generated variable aliases from node patterns.
    fn prune_dead_variables(&self, arena: &mut QueryAstArena, root: NodeHandle) -> Result<()> {
        let mut used_vars = HashSet::new();
        self.collect_used_variables(arena, root, &mut used_vars)?;

        let match_handles = {
            let root_node = arena.get(root)?;
            if let AstNode::QueryStatement { matches, .. } = root_node {
                matches.clone()
            } else {
                Vec::new()
            }
        };

        for match_h in match_handles {
            let path_handles = {
                let match_node = arena.get(match_h)?;
                if let AstNode::MatchClause { paths, .. } = match_node {
                    paths.clone()
                } else {
                    Vec::new()
                }
            };

            for path_h in path_handles {
                self.prune_path_variables(arena, path_h, &used_vars)?;
            }
        }

        Ok(())
    }

    fn collect_used_variables(
        &self,
        arena: &QueryAstArena,
        root: NodeHandle,
        used_vars: &mut HashSet<String>,
    ) -> Result<()> {
        let root_node = arena.get(root)?;
        if let AstNode::QueryStatement {
            matches,
            return_clause,
            mutations,
            with_clauses,
            unwinds,
            load_csv,
            ..
        } = root_node
        {
            if let Some(load_h) = load_csv {
                let load_node = arena.get(*load_h)?;
                if let AstNode::LoadCsvClause { url, alias, .. } = load_node {
                    self.collect_expr_vars(arena, *url, used_vars)?;
                    used_vars.insert(alias.clone());
                }
            }

            for &unwind_h in unwinds {
                let unwind_node = arena.get(unwind_h)?;
                if let AstNode::UnwindClause { expression, alias } = unwind_node {
                    self.collect_expr_vars(arena, *expression, used_vars)?;
                    used_vars.insert(alias.clone());
                }
            }

            if let AstNode::QueryStatement { linear_clauses, .. } = &root_node {
                for &lin_h in linear_clauses {
                    let lin_node = arena.get(lin_h)?;
                    match lin_node {
                        AstNode::LetClause {
                            variable,
                            expression,
                        } => {
                            used_vars.insert(variable.clone());
                            self.collect_expr_vars(arena, *expression, used_vars)?;
                        }
                        AstNode::FilterClause { predicate } => {
                            self.collect_expr_vars(arena, *predicate, used_vars)?;
                        }
                        _ => {}
                    }
                }
            }

            for &with_h in with_clauses {
                let with_node = arena.get(with_h)?;
                if let AstNode::WithClause {
                    projections,
                    order_by,
                    where_clause,
                    ..
                } = with_node
                {
                    for proj in projections {
                        self.collect_expr_vars(arena, proj.expression, used_vars)?;
                    }
                    for (order_expr, _) in order_by {
                        self.collect_expr_vars(arena, *order_expr, used_vars)?;
                    }
                    if let Some(wh) = where_clause {
                        self.collect_expr_vars(arena, *wh, used_vars)?;
                    }
                }
            }

            if let Some(ret_h) = return_clause {
                let ret_node = arena.get(*ret_h)?;
                if let AstNode::ReturnClause {
                    projections,
                    order_by,
                    ..
                } = ret_node
                {
                    for proj in projections {
                        self.collect_expr_vars(arena, proj.expression, used_vars)?;
                    }
                    for (order_expr, _) in order_by {
                        self.collect_expr_vars(arena, *order_expr, used_vars)?;
                    }
                }
            }

            for &match_h in matches {
                let match_node = arena.get(match_h)?;
                if let AstNode::MatchClause {
                    where_clause: Some(wh),
                    ..
                } = match_node
                {
                    self.collect_expr_vars(arena, *wh, used_vars)?;
                }
            }

            for &mut_h in mutations {
                let mut_node = arena.get(mut_h)?;
                match mut_node {
                    AstNode::SetClause { items } => {
                        for &item in items {
                            let item_node = arena.get(item)?;
                            if let AstNode::SetItem { target, value, .. } = item_node {
                                self.collect_expr_vars(arena, *target, used_vars)?;
                                self.collect_expr_vars(arena, *value, used_vars)?;
                            }
                        }
                    }
                    AstNode::DeleteClause { targets, .. } => {
                        for &target in targets {
                            self.collect_expr_vars(arena, target, used_vars)?;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn collect_expr_vars(
        &self,
        arena: &QueryAstArena,
        handle: NodeHandle,
        used_vars: &mut HashSet<String>,
    ) -> Result<()> {
        let node = arena.get(handle)?;
        match node {
            AstNode::Identifier(ident) => {
                used_vars.insert(ident.clone());
            }
            AstNode::PropertyAccess { target, .. } => {
                self.collect_expr_vars(arena, *target, used_vars)?;
            }
            AstNode::BinaryExpression { left, right, .. } => {
                self.collect_expr_vars(arena, *left, used_vars)?;
                self.collect_expr_vars(arena, *right, used_vars)?;
            }
            AstNode::UnaryExpression { operand, .. } => {
                self.collect_expr_vars(arena, *operand, used_vars)?;
            }
            AstNode::FunctionCall { arguments, .. } => {
                for &arg in arguments {
                    self.collect_expr_vars(arena, arg, used_vars)?;
                }
            }
            AstNode::CaseExpression {
                operand,
                when_then_branches,
                else_branch,
            } => {
                if let Some(op) = operand {
                    self.collect_expr_vars(arena, *op, used_vars)?;
                }
                for (when_expr, then_expr) in when_then_branches {
                    self.collect_expr_vars(arena, *when_expr, used_vars)?;
                    self.collect_expr_vars(arena, *then_expr, used_vars)?;
                }
                if let Some(else_expr) = else_branch {
                    self.collect_expr_vars(arena, *else_expr, used_vars)?;
                }
            }
            AstNode::ListComprehension {
                list_expression,
                where_filter,
                map_expression,
                ..
            } => {
                self.collect_expr_vars(arena, *list_expression, used_vars)?;
                if let Some(wh) = where_filter {
                    self.collect_expr_vars(arena, *wh, used_vars)?;
                }
                if let Some(map) = map_expression {
                    self.collect_expr_vars(arena, *map, used_vars)?;
                }
            }
            AstNode::PatternComprehension {
                where_filter,
                projection,
                ..
            } => {
                if let Some(wh) = where_filter {
                    self.collect_expr_vars(arena, *wh, used_vars)?;
                }
                self.collect_expr_vars(arena, *projection, used_vars)?;
            }
            AstNode::ExistsSubquery { subquery } | AstNode::CountSubquery { subquery } => {
                self.collect_expr_vars(arena, *subquery, used_vars)?;
            }
            AstNode::ListLiteral(items) => {
                for &item in items {
                    self.collect_expr_vars(arena, item, used_vars)?;
                }
            }
            AstNode::WhereClause { root_predicate } => {
                self.collect_expr_vars(arena, *root_predicate, used_vars)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn prune_path_variables(
        &self,
        arena: &mut QueryAstArena,
        path_handle: NodeHandle,
        used_vars: &HashSet<String>,
    ) -> Result<()> {
        let (is_prunable, is_chain, start_h, edge_handles) = {
            let path_node = arena.get(path_handle)?;
            match path_node {
                AstNode::NodePattern { variable, .. } => {
                    let prunable = if let Some(var) = variable {
                        var.starts_with('_') && !used_vars.contains(var)
                    } else {
                        false
                    };
                    (prunable, false, NodeHandle::NULL, Vec::new())
                }
                AstNode::PathChain {
                    start_node, edges, ..
                } => (false, true, *start_node, edges.clone()),
                _ => (false, false, NodeHandle::NULL, Vec::new()),
            }
        };

        if is_prunable {
            let node_mut = arena.get_mut(path_handle)?;
            if let AstNode::NodePattern { variable: v, .. } = node_mut {
                *v = None;
            }
        }

        if is_chain {
            self.prune_path_variables(arena, start_h, used_vars)?;

            for edge_h in edge_handles {
                let target_h = {
                    let edge_node = arena.get(edge_h)?;
                    if let AstNode::EdgePattern { target_node, .. } = edge_node {
                        *target_node
                    } else {
                        NodeHandle::NULL
                    }
                };

                if !target_h.is_null() {
                    self.prune_path_variables(arena, target_h, used_vars)?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::QueryBuilder;
    use crate::emitters::cypher::CypherEmitter;
    use crate::visitor::AstVisitor;

    #[test]
    fn test_optimizer_predicate_pushdown_single_node() {
        let mut builder = QueryBuilder::new();
        builder
            .match_node(Some("p"), vec!["Person"])
            .where_eq("p", "city", "New York")
            .field("p", "name", Some("name"));

        let (mut arena, root) = builder.build();

        let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
        optimizer.optimize(&mut arena, root).unwrap();

        let mut cypher = CypherEmitter::new();
        let res = cypher.visit_query(&arena, root).unwrap();

        // The where clause was hoisted into (p:Person {city: $p0}) and WHERE was removed!
        assert!(
            res.statement.contains("(p:Person {city: $p0})"),
            "Expected inlined property map, got: {}",
            res.statement
        );
        assert!(
            !res.statement.contains("WHERE"),
            "Expected WHERE clause to be completely eliminated, got: {}",
            res.statement
        );
    }

    #[test]
    fn test_optimizer_mixed_predicates_pushdown() {
        let mut builder = QueryBuilder::new();
        builder
            .match_node(Some("p"), vec!["Person"])
            .where_eq("p", "city", "London")
            .where_gt("p", "age", 21)
            .field("p", "name", Some("name"));

        let (mut arena, root) = builder.build();

        let optimizer = AstOptimizer::new(OptimizationLevel::Standard);
        optimizer.optimize(&mut arena, root).unwrap();

        let mut cypher = CypherEmitter::new();
        let res = cypher.visit_query(&arena, root).unwrap();

        // Equality is pushed down, inequality remains in WHERE clause
        assert!(
            res.statement.contains("(p:Person {city: $p0})"),
            "Expected inlined city, got: {}",
            res.statement
        );
        assert!(
            res.statement.contains("WHERE p.age > $p1"),
            "Expected WHERE p.age > $p1, got: {}",
            res.statement
        );
    }

    #[test]
    fn test_optimizer_level_none() {
        let mut builder = QueryBuilder::new();
        builder
            .match_node(Some("p"), vec!["Person"])
            .where_eq("p", "city", "Paris")
            .field("p", "name", Some("name"));

        let (mut arena, root) = builder.build();

        let optimizer = AstOptimizer::new(OptimizationLevel::None);
        optimizer.optimize(&mut arena, root).unwrap();

        let mut cypher = CypherEmitter::new();
        let res = cypher.visit_query(&arena, root).unwrap();

        // No optimization applied: WHERE p.city = $p0 is preserved
        assert!(
            res.statement.contains("WHERE p.city = $p0"),
            "Expected unoptimized WHERE, got: {}",
            res.statement
        );
    }
}
