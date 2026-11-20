//! Native transaction handle that leases one pooled connection for its whole lifetime.

use std::collections::HashMap;

use crate::engine::{AsyncConnection, QueryResult, QuerySummary};
use crate::error::{NetError, Result};
use crate::pool::PooledConnection;

/// Lifecycle state of a native transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTransactionState {
    /// The transaction accepts statements.
    Active,
    /// A statement failed; only rollback (or drop) is allowed.
    Failed,
    /// The transaction committed successfully.
    Committed,
    /// The transaction rolled back successfully.
    RolledBack,
}

impl NativeTransactionState {
    /// Returns the Python-facing lifecycle string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Failed => "FAILED",
            Self::Committed => "COMMITTED",
            Self::RolledBack => "ROLLED_BACK",
        }
    }
}

/// Explicit database transaction holding a single pooled connection.
///
/// The connection is leased when the transaction begins and is only returned to
/// the pool when the transaction is committed, rolled back, or dropped. This
/// guarantees that every statement issued inside the transaction runs on the
/// same physical connection, so reads observe the transaction's own writes.
///
/// A failed statement marks the transaction [`NativeTransactionState::Failed`].
/// From then on `execute` and `commit` fail closed with a clear "transaction is
/// aborted" error; only `rollback` (a no-op when the driver already aborted the
/// transaction) is allowed.
pub struct NativeTransaction<C: AsyncConnection + 'static> {
    conn: Option<PooledConnection<C>>,
    state: NativeTransactionState,
    supports_savepoints: bool,
    summary: QuerySummary,
}

