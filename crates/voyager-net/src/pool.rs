//! Thread-safe generic asynchronous connection pool with RAII leasing, liveness probing, and idle eviction.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::config::PoolConfig;
use crate::engine::{AsyncConnection, ConnectionFactory, FnConnectionFactory};
use crate::error::{NetError, Result};

/// Idle connection wrapper with lifecycle timestamps.
struct IdleEntry<C: AsyncConnection> {
    conn: C,
    created_at: Instant,
    last_used_at: Instant,
}

/// Direct handover payload transferred to a waiting checkout task under saturation.
enum Handover<C: AsyncConnection + 'static> {
    /// A ready-to-use pooled connection with its active lease permit.
    Connection(PooledConnection<C>),
    /// A released permit allowing the waiter to establish a replacement connection.
    Permit(OwnedSemaphorePermit),
}

type WaiterSender<C> = oneshot::Sender<Handover<C>>;

/// Real-time metrics counters for connection pool telemetry.
#[derive(Debug, Default)]
pub struct PoolMetrics {
    acquire_count: AtomicU64,
    acquire_timeout_count: AtomicU64,
    connections_created: AtomicU64,
    connections_evicted: AtomicU64,
    connections_failed_health_check: AtomicU64,
}

/// Snapshot of pool telemetry metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolMetricsSnapshot {
    /// Total number of checkout acquire requests.
    pub acquire_count: u64,
    /// Total number of acquire requests that timed out.
    pub acquire_timeout_count: u64,
    /// Total number of new physical connections created.
    pub connections_created: u64,
    /// Total number of connections evicted due to idle timeout or lifetime expiry.
    pub connections_evicted: u64,
    /// Total number of connections discarded during liveness health checks.
    pub connections_failed_health_check: u64,
}

/// Shared internal state for `ConnectionPool`.
struct PoolInner<C: AsyncConnection + 'static> {
    config: PoolConfig,
    factory: Arc<dyn ConnectionFactory<C>>,
    semaphore: Arc<Semaphore>,
    idle_queue: Mutex<VecDeque<IdleEntry<C>>>,
    waiters: Mutex<VecDeque<WaiterSender<C>>>,
    total_connections: AtomicUsize,
    is_closed: AtomicBool,
    metrics: PoolMetrics,
}

impl<C: AsyncConnection + 'static> PoolInner<C> {
    /// Returns a connection to the pool, handing off directly to a waiter if one is waiting,
    /// or storing it in the idle queue.
    fn return_connection(
        self: &Arc<Self>,
        conn: C,
        created_at: Instant,
        permit: Option<OwnedSemaphorePermit>,
    ) {
        if self.is_closed.load(Ordering::Relaxed) || !conn.is_valid() {
            self.total_connections.fetch_sub(1, Ordering::SeqCst);
            self.handle_permit_return(permit);
            return;
        }

        let mut waiters = self.waiters.lock();
        let mut current_conn = Some(conn);
        let mut current_permit = permit;

        while let Some(tx) = waiters.pop_front() {
            if !tx.is_closed() {
                let pooled = PooledConnection {
                    conn: current_conn.take(),
                    created_at,
                    is_broken: false,
                    pool: Some(self.clone()),
                    _permit: current_permit.take(),
                };
                match tx.send(Handover::Connection(pooled)) {
                    Ok(()) => return,
                    Err(Handover::Connection(returned_pooled)) => {
                        let (c, p) = returned_pooled.into_parts();
                        current_conn = c;
                        current_permit = p;
                    }
                    _ => unreachable!(),
                }
            }
        }
        drop(waiters);

        if let Some(conn) = current_conn {
            let now = Instant::now();
            self.idle_queue.lock().push_back(IdleEntry {
                conn,
                created_at,
                last_used_at: now,
            });
        }
        drop(current_permit);
    }

    /// Handles returning a permit back to waiters or dropping it to replenish the semaphore.
    fn handle_permit_return(&self, mut permit: Option<OwnedSemaphorePermit>) {
        if let Some(p) = permit.take() {
            let mut waiters = self.waiters.lock();
            let mut current_permit = p;
            while let Some(tx) = waiters.pop_front() {
                if !tx.is_closed() {
                    match tx.send(Handover::Permit(current_permit)) {
                        Ok(()) => return,
                        Err(Handover::Permit(ret_p)) => {
                            current_permit = ret_p;
                        }
                        _ => unreachable!(),
                    }
                }
            }
            drop(current_permit);
        }
    }
}

/// Generic, thread-safe asynchronous connection pool.
pub struct ConnectionPool<C: AsyncConnection + 'static> {
    inner: Arc<PoolInner<C>>,
}

