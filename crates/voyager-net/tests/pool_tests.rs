//! Comprehensive unit and integration tests for voyager-net generic connection pool.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use voyager_net::config::PoolConfig;
use voyager_net::engine::{AsyncConnection, AsyncEngine};
use voyager_net::mock::{MockConnectionFactory, MockEngine};
use voyager_net::pool::ConnectionPool;

#[tokio::test]
async fn test_pool_warm_up_and_acquire() {
    let factory = Arc::new(MockConnectionFactory::new());
    let config = PoolConfig::new().with_min_idle(3).with_max_size(5);
    let pool = ConnectionPool::new(config, factory.clone());

    pool.warm_up().await.expect("Failed to warm up pool");
    assert_eq!(pool.idle_count(), 3);
    assert_eq!(pool.total_count(), 3);
    assert_eq!(pool.active_count(), 0);
    assert_eq!(factory.created_count(), 3);

    // Acquire one connection
    {
        let conn = pool.acquire().await.expect("Failed to acquire connection");
        assert_eq!(pool.idle_count(), 2);
        assert_eq!(pool.active_count(), 1);
        assert_eq!(pool.total_count(), 3);
        assert_eq!(factory.created_count(), 3); // Reused from idle queue, no new creation
        assert!(conn.is_valid());
    }

    // Connection returned to idle queue on drop
    assert_eq!(pool.idle_count(), 3);
    assert_eq!(pool.active_count(), 0);
}

#[tokio::test]
async fn test_pool_concurrency_and_exhaustion() {
    let factory = Arc::new(MockConnectionFactory::new());
    let config = PoolConfig::new()
        .with_max_size(2)
        .with_acquire_timeout(Duration::from_millis(50));
    let pool = ConnectionPool::new(config, factory.clone());

    // Lease max capacity (2 connections)
    let conn1 = pool.acquire().await.expect("Failed to acquire conn1");
    let conn2 = pool.acquire().await.expect("Failed to acquire conn2");

    assert_eq!(pool.active_count(), 2);
    assert_eq!(pool.idle_count(), 0);

    // 3rd acquire must timeout
    let err = pool.acquire().await.unwrap_err();
    match err {
        voyager_net::NetError::PoolExhausted(msg) => {
            assert!(msg.contains("Timed out waiting for connection"));
        }
        other => panic!("Expected PoolExhausted error, got: {:?}", other),
    }

    // Drop conn1 to free a slot
    drop(conn1);
    assert_eq!(pool.idle_count(), 1);
    assert_eq!(pool.active_count(), 1);

    // Now 3rd acquire succeeds
    let conn3 = pool
        .acquire()
        .await
        .expect("Failed to acquire conn3 after drop");
    assert_eq!(pool.active_count(), 2);

    drop(conn2);
    drop(conn3);
    assert_eq!(pool.idle_count(), 2);
    assert_eq!(pool.active_count(), 0);
}

#[tokio::test]
async fn test_pool_broken_connection_recycling() {
    let factory = Arc::new(MockConnectionFactory::new());
    let config = PoolConfig::new().with_max_size(3);
    let pool = ConnectionPool::new(config, factory.clone());

    // Acquire connection and mark it broken
    {
        let mut conn = pool.acquire().await.expect("Failed to acquire connection");
        assert_eq!(pool.total_count(), 1);
        conn.mark_broken();
    }

    // Since connection was broken, it should not be returned to idle queue, total count decremented
    assert_eq!(pool.idle_count(), 0);
    assert_eq!(pool.total_count(), 0);
    assert_eq!(pool.active_count(), 0);

    // Subsequent acquire should create a fresh connection
    {
        let _conn = pool
            .acquire()
            .await
            .expect("Failed to acquire fresh connection");
        assert_eq!(pool.total_count(), 1);
        assert_eq!(factory.created_count(), 2);
    }
    assert_eq!(pool.idle_count(), 1);
}