impl<C: AsyncConnection + 'static> NativeTransaction<C> {
    /// Begins an explicit transaction on an already leased pooled connection.
    pub async fn begin(mut conn: PooledConnection<C>) -> Result<Self> {
        let supports_savepoints = conn.supports_savepoints();
        conn.begin_transaction().await?;
        Ok(Self {
            conn: Some(conn),
            state: NativeTransactionState::Active,
            supports_savepoints,
            summary: QuerySummary::empty(),
        })
    }

    /// Returns the current lifecycle state.
    pub fn state(&self) -> NativeTransactionState {
        self.state
    }

    /// Returns `true` while the transaction accepts statements.
    pub fn is_active(&self) -> bool {
        self.state == NativeTransactionState::Active
    }

    /// Returns `true` when this transaction supports real savepoints.
    pub fn supports_savepoints(&self) -> bool {
        self.supports_savepoints
    }

    /// Returns the accumulated mutation summary for the transaction.
    pub fn summary(&self) -> &QuerySummary {
        &self.summary
    }

    fn active_conn(&mut self) -> Result<&mut PooledConnection<C>> {
        match self.state {
            NativeTransactionState::Active => {}
            NativeTransactionState::Failed => {
                return Err(NetError::TransactionError(
                    "transaction is aborted; roll back before executing more statements"
                        .to_string(),
                ));
            }
            NativeTransactionState::Committed | NativeTransactionState::RolledBack => {
                return Err(NetError::TransactionError(
                    "Transaction is already completed".to_string(),
                ));
            }
        }
        self.conn.as_mut().ok_or_else(|| {
            NetError::TransactionError("Transaction has no leased connection".to_string())
        })
    }

    fn accumulate(&mut self, summary: &QuerySummary) {
        self.summary.nodes_created += summary.nodes_created;
        self.summary.nodes_deleted += summary.nodes_deleted;
        self.summary.relationships_created += summary.relationships_created;
        self.summary.relationships_deleted += summary.relationships_deleted;
        self.summary.properties_set += summary.properties_set;
        self.summary.labels_added += summary.labels_added;
        self.summary.labels_removed += summary.labels_removed;
        self.summary.indexes_added += summary.indexes_added;
        self.summary.constraints_added += summary.constraints_added;
        self.summary.execution_time_ms += summary.execution_time_ms;
    }

    /// Executes a parameterized statement inside the transaction.
    pub async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        let conn = self.active_conn()?;
        match conn.execute(query, params).await {
            Ok(res) => {
                self.accumulate(&res.summary);
                Ok(res)
            }
            Err(err) => {
                // Any statement error aborts the transaction on every backend:
                // Bolt RESET aborts the explicit transaction and PostgreSQL
                // enters FailedTransaction. Never allow a later autocommit.
                self.state = NativeTransactionState::Failed;
                Err(err)
            }
        }
    }

    /// Commits the transaction and returns its accumulated mutation summary.
    pub async fn commit(&mut self) -> Result<QuerySummary> {
        match self.state {
            NativeTransactionState::Failed => {
                return Err(NetError::TransactionError(
                    "transaction is aborted; roll back instead of committing".to_string(),
                ));
            }
            NativeTransactionState::Active => {}
            NativeTransactionState::Committed | NativeTransactionState::RolledBack => {
                return Err(NetError::TransactionError(
                    "Transaction is already completed".to_string(),
                ));
            }
        }
        let mut conn = self.conn.take().ok_or_else(|| {
            NetError::TransactionError("Transaction has no leased connection".to_string())
        })?;
        match conn.commit_transaction().await {
            Ok(()) => {
                self.state = NativeTransactionState::Committed;
                drop(conn);
                Ok(self.summary.clone())
            }
            Err(err) => {
                self.state = NativeTransactionState::Failed;
                self.conn = Some(conn);
                Err(err)
            }
        }
    }

    /// Rolls the transaction back and releases its pooled connection.
    pub async fn rollback(&mut self) -> Result<()> {
        match self.state {
            NativeTransactionState::Active | NativeTransactionState::Failed => {}
            NativeTransactionState::Committed | NativeTransactionState::RolledBack => {
                return Err(NetError::TransactionError(
                    "Transaction is already completed".to_string(),
                ));
            }
        }
        let mut conn = self.conn.take().ok_or_else(|| {
            NetError::TransactionError("Transaction has no leased connection".to_string())
        })?;
        // A failed Bolt transaction was already aborted by RESET, so there is
        // no server-side transaction left to roll back. PostgreSQL keeps the
        // failed transaction open and does accept ROLLBACK.
        let result = if conn.can_rollback() {
            conn.rollback_transaction().await
        } else {
            Ok(())
        };
        match result {
            Ok(()) => {
                self.state = NativeTransactionState::RolledBack;
                drop(conn);
                Ok(())
            }
            Err(err) => {
                self.state = NativeTransactionState::Failed;
                self.conn = Some(conn);
                Err(err)
            }
        }
    }

    /// Creates a named savepoint inside the transaction.
    pub async fn savepoint(&mut self, name: &str) -> Result<()> {
        if !self.supports_savepoints {
            return Err(NetError::TransactionError(format!(
                "Savepoints are not supported by this backend; cannot create savepoint '{name}'"
            )));
        }
        let conn = self.active_conn()?;
        match conn.create_savepoint(name).await {
            Ok(()) => Ok(()),
            Err(err) => {
                self.state = NativeTransactionState::Failed;
                Err(err)
            }
        }
    }

    /// Rolls the transaction back to a named savepoint.
    pub async fn rollback_to_savepoint(&mut self, name: &str) -> Result<()> {
        if !self.supports_savepoints {
            return Err(NetError::TransactionError(format!(
                "Savepoints are not supported by this backend; cannot roll back to '{name}'"
            )));
        }
        let conn = self.active_conn()?;
        match conn.rollback_to_savepoint(name).await {
            Ok(()) => Ok(()),
            Err(err) => {
                self.state = NativeTransactionState::Failed;
                Err(err)
            }
        }
    }

    /// Releases a named savepoint from the transaction.
    pub async fn release_savepoint(&mut self, name: &str) -> Result<()> {
        if !self.supports_savepoints {
            return Err(NetError::TransactionError(format!(
                "Savepoints are not supported by this backend; cannot release '{name}'"
            )));
        }
        let conn = self.active_conn()?;
        match conn.release_savepoint(name).await {
            Ok(()) => Ok(()),
            Err(err) => {
                self.state = NativeTransactionState::Failed;
                Err(err)
            }
        }
    }
}

