//! In-memory mock connection and mock engine for zero-network testing and verification.

use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::config::PoolConfig;
use crate::engine::{AsyncConnection, AsyncEngine, ConnectionFactory, QueryResult, QuerySummary};
use crate::error::{NetError, Result};
use crate::pool::ConnectionPool;

/// Type alias for executed query logs in mock connections.
pub type ExecutedQueryLog = Arc<Mutex<Vec<(String, HashMap<String, serde_json::Value>)>>>;
/// Type alias for canned query result queues in mock connections.
pub type CannedResultQueue = Arc<Mutex<VecDeque<Result<QueryResult>>>>;

/// In-memory mock connection for unit testing connection lifecycles and query handling.
#[derive(Debug, Clone)]
pub struct MockConnection {
    /// Whether the mock socket is considered open and valid.
    pub is_valid: bool,
    /// Whether the connection is currently inside an active transaction.
    pub in_transaction: bool,
    /// If true, calls to `ping()` will return an error to simulate liveness failure.
    pub fail_ping: bool,
    /// If true, calls to `reset()` will return an error to simulate reset failure.
    pub fail_reset: bool,
    /// If true, calls to `execute()` will return an error to simulate query execution failure.
    pub fail_execute: bool,
    /// Shared log of executed queries and parameter maps.
    pub executed_queries: ExecutedQueryLog,
    /// Queue of canned query results to return sequentially on calls to `execute()`.
    pub canned_results: CannedResultQueue,
    /// Counter tracking number of times `reset()` was invoked.
    pub reset_count: Arc<AtomicUsize>,
}

impl MockConnection {
    /// Creates a fresh healthy mock connection.
    pub fn new() -> Self {
        Self {
            is_valid: true,
            in_transaction: false,
            fail_ping: false,
            fail_reset: false,
            fail_execute: false,
            executed_queries: Arc::new(Mutex::new(Vec::new())),
            canned_results: Arc::new(Mutex::new(VecDeque::new())),
            reset_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Returns the number of times `reset()` was called on this connection.
    pub fn reset_count(&self) -> usize {
        self.reset_count.load(Ordering::SeqCst)
    }

    /// Queues a canned query result to be returned on the next execution.
    pub fn push_canned_result(&self, result: Result<QueryResult>) {
        self.canned_results.lock().push_back(result);
    }

    /// Returns a snapshot of all executed queries.
    pub fn get_executed_queries(&self) -> Vec<(String, HashMap<String, serde_json::Value>)> {
        self.executed_queries.lock().clone()
    }
}

impl Default for MockConnection {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncConnection for MockConnection {
    async fn execute(
        &mut self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        if !self.is_valid {
            return Err(NetError::Closed("Mock connection is closed".to_string()));
        }
        if self.fail_execute {
            return Err(NetError::ProtocolError(
                "Simulated execution failure".to_string(),
            ));
        }

        self.executed_queries
            .lock()
            .push((query.to_string(), params.clone()));

        if let Some(canned) = self.canned_results.lock().pop_front() {
            canned
        } else {
            Ok(QueryResult::empty(QuerySummary::default()))
        }
    }

    async fn ping(&mut self) -> Result<()> {
        if !self.is_valid || self.fail_ping {
            Err(NetError::ConnectionFailed(
                "Mock connection ping failed".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    async fn reset(&mut self) -> Result<()> {
        self.reset_count.fetch_add(1, Ordering::SeqCst);
        if self.fail_reset {
            self.is_valid = false;
            return Err(NetError::ConnectionFailed(
                "Mock connection reset failed".to_string(),
            ));
        }
        self.in_transaction = false;
        Ok(())
    }

    fn is_valid(&self) -> bool {
        self.is_valid
    }

    fn is_in_transaction(&self) -> bool {
        self.in_transaction
    }

    async fn close(&mut self) -> Result<()> {
        self.is_valid = false;
        Ok(())
    }
}

/// Factory for generating `MockConnection` instances with configurable failure injection.
#[derive(Debug, Default)]
pub struct MockConnectionFactory {
    fail_create: AtomicBool,
    fail_ping_on_create: AtomicBool,
    created_count: AtomicUsize,
}

impl MockConnectionFactory {
    /// Creates a new mock factory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether subsequent calls to `create()` should return a connection error.
    pub fn set_fail_create(&self, fail: bool) {
        self.fail_create.store(fail, Ordering::SeqCst);
    }

    /// Sets whether newly created connections should fail their liveness ping check.
    pub fn set_fail_ping(&self, fail: bool) {
        self.fail_ping_on_create.store(fail, Ordering::SeqCst);
    }

    /// Returns the total number of connections created by this factory.
    pub fn created_count(&self) -> usize {
        self.created_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ConnectionFactory<MockConnection> for MockConnectionFactory {
    async fn create(&self) -> Result<MockConnection> {
        if self.fail_create.load(Ordering::SeqCst) {
            return Err(NetError::ConnectionFailed(
                "Mock factory simulated creation failure".to_string(),
            ));
        }

        self.created_count.fetch_add(1, Ordering::SeqCst);
        let mut conn = MockConnection::new();
        if self.fail_ping_on_create.load(Ordering::SeqCst) {
            conn.fail_ping = true;
        }
        Ok(conn)
    }
}

/// In-memory mock asynchronous database engine.
#[derive(Clone)]
pub struct MockEngine {
    pool: ConnectionPool<MockConnection>,
}

impl MockEngine {
    /// Creates a mock engine with default pool configuration.
    pub fn new() -> Self {
        let factory = Arc::new(MockConnectionFactory::new());
        let pool = ConnectionPool::new(PoolConfig::default(), factory);
        Self { pool }
    }

    /// Creates a mock engine with custom pool configuration and factory.
    pub fn with_pool(config: PoolConfig, factory: Arc<MockConnectionFactory>) -> Self {
        let pool = ConnectionPool::new(config, factory);
        Self { pool }
    }

    /// Returns a reference to the internal connection pool.
    pub fn pool(&self) -> &ConnectionPool<MockConnection> {
        &self.pool
    }
}

impl Default for MockEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncEngine for MockEngine {
    async fn execute(
        &self,
        query: &str,
        params: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult> {
        let mut conn = self.pool.acquire().await?;
        conn.execute(query, params).await
    }

    async fn ping(&self) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        conn.ping().await
    }
}