impl<C: AsyncConnection + 'static> Clone for ConnectionPool<C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<C: AsyncConnection + 'static> ConnectionPool<C> {
    /// Creates a new `ConnectionPool` with the given configuration and connection factory.
    pub fn new(config: PoolConfig, factory: Arc<dyn ConnectionFactory<C>>) -> Self {
        let max_size = config.max_size.max(1);
        let semaphore = Arc::new(Semaphore::new(max_size));
        let inner = Arc::new(PoolInner {
            config,
            factory,
            semaphore,
            idle_queue: Mutex::new(VecDeque::new()),
            waiters: Mutex::new(VecDeque::new()),
            total_connections: AtomicUsize::new(0),
            is_closed: AtomicBool::new(false),
            metrics: PoolMetrics::default(),
        });

        Self { inner }
    }

    /// Creates a new `ConnectionPool` using an asynchronous factory closure.
    pub fn from_fn<F, Fut>(config: PoolConfig, factory_fn: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<C>> + Send + 'static,
    {
        let factory = Arc::new(FnConnectionFactory::new(factory_fn));
        Self::new(config, factory)
    }

    /// Warms up the pool by pre-allocating connections up to `min_idle`.
    pub async fn warm_up(&self) -> Result<()> {
        let target = self.inner.config.min_idle.min(self.inner.config.max_size);
        for _ in 0..target {
            if self.inner.is_closed.load(Ordering::Relaxed) {
                break;
            }
            let conn = self.inner.factory.create().await?;
            self.inner
                .metrics
                .connections_created
                .fetch_add(1, Ordering::Relaxed);
            self.inner.total_connections.fetch_add(1, Ordering::SeqCst);
            let now = Instant::now();
            self.inner.idle_queue.lock().push_back(IdleEntry {
                conn,
                created_at: now,
                last_used_at: now,
            });
        }
        Ok(())
    }

    /// Acquires a connection from the pool, waiting up to `acquire_timeout`.
    pub async fn acquire(&self) -> Result<PooledConnection<C>> {
        self.inner
            .metrics
            .acquire_count
            .fetch_add(1, Ordering::Relaxed);

        if self.inner.is_closed.load(Ordering::Relaxed) {
            return Err(NetError::Closed(
                "Connection pool has been closed".to_string(),
            ));
        }

        let acquire_timeout = self.inner.config.acquire_timeout;

        // 1. Fast-path: non-blocking permit acquire
        let permit = match self.inner.semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                // 2. Contention path: pool is saturated. Register in direct waiter queue.
                let (tx, rx) = tokio::sync::oneshot::channel();
                {
                    let mut waiters = self.inner.waiters.lock();
                    // Double check if permit became available right before acquiring waiters lock
                    match self.inner.semaphore.clone().try_acquire_owned() {
                        Ok(permit) => {
                            drop(waiters);
                            return self.obtain_with_permit(permit).await;
                        }
                        Err(_) => {
                            waiters.push_back(tx);
                        }
                    }
                }

                // Await direct handover with timeout
                match timeout(acquire_timeout, rx).await {
                    Ok(Ok(Handover::Connection(pooled))) => {
                        return Ok(pooled);
                    }
                    Ok(Ok(Handover::Permit(permit))) => {
                        return self.obtain_with_permit(permit).await;
                    }
                    Ok(Err(_)) => {
                        return Err(NetError::Closed(
                            "Connection pool was closed while waiting for connection".to_string(),
                        ));
                    }
                    Err(_) => {
                        self.inner
                            .metrics
                            .acquire_timeout_count
                            .fetch_add(1, Ordering::Relaxed);
                        return Err(NetError::PoolExhausted(format!(
                            "Timed out waiting for connection after {:?} (active: {}, idle: {}, max: {})",
                            acquire_timeout,
                            self.active_count(),
                            self.idle_count(),
                            self.inner.config.max_size
                        )));
                    }
                }
            }
        };

