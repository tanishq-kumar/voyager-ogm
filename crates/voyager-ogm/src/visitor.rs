//! AST Visitor Trait and Compiled Query Container.

use crate::ast::{ExecutionMode, LiteralValue, NodeHandle, QueryAstArena};
use crate::error::Result;
use std::collections::HashMap;

/// Metadata describing an output projection column in a compiled query.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColumnMeta {
    /// Canonical output column name (e.g. alias if present, otherwise variable/field name).
    pub name: String,
    /// Explicit column alias if declared with `AS <alias>`.
    pub alias: Option<String>,
}

impl ColumnMeta {
    /// Creates a new `ColumnMeta` with the given column name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
        }
    }

    /// Creates a new `ColumnMeta` with an explicit alias.
    pub fn with_alias(name: impl Into<String>, alias: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: Some(alias.into()),
        }
    }

    /// Resolves column metadata from a list of [`crate::ast::ProjectionItem`] AST nodes.
    pub fn from_projections(
        arena: &QueryAstArena,
        projections: &[crate::ast::ProjectionItem],
    ) -> Vec<Self> {
        projections
            .iter()
            .enumerate()
            .map(|(idx, proj)| {
                if let Some(alias) = &proj.alias {
                    Self {
                        name: alias.clone(),
                        alias: Some(alias.clone()),
                    }
                } else {
                    let base_name = resolve_expression_name(arena, proj.expression)
                        .unwrap_or_else(|| format!("col_{idx}"));
                    let final_name = if let Some(func) = proj.aggregation {
                        format!("{func:?}({base_name})").to_ascii_lowercase()
                    } else {
                        base_name
                    };
                    Self {
                        name: final_name,
                        alias: None,
                    }
                }
            })
            .collect()
    }
}

fn resolve_expression_name(arena: &QueryAstArena, handle: NodeHandle) -> Option<String> {
    match arena.get(handle).ok()? {
        crate::ast::AstNode::Identifier(id) => Some(id.clone()),
        crate::ast::AstNode::NodePattern { variable, .. } => variable.clone(),
        crate::ast::AstNode::EdgePattern { variable, .. } => variable.clone(),
        crate::ast::AstNode::PropertyAccess { target, property } => {
            if let Some(target_name) = resolve_expression_name(arena, *target) {
                if property.is_empty() {
                    Some(target_name)
                } else {
                    Some(format!("{target_name}.{property}"))
                }
            } else if !property.is_empty() {
                Some(property.clone())
            } else {
                None
            }
        }
        crate::ast::AstNode::Parameter(param) => Some(param.clone()),
        crate::ast::AstNode::Literal(lit) => Some(format!("{lit:?}")),
        crate::ast::AstNode::FunctionCall { name, .. } => Some(format!("{name}(...)")),
        _ => None,
    }
}

/// Result of compiling an AST query into a dialect-specific parameterized string.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledQuery {
    /// The formatted, dialect-specific query string.
    pub statement: String,
    /// Parameter key-value map extracted during emission (e.g. `{"p0": Int64(21)}`).
    pub parameters: HashMap<String, LiteralValue>,
    /// Execution mode of the query (Normal, Explain, Profile, ExplainAndProfile).
    pub execution_mode: ExecutionMode,
    /// Metadata for projected columns (name, optional alias).
    pub columns: Vec<ColumnMeta>,
}

impl CompiledQuery {
    /// Creates a new compiled query result with default Normal execution mode.
    pub fn new(statement: String, parameters: HashMap<String, LiteralValue>) -> Self {
        Self {
            statement,
            parameters,
            execution_mode: ExecutionMode::Normal,
            columns: Vec::new(),
        }
    }

    /// Creates a new compiled query result with an explicit execution mode.
    pub fn with_execution_mode(
        statement: String,
        parameters: HashMap<String, LiteralValue>,
        execution_mode: ExecutionMode,
    ) -> Self {
        Self {
            statement,
            parameters,
            execution_mode,
            columns: Vec::new(),
        }
    }

    /// Creates a new compiled query result with columns and execution mode.
    pub fn with_columns(
        statement: String,
        parameters: HashMap<String, LiteralValue>,
        execution_mode: ExecutionMode,
        columns: Vec<ColumnMeta>,
    ) -> Self {
        Self {
            statement,
            parameters,
            execution_mode,
            columns,
        }
    }

    /// Returns the parameters sorted deterministically by key name.
    pub fn sorted_parameters(&self) -> std::collections::BTreeMap<String, LiteralValue> {
        self.parameters.clone().into_iter().collect()
    }

    /// Returns a slice of projected columns.
    pub fn columns(&self) -> &[ColumnMeta] {
        &self.columns
    }
}

/// Core AST Visitor trait for dialect emitters, linters, and optimizers.
pub trait AstVisitor {
    /// Compiles a root AST statement handle into a dialect query.
    fn visit_query(&mut self, arena: &QueryAstArena, root: NodeHandle) -> Result<CompiledQuery>;
}
