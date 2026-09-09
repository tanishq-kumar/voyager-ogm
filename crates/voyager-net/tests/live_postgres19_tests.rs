//! Live integration test suite for PostgreSQL 19 Beta 3 over native wire protocol.

use std::collections::HashMap;
use voyager_net::postgres::{PostgresConnection, PostgresTransaction};
use voyager_net::{AsyncConnection, ParsedUri};

const PG19_URI: &str = "postgresql://postgres:voyagerpass123@localhost:5456/postgres";

async fn get_pg19_connection() -> Option<PostgresConnection> {
    let parsed = match ParsedUri::parse(PG19_URI) {
        Ok(p) => p,
        Err(_) => return None,
    };

    match PostgresConnection::connect(&parsed).await {
        Ok(conn) => Some(conn),
        Err(e) => {
            eprintln!(
                "Skipping live PostgreSQL 19 tests (container not accessible): {}",
                e
            );
            None
        }
    }
}

#[tokio::test]
async fn test_live_postgres19_ping_and_version() {
    let mut conn = match get_pg19_connection().await {
        Some(c) => c,
        None => return,
    };

    assert!(conn.ping().await.is_ok());
    assert!(conn.is_valid());
    assert!(!conn.is_in_transaction());

    let params = conn.server_parameters();
    let version = params.get("server_version").cloned().unwrap_or_default();
    println!("PostgreSQL 19 Live Server Version: {}", version);
    assert!(!version.is_empty());
}

#[tokio::test]
async fn test_live_postgres19_recursive_graph_and_arrow() {
    let mut conn = match get_pg19_connection().await {
        Some(c) => c,
        None => return,
    };

    // 1. Setup Table Schema & Hierarchy
    let _ = conn
        .execute_simple("DROP TABLE IF EXISTS net_categories;")
        .await;
    conn.execute_simple(
        "CREATE TABLE net_categories (id INT PRIMARY KEY, name TEXT, parent_id INT);",
    )
    .await
    .unwrap();

    let insert_sql = "INSERT INTO net_categories VALUES (1, 'Root', NULL), (2, 'Electronics', 1), (3, 'Laptops', 2), (4, 'Gaming Laptops', 3);";
    let insert_res = conn.execute_simple(insert_sql).await.unwrap();
    assert_eq!(insert_res.summary.nodes_created, 4);

    // 2. Query Hierarchy using Recursive CTE (Graph Traversal in SQL)
    let recursive_query = "
        WITH RECURSIVE cat_hierarchy AS (
            SELECT id, name, parent_id, 1 as depth
            FROM net_categories
            WHERE id = 1
            UNION ALL
            SELECT c.id, c.name, c.parent_id, ch.depth + 1
            FROM net_categories c
            JOIN cat_hierarchy ch ON c.parent_id = ch.id
        )
        SELECT id, name, parent_id, depth FROM cat_hierarchy ORDER BY depth;
    ";

    let result = conn.execute_simple(recursive_query).await.unwrap();
    assert_eq!(result.columns, vec!["id", "name", "parent_id", "depth"]);
    assert_eq!(result.row_count(), 4);

    let batch = &result.batches[0];
    assert_eq!(batch.num_columns(), 4);
    assert_eq!(batch.num_rows(), 4);

    let id_col = batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .unwrap();
    assert_eq!(id_col.value(0), 1);
    assert_eq!(id_col.value(1), 2);
    assert_eq!(id_col.value(2), 3);
    assert_eq!(id_col.value(3), 4);

    let name_col = batch
        .column(1)
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .unwrap();
    assert_eq!(name_col.value(0), "Root");
    assert_eq!(name_col.value(1), "Electronics");
    assert_eq!(name_col.value(2), "Laptops");
    assert_eq!(name_col.value(3), "Gaming Laptops");

    let depth_col = batch
        .column(3)
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .unwrap();
    assert_eq!(depth_col.value(0), 1);
    assert_eq!(depth_col.value(3), 4);

    // 3. Clean up
    let _ = conn.execute_simple("DROP TABLE net_categories;").await;
}

#[tokio::test]
async fn test_live_postgres19_transactions() {
    let mut conn = match get_pg19_connection().await {
        Some(c) => c,
        None => return,
    };

    let _ = conn
        .execute_simple("DROP TABLE IF EXISTS net_tx_test;")
        .await;
    conn.execute_simple("CREATE TABLE net_tx_test (id INT PRIMARY KEY, name TEXT);")
        .await
        .unwrap();

    // 1. Committed Transaction
    {
        let mut tx = PostgresTransaction::begin(&mut conn).await.unwrap();
        let _ = tx
            .execute(
                "INSERT INTO net_tx_test VALUES (1, 'Committed');",
                &HashMap::new(),
            )
            .await
            .unwrap();
        let _ = tx.commit().await.unwrap();
    }

    let check_committed = conn
        .execute_simple("SELECT id, name FROM net_tx_test WHERE id = 1;")
        .await
        .unwrap();
    assert_eq!(check_committed.row_count(), 1);

    // 2. Aborted / Rolled Back Transaction
    {
        let mut tx = PostgresTransaction::begin(&mut conn).await.unwrap();
        let _ = tx
            .execute(
                "INSERT INTO net_tx_test VALUES (2, 'Aborted');",
                &HashMap::new(),
            )
            .await
            .unwrap();
        tx.rollback().await.unwrap();
    }

    let check_aborted = conn
        .execute_simple("SELECT id, name FROM net_tx_test WHERE id = 2;")
        .await
        .unwrap();
    assert_eq!(check_aborted.row_count(), 0);

    // 3. Savepoints
    {
        let mut tx = PostgresTransaction::begin(&mut conn).await.unwrap();
        let _ = tx
            .execute(
                "INSERT INTO net_tx_test VALUES (3, 'Before SP');",
                &HashMap::new(),
            )
            .await
            .unwrap();
        tx.savepoint("sp1").await.unwrap();
        let _ = tx
            .execute(
                "INSERT INTO net_tx_test VALUES (4, 'Inside SP');",
                &HashMap::new(),
            )
            .await
            .unwrap();
        tx.rollback_to_savepoint("sp1").await.unwrap();
        let _ = tx.commit().await.unwrap();
    }

    let check_sp_kept = conn
        .execute_simple("SELECT id FROM net_tx_test WHERE id = 3;")
        .await
        .unwrap();
    assert_eq!(check_sp_kept.row_count(), 1);

    let check_sp_rolled = conn
        .execute_simple("SELECT id FROM net_tx_test WHERE id = 4;")
        .await
        .unwrap();
    assert_eq!(check_sp_rolled.row_count(), 0);

    // Clean up
    let _ = conn.execute_simple("DROP TABLE net_tx_test;").await;
}