impl<C: AsyncConnection + 'static> Drop for NativeTransaction<C> {
    fn drop(&mut self) {
        // Dropping an open transaction must deterministically end the server
        // transaction. We cannot await here, so mark the pooled connection
        // broken: the pool discards the physical socket instead of RESET/reuse,
        // which makes the server abort the transaction when the socket closes.
        let should_discard = self.conn.as_ref().is_some_and(|conn| match self.state {
            NativeTransactionState::Active => true,
            NativeTransactionState::Failed => conn.can_rollback(),
            NativeTransactionState::Committed | NativeTransactionState::RolledBack => false,
        });
        if should_discard && let Some(conn) = self.conn.as_mut() {
            conn.mark_broken();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoolConfig;
    use crate::mock::MockConnectionFactory;
    use crate::pool::ConnectionPool;
    use std::sync::Arc;
    use std::time::Duration;

    type MockConnectionType = crate::mock::MockConnection;

    fn pool_config() -> PoolConfig {
        PoolConfig {
            max_size: 1,
            acquire_timeout: Duration::from_secs(1),
            ..PoolConfig::default()
        }
    }

    async fn pool() -> (
        ConnectionPool<MockConnectionType>,
        Arc<MockConnectionFactory>,
    ) {
        let factory = Arc::new(MockConnectionFactory::new());
        let pool = ConnectionPool::new(pool_config(), factory.clone());
        (pool, factory)
    }

    #[tokio::test]
    async fn native_transaction_holds_one_connection_commit_and_rollback() {
        let (pool, factory) = pool().await;
        let conn = pool.acquire().await.expect("acquire connection");
        let mut tx = NativeTransaction::begin(conn).await.expect("begin");
        assert!(tx.is_active());
        assert_eq!(tx.state().as_str(), "ACTIVE");
        assert_eq!(pool.active_count(), 1);
        assert_eq!(pool.idle_count(), 0);

        tx.execute("CREATE (n)", &HashMap::new())
            .await
            .expect("execute inside tx");
        assert_eq!(pool.active_count(), 1);
        assert!(
            factory.row_count() == 0,
            "uncommitted write must not be visible"
        );
        assert!(tx.commit().await.is_ok());
        assert!(!tx.is_active());
        assert_eq!(tx.state().as_str(), "COMMITTED");
        assert_eq!(pool.active_count(), 0);
        assert_eq!(factory.row_count(), 1);

        let conn = pool.acquire().await.expect("reacquire connection");
        let mut tx = NativeTransaction::begin(conn).await.expect("begin again");
        tx.execute("CREATE (n)", &HashMap::new())
            .await
            .expect("execute inside tx");
        tx.rollback().await.expect("rollback");
        assert!(!tx.is_active());
        assert_eq!(tx.state().as_str(), "ROLLED_BACK");
        assert_eq!(pool.active_count(), 0);
        assert_eq!(factory.row_count(), 1);
    }

    #[tokio::test]
    async fn native_transaction_fails_closed_after_statement_error() {
        let (pool, factory) = pool().await;
        let conn = pool.acquire().await.expect("acquire connection");
        let mut tx = NativeTransaction::begin(conn).await.expect("begin");
        tx.execute("CREATE (n)", &HashMap::new())
            .await
            .expect("execute inside tx");

        tx.conn.as_mut().expect("leased connection").fail_execute = true;
        let err = tx
            .execute("CREATE (n)", &HashMap::new())
            .await
            .expect_err("failing statement must return an error");
        assert!(err.to_string().contains("Simulated execution failure"));
        assert_eq!(tx.state().as_str(), "FAILED");
        assert!(!tx.is_active());

        let err = tx
            .execute("CREATE (n)", &HashMap::new())
            .await
            .expect_err("writes after failure must be rejected");
        assert!(err.to_string().contains("aborted"));

        let err = tx
            .commit()
            .await
            .expect_err("commit after failure must fail");
        assert!(err.to_string().contains("aborted"));
        assert_eq!(tx.state().as_str(), "FAILED");

        tx.rollback().await.expect("rollback after failure");
        assert_eq!(tx.state().as_str(), "ROLLED_BACK");
        assert_eq!(factory.row_count(), 0);
    }

    #[tokio::test]
    async fn native_transaction_drop_discards_open_server_transaction() {
        let (pool, factory) = pool().await;
        let conn = pool.acquire().await.expect("acquire connection");
        let mut tx = NativeTransaction::begin(conn).await.expect("begin");
        tx.execute("CREATE (n)", &HashMap::new())
            .await
            .expect("execute inside tx");
        assert!(!tx.conn.as_ref().expect("leased connection").is_broken());

        drop(tx);
        assert_eq!(pool.active_count(), 0);
        assert_eq!(factory.row_count(), 0, "dropped write must be rolled back");

        let conn = pool.acquire().await.expect("reacquire after drop");
        assert!(!conn.is_in_transaction());
        assert_eq!(conn.row_count(), 0);
    }

    #[tokio::test]
    async fn native_transaction_savepoints_follow_backend_support() {
        let (pool, _) = pool().await;
        let conn = pool.acquire().await.expect("acquire connection");
        let mut tx = NativeTransaction::begin(conn).await.expect("begin");
        assert!(tx.supports_savepoints());

        tx.savepoint("sp_1").await.expect("create savepoint");
        tx.rollback_to_savepoint("sp_1")
            .await
            .expect("rollback to savepoint");
        tx.release_savepoint("sp_1")
            .await
            .expect("release savepoint");
        tx.commit().await.expect("commit");
    }
}