        // 3. We have a permit from the fast path
        self.obtain_with_permit(permit).await
    }

    /// Obtains a pooled connection using an already acquired semaphore permit.
    async fn obtain_with_permit(
        &self,
        permit: OwnedSemaphorePermit,
    ) -> Result<PooledConnection<C>> {
        loop {
            if self.inner.is_closed.load(Ordering::Relaxed) {
                drop(permit);
                return Err(NetError::Closed(
                    "Connection pool has been closed".to_string(),
                ));
            }

            let maybe_idle = self.inner.idle_queue.lock().pop_front();
            if let Some(mut idle) = maybe_idle {
                let now = Instant::now();

                // Check max lifetime
                if self
                    .inner
                    .config
                    .max_lifetime
                    .is_some_and(|max| now.duration_since(idle.created_at) > max)
                {
                    debug!("Evicting connection exceeding max lifetime");
                    self.inner
                        .metrics
                        .connections_evicted
                        .fetch_add(1, Ordering::Relaxed);
                    self.inner.total_connections.fetch_sub(1, Ordering::SeqCst);
                    let _ = idle.conn.close().await;
                    continue;
                }

                // Check idle timeout
                if self
                    .inner
                    .config
                    .idle_timeout
                    .is_some_and(|to| now.duration_since(idle.last_used_at) > to)
                {
                    debug!("Evicting idle connection exceeding idle timeout");
                    self.inner
                        .metrics
                        .connections_evicted
                        .fetch_add(1, Ordering::Relaxed);
                    self.inner.total_connections.fetch_sub(1, Ordering::SeqCst);
                    let _ = idle.conn.close().await;
                    continue;
                }

                // Check liveness probe if test_on_acquire is enabled
                if self.inner.config.test_on_acquire
                    && (!idle.conn.is_valid() || idle.conn.ping().await.is_err())
                {
                    warn!("Connection failed liveness probe, discarding");
                    self.inner
                        .metrics
                        .connections_failed_health_check
                        .fetch_add(1, Ordering::Relaxed);
                    self.inner.total_connections.fetch_sub(1, Ordering::SeqCst);
                    let _ = idle.conn.close().await;
                    continue;
                }

                // Reset state in case transaction was left open, verifying reset succeeded and connection is valid
                if idle.conn.is_in_transaction() {
                    if idle.conn.reset().await.is_err() || !idle.conn.is_valid() {
                        warn!("Connection failed transaction reset or is invalid, discarding");
                        self.inner
                            .metrics
                            .connections_failed_health_check
                            .fetch_add(1, Ordering::Relaxed);
                        self.inner.total_connections.fetch_sub(1, Ordering::SeqCst);
                        let _ = idle.conn.close().await;
                        continue;
                    }
                } else if !idle.conn.is_valid() {
                    warn!("Connection is marked invalid, discarding");
                    self.inner
                        .metrics
                        .connections_failed_health_check
                        .fetch_add(1, Ordering::Relaxed);
                    self.inner.total_connections.fetch_sub(1, Ordering::SeqCst);
                    let _ = idle.conn.close().await;
                    continue;
                }

                return Ok(PooledConnection {
                    conn: Some(idle.conn),
                    created_at: idle.created_at,
                    is_broken: false,
                    pool: Some(self.inner.clone()),
                    _permit: Some(permit),
                });
            } else {
                // No idle connections in queue, create a fresh connection
                match self.inner.factory.create().await {
                    Ok(conn) => {
                        self.inner
                            .metrics
                            .connections_created
                            .fetch_add(1, Ordering::Relaxed);
                        self.inner.total_connections.fetch_add(1, Ordering::SeqCst);
                        return Ok(PooledConnection {
                            conn: Some(conn),
                            created_at: Instant::now(),
                            is_broken: false,
                            pool: Some(self.inner.clone()),
                            _permit: Some(permit),
                        });
                    }
                    Err(e) => {
                        self.inner.handle_permit_return(Some(permit));
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Performs an idle eviction and max lifetime sweep on all idle connections in the pool.
    pub async fn evict_expired(&self) {
        let now = Instant::now();
        let idle_timeout = self.inner.config.idle_timeout;
        let max_lifetime = self.inner.config.max_lifetime;

        let mut to_close = Vec::new();
        {
            let mut queue = self.inner.idle_queue.lock();
            let mut retained = VecDeque::with_capacity(queue.len());
            while let Some(entry) = queue.pop_front() {
                let expired_idle =
                    idle_timeout.is_some_and(|to| now.duration_since(entry.last_used_at) > to);
                let expired_life =
                    max_lifetime.is_some_and(|to| now.duration_since(entry.created_at) > to);

                if expired_idle || expired_life {
                    self.inner
                        .metrics
                        .connections_evicted
                        .fetch_add(1, Ordering::Relaxed);
                    self.inner.total_connections.fetch_sub(1, Ordering::SeqCst);
                    to_close.push(entry.conn);
                } else {
                    retained.push_back(entry);
                }
            }
            *queue = retained;
        }

        for mut conn in to_close {
            let _ = conn.close().await;
        }
    }

    /// Returns the number of currently checked-out active connections.
    pub fn active_count(&self) -> usize {
        let max = self.inner.config.max_size;
        let available = self.inner.semaphore.available_permits();
        max.saturating_sub(available)
    }

    /// Returns the number of idle connections sitting in the pool queue.
    pub fn idle_count(&self) -> usize {
        self.inner.idle_queue.lock().len()
    }

    /// Returns the total number of managed connections (active + idle).
    pub fn total_count(&self) -> usize {
        self.inner.total_connections.load(Ordering::Relaxed)
    }

    /// Returns the configured maximum capacity of the pool.
    pub fn max_size(&self) -> usize {
        self.inner.config.max_size
    }

    /// Returns a snapshot of real-time telemetry metrics.
    pub fn metrics(&self) -> PoolMetricsSnapshot {
        PoolMetricsSnapshot {
            acquire_count: self.inner.metrics.acquire_count.load(Ordering::Relaxed),
            acquire_timeout_count: self
                .inner
                .metrics
                .acquire_timeout_count
                .load(Ordering::Relaxed),
            connections_created: self
                .inner
                .metrics
                .connections_created
                .load(Ordering::Relaxed),
            connections_evicted: self
                .inner
                .metrics
                .connections_evicted
                .load(Ordering::Relaxed),
            connections_failed_health_check: self
                .inner
                .metrics
                .connections_failed_health_check
                .load(Ordering::Relaxed),
        }
    }

    /// Gracefully closes all idle connections and marks the pool as closed.
    pub async fn close(&self) {
        self.inner.is_closed.store(true, Ordering::SeqCst);
        let waiters: Vec<_> = self.inner.waiters.lock().drain(..).collect();
        drop(waiters);

        let mut to_close = Vec::new();
        {
            let mut queue = self.inner.idle_queue.lock();
            while let Some(entry) = queue.pop_front() {
                self.inner.total_connections.fetch_sub(1, Ordering::SeqCst);
                to_close.push(entry.conn);
            }
        }
        for mut conn in to_close {
            let _ = conn.close().await;
        }
    }
}

/// RAII leasing guard for a pooled database connection.
///
/// Automatically returns the connection to the idle queue or waiting acquirers when dropped.
pub struct PooledConnection<C: AsyncConnection + 'static> {
    conn: Option<C>,
    created_at: Instant,
    is_broken: bool,
    pool: Option<Arc<PoolInner<C>>>,
    _permit: Option<OwnedSemaphorePermit>,
}

impl<C: AsyncConnection + 'static> PooledConnection<C> {
    /// Disassembles the pooled connection into its raw parts without triggering RAII drop.
    pub(crate) fn into_parts(mut self) -> (Option<C>, Option<OwnedSemaphorePermit>) {
        let conn = self.conn.take();
        let permit = self._permit.take();
        self.pool = None;
        (conn, permit)
    }

    /// Marks the connection as unrecoverable / broken, preventing it from being returned to the idle pool.
    pub fn mark_broken(&mut self) {
        self.is_broken = true;
    }

    /// Returns `true` if the connection has been marked broken.
    pub fn is_broken(&self) -> bool {
        self.is_broken
    }

    /// Returns the duration since this physical connection was established.
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

impl<C: AsyncConnection + 'static> std::fmt::Debug for PooledConnection<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledConnection")
            .field("age", &self.age())
            .field("is_broken", &self.is_broken)
            .finish()
    }
}

impl<C: AsyncConnection + 'static> Deref for PooledConnection<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("PooledConnection dereferenced after take")
    }
}

