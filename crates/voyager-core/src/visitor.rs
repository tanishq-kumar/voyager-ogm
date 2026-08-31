//! AST Visitor Trait and Compiled Query Container.

use crate::ast::{LiteralValue, NodeHandle, QueryAstArena};
use crate::error::Result;
use std::collections::HashMap;

/// Result of compiling an AST query into a dialect-specific parameterized string.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledQuery {
    /// The formatted, dialect-specific query string.
    pub statement: String,
    /// Parameter key-value map extracted during emission (e.g. `{"p0": Int64(21)}`).
    pub parameters: HashMap<String, LiteralValue>,
}

impl CompiledQuery {
    /// Creates a new compiled query result.
    pub fn new(statement: String, parameters: HashMap<String, LiteralValue>) -> Self {
        Self {
            statement,
            parameters,
        }
    }

    /// Returns the parameters sorted deterministically by key name.
    pub fn sorted_parameters(&self) -> std::collections::BTreeMap<String, LiteralValue> {
        self.parameters.clone().into_iter().collect()
    }
}

/// Core AST Visitor trait for dialect emitters, linters, and optimizers.
pub trait AstVisitor {
    /// Compiles a root AST statement handle into a dialect query.
    fn visit_query(&mut self, arena: &QueryAstArena, root: NodeHandle) -> Result<CompiledQuery>;
}
