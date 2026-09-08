//! Thread-safe generic asynchronous connection pool with RAII leasing, liveness probing, and idle eviction.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
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
struct PoolInner<C: AsyncConnection> {
    config: PoolConfig,
    factory: Arc<dyn ConnectionFactory<C>>,
    semaphore: Arc<Semaphore>,
    idle_queue: Mutex<VecDeque<IdleEntry<C>>>,
    total_connections: AtomicUsize,
    is_closed: AtomicBool,
    metrics: PoolMetrics,
}

/// Generic, thread-safe asynchronous connection pool.
#[derive(Clone)]
pub struct ConnectionPool<C: AsyncConnection + 'static> {
    inner: Arc<PoolInner<C>>,
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
        let permit = match timeout(
            acquire_timeout,
            self.inner.semaphore.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                return Err(NetError::Closed(
                    "Semaphore closed unexpectedly".to_string(),
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
        };

        // Try to obtain a valid idle connection from queue or create a fresh one
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

                // Reset state in case transaction was left open
                if idle.conn.is_in_transaction() {
                    let _ = idle.conn.reset().await;
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
                        drop(permit);
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
/// Automatically returns the connection to the idle queue when dropped, or discards it if marked broken.
pub struct PooledConnection<C: AsyncConnection + 'static> {
    conn: Option<C>,
    created_at: Instant,
    is_broken: bool,
    pool: Option<Arc<PoolInner<C>>>,
    _permit: Option<OwnedSemaphorePermit>,
}

impl<C: AsyncConnection + 'static> PooledConnection<C> {
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
        if let (Some(conn), Some(pool)) = (self.conn.take(), self.pool.take()) {
            if !self.is_broken && !pool.is_closed.load(Ordering::Relaxed) && conn.is_valid() {
                // Connection is healthy; return to idle queue
                let now = Instant::now();
                pool.idle_queue.lock().push_back(IdleEntry {
                    conn,
                    created_at: self.created_at,
                    last_used_at: now,
                });
            } else {
                // Connection is broken or pool closed; decrement total count
                pool.total_connections.fetch_sub(1, Ordering::SeqCst);
                // The connection will be dropped here
            }
        }
        // Dropping `_permit` returns the permit to the semaphore
    }
}