impl<C: AsyncConnection + 'static> DerefMut for PooledConnection<C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
            .as_mut()
            .expect("PooledConnection dereferenced after take")
    }
}

impl<C: AsyncConnection + 'static> Drop for PooledConnection<C> {
    fn drop(&mut self) {
        let conn = self.conn.take();
        let pool = self.pool.take();
        let permit = self._permit.take();

        if let (Some(mut conn), Some(pool)) = (conn, pool) {
            let is_broken =
                self.is_broken || pool.is_closed.load(Ordering::Relaxed) || !conn.is_valid();

            if is_broken {
                // Connection is broken or pool closed; decrement total count and hand permit to waiter or drop
                pool.total_connections.fetch_sub(1, Ordering::SeqCst);
                pool.handle_permit_return(permit);
            } else if conn.is_in_transaction() {
                // Connection was left in an active transaction!
                // Spawn background async task to ROLL BACK immediately on the server so entity locks are freed now.
                let created_at = self.created_at;
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    let pool_clone = pool.clone();
                    handle.spawn(async move {
                        debug!("PooledConnection dropped with active transaction; rolling back asynchronously");
                        let reset_result = timeout(Duration::from_secs(2), conn.reset()).await;
                        if let Ok(Ok(())) = reset_result
                            && conn.is_valid()
                            && !pool_clone.is_closed.load(Ordering::Relaxed)
                        {
                            pool_clone.return_connection(conn, created_at, permit);
                            return;
                        }
                        warn!("Failed to reset transaction on dropped connection; closing connection");
                        pool_clone.total_connections.fetch_sub(1, Ordering::SeqCst);
                        let _ = conn.close().await;
                        pool_clone.handle_permit_return(permit);
                    });
                } else {
                    pool.total_connections.fetch_sub(1, Ordering::SeqCst);
                    pool.handle_permit_return(permit);
                }
            } else {
                // Connection is clean and healthy; return directly or hand off to waiter
                pool.return_connection(conn, self.created_at, permit);
            }
        }
    }
}
