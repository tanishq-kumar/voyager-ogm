//! Apache AGE (PostgreSQL Embedded Cypher) Emitter.

use crate::ast::{AstNode, ExecutionMode, NodeHandle, QueryAstArena};
use crate::emitters::cypher::CypherEmitter;
use crate::error::Result;
use crate::visitor::{AstVisitor, CompiledQuery};

/// Default graph name used by Apache AGE queries when none is explicitly configured.
pub const DEFAULT_AGE_GRAPH: &str = "age_graph";

/// Validates whether a graph name is a safe PostgreSQL / Apache AGE identifier.
pub fn is_valid_age_graph_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 63 {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Emits PostgreSQL Apache AGE wrapped Cypher queries:
/// `SELECT * FROM cypher('graph_name', $$ MATCH ... RETURN ... $$) AS (col1 agtype, col2 agtype)`
#[derive(Debug)]
pub struct AgeEmitter {
    graph_name: String,
}

impl AgeEmitter {
    /// Creates a fresh Apache AGE emitter targeting a specific graph name.
    pub fn new(graph_name: impl Into<String>) -> Self {
        Self {
            graph_name: graph_name.into(),
        }
    }

    /// Returns the target graph name.
    pub fn graph_name(&self) -> &str {
        &self.graph_name
    }
}

impl Default for AgeEmitter {
    fn default() -> Self {
        Self::new(DEFAULT_AGE_GRAPH)
    }
}

impl AstVisitor for AgeEmitter {
    fn visit_query(&mut self, arena: &QueryAstArena, root: NodeHandle) -> Result<CompiledQuery> {
        if !is_valid_age_graph_name(&self.graph_name) {
            return Err(crate::error::Error::EmissionError(format!(
                "Invalid Apache AGE graph name '{}'. Graph names must be valid identifiers (alphanumeric and underscores, starting with a letter or underscore, 1 to 63 characters).",
                self.graph_name
            )));
        }
        let mut cypher_emitter = CypherEmitter::new().without_execution_mode();
        let cypher_compiled = cypher_emitter.visit_query(arena, root)?;

        let root_node = arena.get(root)?;
        let mut column_defs = Vec::new();
        let mut execution_mode = ExecutionMode::Normal;

        if let AstNode::QueryStatement {
            execution_mode: mode,
            return_clause: Some(ret_handle),
            ..
        } = root_node
        {
            execution_mode = *mode;
            let ret_node = arena.get(*ret_handle)?;
            if let AstNode::ReturnClause { projections, .. } = ret_node {
                for proj in projections {
                    let col_name = if let Some(alias) = &proj.alias {
                        alias.clone()
                    } else {
                        match arena.get(proj.expression)? {
                            AstNode::PropertyAccess { property, .. } => {
                                if property.is_empty() {
                                    "entity".to_string()
                                } else {
                                    property.clone()
                                }
                            }
                            AstNode::Identifier(id) => id.clone(),
                            _ => "col".to_string(),
                        }
                    };
                    column_defs.push(format!("{col_name} agtype"));
                }
            }
        } else if let AstNode::QueryStatement {
            execution_mode: mode,
            ..
        } = root_node
        {
            execution_mode = *mode;
        } else if let AstNode::ProcedureCall {
            execution_mode: mode,
            ..
        } = root_node
        {
            execution_mode = *mode;
        }

        let as_clause = if column_defs.is_empty() {
            "AS (result agtype)".to_string()
        } else {
            format!("AS ({})", column_defs.join(", "))
        };

        let params_arg = if cypher_compiled.parameters.is_empty() {
            String::new()
        } else {
            ", %s".to_string()
        };

        let prefix = match execution_mode {
            ExecutionMode::Normal => "",
            ExecutionMode::Explain => "EXPLAIN ",
            ExecutionMode::Profile | ExecutionMode::ExplainAndProfile => "EXPLAIN ANALYZE ",
        };

        let wrapped_statement = format!(
            "{}SELECT * FROM cypher('{}', $$ {} $${}) {}",
            prefix, self.graph_name, cypher_compiled.statement, params_arg, as_clause
        );

        Ok(CompiledQuery::with_columns(
            wrapped_statement,
            cypher_compiled.parameters,
            execution_mode,
            cypher_compiled.columns,
        ))
    }
}
