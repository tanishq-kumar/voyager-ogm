//! Rule-based AST Query Optimizer for Voyager OGM.
//!
//! Provides multi-level rule-based optimizations over [`QueryAstArena`]:
//! - **Predicate Pushdown**: Hoists single-node equality filters from `WHERE` clauses into inline node property patterns (`(p:Person {city: $p0})`) to enable database index seeks before path traversal.
//! - **Boolean Simplification**: Flattens conjunction chains (`AND`) and strips redundant boolean constants.
//! - **Dead Variable Pruning**: In aggressive mode, eliminates unreferenced intermediate internal aliases.

use crate::ast::{AstNode, BinaryOp, LiteralValue, NodeHandle, QueryAstArena};
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

        // Pass 1 & 2: Predicate Pushdown & Conjunction Simplification
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

        for match_handle in match_handles {
            self.optimize_match_clause(arena, match_handle)?;
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
    ) -> Result<()> {
        let (paths, where_clause) = {
            let match_node = arena.get(match_handle)?;
            if let AstNode::MatchClause {
                paths,
                where_clause,
                ..
            } = match_node
            {
                (paths.clone(), *where_clause)
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
                    // Try to hoist this equality into a matching node pattern in this match clause
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

    fn optimize_mutation_clause(
        &self,
        arena: &mut QueryAstArena,
        mut_handle: NodeHandle,
    ) -> Result<()> {
        let mut_node = arena.get(mut_handle)?;
        if let AstNode::MergeClause { path, .. } = mut_node {
            let path_handle = *path;
            let _ = path_handle;
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

            // Case A: Left is `PropertyAccess`, right is literal/param
            if let AstNode::PropertyAccess { target, .. } = left_node {
                if self.is_literal_or_param(right_node) {
                    if let Some(var) = self.resolve_variable_name(arena, *target)? {
                        return Ok(Some(var));
                    }
                }
            }

            // Case B: Right is `PropertyAccess`, left is literal/param
            if let AstNode::PropertyAccess { target, .. } = right_node {
                if self.is_literal_or_param(left_node) {
                    if let Some(var) = self.resolve_variable_name(arena, *target)? {
                        return Ok(Some(var));
                    }
                }
            }
        }
        Ok(None)
    }

    fn is_literal_or_param(&self, node: &AstNode) -> bool {
        matches!(node, AstNode::Literal(_) | AstNode::Parameter(_))
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
                AstNode::PathChain { start_node, edges } => {
                    (false, true, *start_node, edges.clone())
                }
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
            ..
        } = root_node
        {
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
                if let AstNode::MatchClause { where_clause, .. } = match_node {
                    if let Some(wh) = where_clause {
                        self.collect_expr_vars(arena, *wh, used_vars)?;
                    }
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
                AstNode::PathChain { start_node, edges } => {
                    (false, true, *start_node, edges.clone())
                }
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
