//! PostgreSQL asynchronous transaction manager implementing [`AsyncTransaction`].

use async_trait::async_trait;
use std::collections::HashMap;

use crate::engine::{AsyncConnection, AsyncTransaction, QueryResult, QuerySummary};
use crate::error::{NetError, Result};
use crate::postgres::connection::PostgresConnection;

/// Asynchronous transaction wrapper managing a leased [`PostgresConnection`].
pub struct PostgresTransaction<'a> {
    conn: &'a mut PostgresConnection,
    is_active: bool,
    summary_accumulator: QuerySummary,
}

impl<'a> PostgresTransaction<'a> {
    /// Begins a new transaction on the given PostgreSQL connection.
    pub async fn begin(conn: &'a mut PostgresConnection) -> Result<Self> {
        conn.execute_simple("BEGIN;").await?;
        Ok(Self {
            conn,
            is_active: true,
            summary_accumulator: QuerySummary::empty(),
        })
    }

    /// Executes a parameterized query within the transaction boundary.
    pub async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Cannot execute query on inactive transaction".to_string(),
            ));
        }

        let res = self.conn.execute(query, params).await?;
        self.summary_accumulator.nodes_created += res.summary.nodes_created;
        self.summary_accumulator.nodes_deleted += res.summary.nodes_deleted;
        self.summary_accumulator.properties_set += res.summary.properties_set;
        self.summary_accumulator.execution_time_ms += res.summary.execution_time_ms;

        Ok(res)
    }

    /// Commits the active transaction to durable storage.
    pub async fn commit(mut self) -> Result<QuerySummary> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Transaction is already completed".to_string(),
            ));
        }
        self.conn.execute_simple("COMMIT;").await?;
        self.is_active = false;
        Ok(self.summary_accumulator.clone())
    }

    /// Aborts and rolls back the active transaction.
    pub async fn rollback(mut self) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }
        self.conn.execute_simple("ROLLBACK;").await?;
        self.is_active = false;
        Ok(())
    }

    /// Creates a nested transaction savepoint.
    pub async fn savepoint(&mut self, name: &str) -> Result<()> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Cannot create savepoint in inactive transaction".to_string(),
            ));
        }
        self.conn
            .execute_simple(&format!("SAVEPOINT {};", name))
            .await?;
        Ok(())
    }

    /// Rolls back to a previous transaction savepoint.
    pub async fn rollback_to_savepoint(&mut self, name: &str) -> Result<()> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Cannot rollback savepoint in inactive transaction".to_string(),
            ));
        }
        self.conn
            .execute_simple(&format!("ROLLBACK TO SAVEPOINT {};", name))
            .await?;
        Ok(())
    }

    /// Releases a previous transaction savepoint.
    pub async fn release_savepoint(&mut self, name: &str) -> Result<()> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Cannot release savepoint in inactive transaction".to_string(),
            ));
        }
        self.conn
            .execute_simple(&format!("RELEASE SAVEPOINT {};", name))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl<'a> AsyncTransaction for PostgresTransaction<'a> {
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Cannot execute query on inactive transaction".to_string(),
            ));
        }

        let res = self.conn.execute(query, params).await?;
        // Accumulate mutation counters
        self.summary_accumulator.nodes_created += res.summary.nodes_created;
        self.summary_accumulator.nodes_deleted += res.summary.nodes_deleted;
        self.summary_accumulator.properties_set += res.summary.properties_set;
        self.summary_accumulator.execution_time_ms += res.summary.execution_time_ms;

        Ok(res)
    }

    async fn commit(mut self: Box<Self>) -> Result<QuerySummary> {
        if !self.is_active {
            return Err(NetError::TransactionError(
                "Transaction is already completed".to_string(),
            ));
        }
        self.conn.execute_simple("COMMIT;").await?;
        self.is_active = false;
        Ok(self.summary_accumulator.clone())
    }

    async fn rollback(mut self: Box<Self>) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }
        self.conn.execute_simple("ROLLBACK;").await?;
        self.is_active = false;
        Ok(())
    }
}