#[tokio::test]
async fn test_pool_liveness_probe_failure() {
    let factory = Arc::new(MockConnectionFactory::new());
    let config = PoolConfig::new().with_max_size(2);
    let pool = ConnectionPool::new(config, factory.clone());

    // Acquire and return healthy connection
    {
        let mut conn = pool.acquire().await.expect("Failed to acquire");
        // Simulate connection failing silently after return
        conn.fail_ping = true;
    }
    assert_eq!(pool.idle_count(), 1);

    // Next acquire will trigger liveness probe, fail, discard it, and create a fresh one
    {
        let conn = pool
            .acquire()
            .await
            .expect("Failed to acquire after failed probe");
        assert!(!conn.fail_ping);
        assert_eq!(factory.created_count(), 2);
    }
}

#[tokio::test]
async fn test_pool_idle_and_lifetime_eviction() {
    let factory = Arc::new(MockConnectionFactory::new());
    let config = PoolConfig::new()
        .with_max_size(5)
        .with_idle_timeout(Some(Duration::from_millis(20)))
        .with_max_lifetime(Some(Duration::from_millis(50)));
    let pool = ConnectionPool::new(config, factory.clone());

    // Create 2 idle connections
    pool.warm_up().await.expect("Warmup failed");
    assert_eq!(pool.idle_count(), 1); // min_idle default is 1

    // Sleep longer than idle timeout
    sleep(Duration::from_millis(30)).await;

    // Run eviction sweep
    pool.evict_expired().await;
    assert_eq!(pool.idle_count(), 0);
    assert_eq!(pool.total_count(), 0);
    assert_eq!(pool.metrics().connections_evicted, 1);
}

#[tokio::test]
async fn test_mock_engine_end_to_end() {
    let engine = MockEngine::new();

    let params = HashMap::new();
    let result = engine
        .execute("MATCH (n:Person) RETURN n.name", &params)
        .await
        .expect("Query execution failed");

    assert_eq!(result.row_count(), 0);
    assert!(!result.summary.is_mutating());

    engine.ping().await.expect("Engine ping failed");
}

#[tokio::test]
async fn test_pool_async_tx_rollback_on_drop() {
    let factory = Arc::new(MockConnectionFactory::new());
    let config = PoolConfig::new().with_max_size(2);
    let pool = ConnectionPool::new(config, factory.clone());

    let reset_counter;
    {
        let mut conn = pool.acquire().await.expect("Acquire failed");
        conn.in_transaction = true;
        reset_counter = conn.reset_count.clone();
        assert_eq!(reset_counter.load(std::sync::atomic::Ordering::SeqCst), 0);
        // conn drops here while in_transaction is true
    }

    // Give background tokio::spawn task time to run reset()
    sleep(Duration::from_millis(50)).await;

    // Verify reset() was invoked immediately in the background
    assert_eq!(reset_counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(pool.idle_count(), 1);

    // Re-acquire connection; verify transaction state was cleanly rolled back
    let conn2 = pool.acquire().await.expect("Re-acquire failed");
    assert!(!conn2.is_in_transaction());
}

#[tokio::test]
async fn test_pool_direct_waiter_handover() {
    let factory = Arc::new(MockConnectionFactory::new());
    // Pool capacity 1
    let config = PoolConfig::new()
        .with_max_size(1)
        .with_acquire_timeout(Duration::from_millis(500));
    let pool = ConnectionPool::new(config, factory.clone());

    let conn1 = pool.acquire().await.expect("First acquire failed");
    assert_eq!(pool.active_count(), 1);
    assert_eq!(pool.idle_count(), 0);

    let pool_clone = pool.clone();
    let waiter_handle = tokio::spawn(async move {
        // This will block waiting in the saturated direct waiter queue
        let conn_received = pool_clone.acquire().await.expect("Waiter acquire failed");
        assert!(conn_received.is_valid());
        // Verify no extra connection was created
        conn_received
    });

    // Small delay to ensure waiter task has entered the waiter queue
    sleep(Duration::from_millis(20)).await;

    // Drop conn1: this triggers direct handover to the waiter!
    drop(conn1);

    let conn_from_waiter = waiter_handle.await.expect("Waiter task panicked");
    assert_eq!(pool.active_count(), 1);
    assert_eq!(factory.created_count(), 1); // Exactly 1 connection created; handed over directly

    drop(conn_from_waiter);
    assert_eq!(pool.active_count(), 0);
    assert_eq!(pool.idle_count(), 1);
}
