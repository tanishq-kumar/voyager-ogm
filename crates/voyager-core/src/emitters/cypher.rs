//! openCypher & Cypher 25 Dialect Emitter.

use crate::ast::{
    AggregationFunc, AstNode, Direction, LiteralValue, NodeHandle, ProjectionItem, QueryAstArena,
};
use crate::error::{Error, Result};
use crate::visitor::{AstVisitor, CompiledQuery};
use std::collections::HashMap;

/// Emits standardized, parameterized openCypher 9 and Cypher 25 query strings.
#[derive(Debug, Default)]
pub struct CypherEmitter {
    param_counter: usize,
    parameters: HashMap<String, LiteralValue>,
    buffer: String,
}

impl CypherEmitter {
    /// Creates a fresh Cypher emitter instance.
    pub fn new() -> Self {
        Self {
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
            if let Some(var) = variable {
                self.buffer.push_str(var);
            }
            for label in labels {
                self.buffer.push(':');
                self.buffer.push_str(label);
            }
            if !predicates.is_empty() {
                self.buffer.push_str(" {");
                for (i, &pred_handle) in predicates.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.emit_expression(arena, pred_handle, false)?;
                }
                self.buffer.push('}');
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
                Direction::Incoming => self.buffer.push_str("<-["),
                Direction::Outgoing | Direction::Undirected => self.buffer.push_str("-["),
            }

            if let Some(var) = variable {
                self.buffer.push_str(var);
            }

            for (i, edge_type) in edge_types.iter().enumerate() {
                if i == 0 {
                    self.buffer.push(':');
                } else {
                    self.buffer.push('|');
                }
                self.buffer.push_str(edge_type);
            }

            if min_hops.is_some() || max_hops.is_some() {
                self.buffer.push('*');
                if let Some(min) = min_hops {
                    self.buffer.push_str(&min.to_string());
                }
                self.buffer.push_str("..");
                if let Some(max) = max_hops {
                    self.buffer.push_str(&max.to_string());
                }
            }

            if !predicates.is_empty() {
                self.buffer.push_str(" {");
                for (i, &pred_handle) in predicates.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.emit_expression(arena, pred_handle, false)?;
                }
                self.buffer.push('}');
            }

            match direction {
                Direction::Outgoing => self.buffer.push_str("]->"),
                Direction::Incoming | Direction::Undirected => self.buffer.push_str("]-"),
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
            AstNode::Parameter(param) => {
                self.buffer.push('$');
                self.buffer.push_str(param);
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
                self.buffer.push(' ');
                self.buffer.push_str(&op.to_string());
                self.buffer.push(' ');
                self.emit_expression(arena, *right, true)?;
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

    fn emit_return(
        &mut self,
        arena: &QueryAstArena,
        projections: &[ProjectionItem],
        distinct: bool,
        order_by: &[(NodeHandle, bool)],
        skip: Option<u64>,
        limit: Option<u64>,
    ) -> Result<()> {
        self.buffer.push_str(" RETURN ");
        if distinct {
            self.buffer.push_str("DISTINCT ");
        }

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
                    AggregationFunc::Collect => self.buffer.push_str("COLLECT("),
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

        if !order_by.is_empty() {
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

        if let Some(s) = skip {
            self.buffer.push_str(&format!(" SKIP {s}"));
        }

        if let Some(l) = limit {
            self.buffer.push_str(&format!(" LIMIT {l}"));
        }

        Ok(())
    }

    fn emit_set_item(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::SetItem {
            target,
            value,
            is_merge,
        } = node
        {
            self.emit_expression(arena, *target, false)?;
            if *is_merge {
                self.buffer.push_str(" += ");
            } else {
                self.buffer.push_str(" = ");
            }
            self.emit_expression(arena, *value, false)?;
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected SetItem, got {node:?}"
            )))
        }
    }

