//! Apache AGE (PostgreSQL Embedded Cypher) Emitter.

use crate::ast::{AstNode, NodeHandle, QueryAstArena};
use crate::emitters::cypher::CypherEmitter;
use crate::error::Result;
use crate::visitor::{AstVisitor, CompiledQuery};

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
}

impl Default for AgeEmitter {
    fn default() -> Self {
        Self::new("age_graph")
    }
}

impl AstVisitor for AgeEmitter {
    fn visit_query(&mut self, arena: &QueryAstArena, root: NodeHandle) -> Result<CompiledQuery> {
        let mut cypher_emitter = CypherEmitter::new();
        let cypher_compiled = cypher_emitter.visit_query(arena, root)?;

        let root_node = arena.get(root)?;
        let mut column_defs = Vec::new();

        if let AstNode::QueryStatement {
            return_clause: Some(ret_handle),
            ..
        } = root_node
        {
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

        let wrapped_statement = format!(
            "SELECT * FROM cypher('{}', $$ {} $${}) {}",
            self.graph_name, cypher_compiled.statement, params_arg, as_clause
        );

        Ok(CompiledQuery::new(
            wrapped_statement,
            cypher_compiled.parameters,
        ))
    }
}
