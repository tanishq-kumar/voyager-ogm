//! Asynchronous database engine, connection, and transaction traits for Voyager OGM.

use arrow_array::RecordBatch;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{NetError, Result};

/// Execution summary metrics and mutation counters returned after query execution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuerySummary {
    /// Number of graph nodes created by the query.
    pub nodes_created: u64,
    /// Number of graph nodes deleted by the query.
    pub nodes_deleted: u64,
    /// Number of graph relationships created by the query.
    pub relationships_created: u64,
    /// Number of graph relationships deleted by the query.
    pub relationships_deleted: u64,
    /// Number of properties set or updated on nodes/relationships.
    pub properties_set: u64,
    /// Number of node labels added.
    pub labels_added: u64,
    /// Number of node labels removed.
    pub labels_removed: u64,
    /// Number of schema indexes created.
    pub indexes_added: u64,
    /// Number of schema constraints created.
    pub constraints_added: u64,
    /// Total server-side query planning and execution duration in milliseconds.
    pub execution_time_ms: u64,
}

impl QuerySummary {
    /// Creates an empty summary with all counters initialized to zero.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Returns `true` if any mutating counters (creates, updates, deletes) are non-zero.
    pub fn is_mutating(&self) -> bool {
        self.nodes_created > 0
            || self.nodes_deleted > 0
            || self.relationships_created > 0
            || self.relationships_deleted > 0
            || self.properties_set > 0
            || self.labels_added > 0
            || self.labels_removed > 0
            || self.indexes_added > 0
            || self.constraints_added > 0
    }
}

/// Query result container containing projected column names, Apache Arrow record batches, and execution statistics.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Ordered list of projected return column names.
    pub columns: Vec<String>,
    /// Apache Arrow record batches containing tabular columnar results.
    pub batches: Vec<RecordBatch>,
    /// Server-side execution statistics and mutation counters.
    pub summary: QuerySummary,
}

impl QueryResult {
    /// Creates a new `QueryResult` with the given columns, record batches, and summary.
    pub fn new(columns: Vec<String>, batches: Vec<RecordBatch>, summary: QuerySummary) -> Self {
        Self {
            columns,
            batches,
            summary,
        }
    }

    /// Creates an empty `QueryResult` for non-returning mutating queries.
    pub fn empty(summary: QuerySummary) -> Self {
        Self {
            columns: Vec::new(),
            batches: Vec::new(),
            summary,
        }
    }

    /// Returns total row count across all record batches in the result.
    pub fn row_count(&self) -> usize {
        self.batches.iter().map(|b| b.num_rows()).sum()
    }

    /// Returns `true` if there are no rows in the result.
    pub fn is_empty(&self) -> bool {
        self.row_count() == 0
    }

    /// Consolidates multiple Arrow record batches into a single `RecordBatch`.
    pub fn into_single_batch(self) -> Result<Option<RecordBatch>> {
        if self.batches.is_empty() {
            return Ok(None);
        }
        if self.batches.len() == 1 {
            return Ok(Some(self.batches.into_iter().next().unwrap()));
        }

        let schema = self.batches[0].schema();
        let concatenated =
            arrow::compute::concat_batches(&schema, &self.batches).map_err(NetError::ArrowError)?;
        Ok(Some(concatenated))
    }
}

/// Abstract asynchronous physical connection to a database.
#[async_trait]
pub trait AsyncConnection: Send + Sync {
    /// Executes a parameterized query string against the active connection.
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult>;

    /// Sends a lightweight liveness probe / ping to verify socket connectivity.
    async fn ping(&mut self) -> Result<()>;

    /// Resets the connection state, aborting any active open transactions or buffer states.
    async fn reset(&mut self) -> Result<()>;

    /// Returns `true` if the underlying network socket or stream is currently healthy and open.
    fn is_valid(&self) -> bool;

    /// Returns `true` if the connection is currently inside an explicit transaction block.
    fn is_in_transaction(&self) -> bool;

    /// Gracefully closes the underlying socket and releases server resources.
    async fn close(&mut self) -> Result<()>;
}

#[async_trait]
impl AsyncConnection for Box<dyn AsyncConnection> {
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        (**self).execute(query, params).await
    }

    async fn ping(&mut self) -> Result<()> {
        (**self).ping().await
    }

    async fn reset(&mut self) -> Result<()> {
        (**self).reset().await
    }

    fn is_valid(&self) -> bool {
        (**self).is_valid()
    }

    fn is_in_transaction(&self) -> bool {
        (**self).is_in_transaction()
    }

    async fn close(&mut self) -> Result<()> {
        (**self).close().await
    }
}

/// Abstract asynchronous database transaction.
#[async_trait]
pub trait AsyncTransaction: Send + Sync {
    /// Executes a parameterized query within the transaction boundary.
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult>;

    /// Commits the active transaction to durable storage.
    async fn commit(self: Box<Self>) -> Result<QuerySummary>;

    /// Aborts and rolls back the active transaction.
    async fn rollback(self: Box<Self>) -> Result<()>;
}

/// Factory trait for asynchronously spawning fresh physical database connections.
#[async_trait]
pub trait ConnectionFactory<C: AsyncConnection>: Send + Sync + 'static {
    /// Asynchronously establishes a new connection to the database.
    async fn create(&self) -> Result<C>;
}

/// Function-pointer or closure based connection factory wrapper.
pub struct FnConnectionFactory<F, Fut, C>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<C>> + Send + 'static,
    C: AsyncConnection + 'static,
{
    factory_fn: F,
    _phantom: std::marker::PhantomData<fn() -> (Fut, C)>,
}

impl<F, Fut, C> FnConnectionFactory<F, Fut, C>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<C>> + Send + 'static,
    C: AsyncConnection + 'static,
{
    /// Creates a new connection factory from an asynchronous closure.
    pub fn new(factory_fn: F) -> Self {
        Self {
            factory_fn,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<F, Fut, C> ConnectionFactory<C> for FnConnectionFactory<F, Fut, C>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<C>> + Send + 'static,
    C: AsyncConnection + 'static,
{
    async fn create(&self) -> Result<C> {
        (self.factory_fn)().await
    }
}

/// High-level asynchronous database engine interface.
#[async_trait]
pub trait AsyncEngine: Send + Sync {
    /// Executes a parameterized query using an automatically leased connection from the engine's pool.
    async fn execute(
        &self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult>;

    /// Returns the engine's health status and connection pool statistics.
    async fn ping(&self) -> Result<()>;
}