    fn emit_create(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::CreateClause { paths } = node {
            self.buffer.push_str("CREATE ");
            for (i, &p) in paths.iter().enumerate() {
                if i > 0 {
                    self.buffer.push_str(", ");
                }
                self.emit_path(arena, p)?;
            }
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected CreateClause, got {node:?}"
            )))
        }
    }

    fn emit_merge(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::MergeClause {
            path,
            on_create_set,
            on_match_set,
        } = node
        {
            self.buffer.push_str("MERGE ");
            self.emit_path(arena, *path)?;

            if !on_create_set.is_empty() {
                self.buffer.push_str(" ON CREATE SET ");
                for (i, &item) in on_create_set.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.emit_set_item(arena, item)?;
                }
            }

            if !on_match_set.is_empty() {
                self.buffer.push_str(" ON MATCH SET ");
                for (i, &item) in on_match_set.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.emit_set_item(arena, item)?;
                }
            }
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected MergeClause, got {node:?}"
            )))
        }
    }

    fn emit_set(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::SetClause { items } = node {
            self.buffer.push_str("SET ");
            for (i, &item) in items.iter().enumerate() {
                if i > 0 {
                    self.buffer.push_str(", ");
                }
                self.emit_set_item(arena, item)?;
            }
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected SetClause, got {node:?}"
            )))
        }
    }

    fn emit_delete(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::DeleteClause { detach, targets } = node {
            if *detach {
                self.buffer.push_str("DETACH DELETE ");
            } else {
                self.buffer.push_str("DELETE ");
            }
            for (i, &t) in targets.iter().enumerate() {
                if i > 0 {
                    self.buffer.push_str(", ");
                }
                self.emit_expression(arena, t, false)?;
            }
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected DeleteClause, got {node:?}"
            )))
        }
    }

    fn emit_remove(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::RemoveClause { items } = node {
            self.buffer.push_str("REMOVE ");
            for (i, &item) in items.iter().enumerate() {
                if i > 0 {
                    self.buffer.push_str(", ");
                }
                self.emit_expression(arena, item, false)?;
            }
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected RemoveClause, got {node:?}"
            )))
        }
    }

    fn emit_unwind(&mut self, arena: &QueryAstArena, handle: NodeHandle) -> Result<()> {
        let node = arena.get(handle)?;
        if let AstNode::UnwindClause { expression, alias } = node {
            self.buffer.push_str("UNWIND ");
            self.emit_expression(arena, *expression, false)?;
            self.buffer.push_str(" AS ");
            self.buffer.push_str(alias);
            Ok(())
        } else {
            Err(Error::AstInvariantViolation(format!(
                "Expected UnwindClause, got {node:?}"
            )))
        }
    }
}

impl AstVisitor for CypherEmitter {
    fn visit_query(&mut self, arena: &QueryAstArena, root: NodeHandle) -> Result<CompiledQuery> {
        self.buffer.clear();
        self.parameters.clear();
        self.param_counter = 0;

        let root_node = arena.get(root)?;
        match root_node {
            AstNode::QueryStatement {
                unwinds,
                matches,
                mutations,
                return_clause,
            } => {
                let mut has_emitted = false;

                for &unwind_handle in unwinds {
                    if has_emitted {
                        self.buffer.push(' ');
                    }
                    has_emitted = true;
                    self.emit_unwind(arena, unwind_handle)?;
                }

                for &match_handle in matches {
                    if has_emitted {
                        self.buffer.push(' ');
                    }
                    has_emitted = true;

                    let match_node = arena.get(match_handle)?;
                    if let AstNode::MatchClause {
                        optional,
                        paths,
                        where_clause,
                    } = match_node
                    {
                        if *optional {
                            self.buffer.push_str("OPTIONAL MATCH ");
                        } else {
                            self.buffer.push_str("MATCH ");
                        }

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

                for &mut_handle in mutations {
                    if has_emitted {
                        self.buffer.push(' ');
                    }
                    has_emitted = true;

                    let mut_node = arena.get(mut_handle)?;
                    match mut_node {
                        AstNode::CreateClause { .. } => self.emit_create(arena, mut_handle)?,
                        AstNode::MergeClause { .. } => self.emit_merge(arena, mut_handle)?,
                        AstNode::SetClause { .. } => self.emit_set(arena, mut_handle)?,
                        AstNode::DeleteClause { .. } => self.emit_delete(arena, mut_handle)?,
                        AstNode::RemoveClause { .. } => self.emit_remove(arena, mut_handle)?,
                        other => {
                            return Err(Error::AstInvariantViolation(format!(
                                "Unsupported mutation clause in QueryStatement: {other:?}"
                            )));
                        }
                    }
                }

                if let Some(ret_handle) = return_clause {
                    let ret_node = arena.get(*ret_handle)?;
                    if let AstNode::ReturnClause {
                        distinct,
                        projections,
                        order_by,
                        skip,
                        limit,
                    } = ret_node
                    {
                        self.emit_return(arena, projections, *distinct, order_by, *skip, *limit)?;
                    }
                }

                Ok(CompiledQuery::new(
                    std::mem::take(&mut self.buffer),
                    std::mem::take(&mut self.parameters),
                ))
            }
            AstNode::ProcedureCall {
                namespace,
                procedure,
                arguments,
                yield_items,
            } => {
                self.buffer.push_str("CALL ");
                if let Some(ns) = namespace {
                    self.buffer.push_str(ns);
                    self.buffer.push('.');
                }
                self.buffer.push_str(procedure);
                self.buffer.push('(');
                for (i, &arg) in arguments.iter().enumerate() {
                    if i > 0 {
                        self.buffer.push_str(", ");
                    }
                    self.emit_expression(arena, arg, false)?;
                }
                self.buffer.push(')');

                if !yield_items.is_empty() {
                    self.buffer.push_str(" YIELD ");
                    for (i, item) in yield_items.iter().enumerate() {
                        if i > 0 {
                            self.buffer.push_str(", ");
                        }
                        self.buffer.push_str(item);
                    }
                }

                Ok(CompiledQuery::new(
                    std::mem::take(&mut self.buffer),
                    std::mem::take(&mut self.parameters),
                ))
            }
            other => Err(Error::AstInvariantViolation(format!(
                "Invalid root AST node: {other:?}"
            ))),
        }
    }
}
