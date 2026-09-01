//! # Database Bridging Layer (`voyager-core::bridge`)
//!
//! Provides a vendor-neutral interface for executing compiled graph queries
//! across external database drivers (Bolt/Neo4j/Memgraph, DuckDB/DuckPGQ, Kuzu, etc.)
//! without coupling driver implementations into the core AST compiler.

use crate::error::Result;
use crate::visitor::CompiledQuery;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Metadata summary of an executed graph query.
#[derive(Debug, Clone, Default)]
pub struct QuerySummary {
    /// Number of nodes created/updated/deleted.
    pub nodes_affected: usize,
    /// Number of relationships created/updated/deleted.
    pub relationships_affected: usize,
    /// Execution duration on the database server.
    pub execution_time: Duration,
}

/// Generic record result returned by a database bridge.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Column names returned by the query.
    pub columns: Vec<String>,
    /// Result rows as column-value dictionaries.
    pub rows: Vec<HashMap<String, String>>,
    /// Summary of database state changes.
    pub summary: QuerySummary,
}

/// Vendor-neutral Database Bridge Trait.
///
/// Implemented by database adapters to decouple query compilation from transport execution.
pub trait DatabaseBridge: Send + Sync {
    /// Executes a compiled query and returns the structured result.
    fn execute(&self, query: &CompiledQuery) -> Result<QueryResult>;

    /// Executes a series of compiled queries in a batch or transaction.
    fn execute_batch(&self, queries: &[CompiledQuery]) -> Result<Vec<QueryResult>> {
        let mut results = Vec::with_capacity(queries.len());
        for q in queries {
            results.push(self.execute(q)?);
        }
        Ok(results)
    }
}

/// In-memory Mock Database Bridge for zero-network testing and recording.
#[derive(Debug, Default, Clone)]
pub struct MockDatabaseBridge {
    /// History of executed compiled queries.
    executed_queries: Arc<Mutex<Vec<CompiledQuery>>>,
    /// Pre-configured canned responses to return upon execution.
    canned_results: Arc<Mutex<Vec<QueryResult>>>,
}

impl MockDatabaseBridge {
    /// Creates a new empty mock database bridge.
    pub fn new() -> Self {
        Self {
            executed_queries: Arc::new(Mutex::new(Vec::new())),
            canned_results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Queues a canned query result to be returned by future `execute()` calls.
    pub fn queue_result(&self, result: QueryResult) {
        if let Ok(mut lock) = self.canned_results.lock() {
            lock.push(result);
        }
    }

    /// Returns a list of all compiled queries executed through this bridge.
    pub fn get_executed_queries(&self) -> Vec<CompiledQuery> {
        self.executed_queries
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Clears the recorded execution history.
    pub fn clear(&self) {
        if let Ok(mut lock) = self.executed_queries.lock() {
            lock.clear();
        }
        if let Ok(mut lock) = self.canned_results.lock() {
            lock.clear();
        }
    }
}

impl DatabaseBridge for MockDatabaseBridge {
    fn execute(&self, query: &CompiledQuery) -> Result<QueryResult> {
        if let Ok(mut lock) = self.executed_queries.lock() {
            lock.push(query.clone());
        }

        if let Ok(mut lock) = self.canned_results.lock()
            && !lock.is_empty()
        {
            return Ok(lock.remove(0));
        }

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            summary: QuerySummary::default(),
        })
    }
}
