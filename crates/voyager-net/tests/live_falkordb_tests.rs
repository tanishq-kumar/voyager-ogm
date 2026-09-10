//! Live integration tests for voyager-net RedisConnection against running FalkorDB / Valkey container.

use arrow::array::{Float64Array, Int64Array, StringArray};
use std::collections::HashMap;

use voyager_net::engine::AsyncConnection;
use voyager_net::redis::RedisConnection;
use voyager_net::uri::ParsedUri;

const BASE_FALKORDB_ADDR: &str = "redis://127.0.0.1:6379";

async fn is_falkordb_online() -> bool {
    let uri = ParsedUri::parse(BASE_FALKORDB_ADDR).unwrap();
    match RedisConnection::connect(&uri).await {
        Ok(mut conn) => {
            let _ = conn.close().await;
            true
        }
        Err(_) => false,
    }
}

#[tokio::test]
async fn test_live_falkordb_handshake_and_resp3() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let uri = ParsedUri::parse(BASE_FALKORDB_ADDR).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    assert!(conn.is_valid());
    assert!(conn.is_resp3(), "Expected FalkorDB to negotiate RESP3");
    assert!(!conn.server_info().is_empty());

    // Verify ping works over the negotiated connection
    conn.ping().await.expect("PING probe failed");

    conn.close().await.expect("Failed to close connection");
    assert!(!conn.is_valid());
}

#[tokio::test]
async fn test_live_falkordb_crud_and_arrow_materialization() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "test_falkor_crud";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    // 1. Clean previous state
    let _ = conn.graph_delete(graph_name).await;

    // 2. CREATE nodes and edge
    let create_cypher = "CREATE (a:Developer {name: 'Ada', age: 36}), \
                                (b:Developer {name: 'Alan', age: 41}), \
                                (a)-[:COLLABORATES {weight: 9.85}]->(b)";
    let create_res = conn
        .graph_query(graph_name, create_cypher, &HashMap::new())
        .await
        .expect("Failed to execute CREATE query");

    assert_eq!(create_res.statistics.nodes_created, 2);
    assert_eq!(create_res.statistics.relationships_created, 1);
    assert_eq!(create_res.statistics.properties_set, 5);

    // 3. Query graph and verify Arrow RecordBatch materialization
    let query_cypher = "MATCH (a:Developer)-[r:COLLABORATES]->(b:Developer) \
                        RETURN a.name, a.age, r.weight, b.name";
    let query_res = conn
        .graph_query(graph_name, query_cypher, &HashMap::new())
        .await
        .expect("Failed to execute MATCH query");

    let batch = query_res
        .to_record_batch()
        .expect("Failed to materialize Arrow RecordBatch");

    assert_eq!(batch.num_columns(), 4);
    assert_eq!(batch.num_rows(), 1);

    // Column 0: a.name (StringArray)
    let a_name = batch
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(a_name.value(0), "Ada");

    // Column 1: a.age (Int64Array)
    let a_age = batch
        .column(1)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(a_age.value(0), 36);

    // Column 2: r.weight (Float64Array)
    let r_weight = batch
        .column(2)
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    assert!((r_weight.value(0) - 9.85).abs() < 1e-4);

    // Column 3: b.name (StringArray)
    let b_name = batch
        .column(3)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(b_name.value(0), "Alan");

    // 4. Clean up
    let _ = conn.graph_delete(graph_name).await;
    conn.close().await.expect("Failed to close connection");
}

#[tokio::test]
async fn test_live_falkordb_parameterized_and_ro_query() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "test_falkor_params";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    let _ = conn.graph_delete(graph_name).await;

    // Seed test data
    conn.graph_query(
        graph_name,
        "CREATE (:Developer {name: 'Ada', age: 36}), (:Developer {name: 'Alan', age: 41})",
        &HashMap::new(),
    )
    .await
    .expect("Failed to seed nodes");

    // Parameterized MATCH
    let mut params = HashMap::new();
    params.insert("target_name".to_string(), serde_json::json!("Ada"));

    let res = conn
        .graph_query(
            graph_name,
            "MATCH (d:Developer {name: $target_name}) RETURN d.age",
            &params,
        )
        .await
        .expect("Failed to execute parameterized query");

    let batch = res.to_record_batch().unwrap();
    assert_eq!(batch.num_rows(), 1);
    let age_col = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(age_col.value(0), 36);

    // Read-only query (GRAPH.RO_QUERY)
    let ro_res = conn
        .graph_ro_query(
            graph_name,
            "MATCH (d:Developer) RETURN count(d)",
            &HashMap::new(),
        )
        .await
        .expect("Failed to execute read-only query");

    assert_eq!(ro_res.rows.len(), 1);
    let count_val = ro_res.rows[0][0].as_i64().unwrap();
    assert_eq!(count_val, 2);

    let _ = conn.graph_delete(graph_name).await;
    conn.close().await.unwrap();
}

#[tokio::test]
async fn test_live_falkordb_transactions_commit_and_rollback() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "test_falkor_tx";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    let _ = conn.graph_delete(graph_name).await;

    // 1. Transaction Commit
    let mut tx = conn
        .begin_transaction()
        .await
        .expect("Failed to begin transaction");
    assert!(tx.is_active());

    tx.execute("CREATE (n:TxNode {id: 101})", &HashMap::new())
        .await
        .expect("Failed to queue query 1");
    tx.execute("CREATE (n:TxNode {id: 102})", &HashMap::new())
        .await
        .expect("Failed to queue query 2");

    let summary = tx.commit().await.expect("Failed to commit transaction");
    assert!(!conn.is_in_transaction());
    assert_eq!(summary.nodes_created, 2);

    // Verify nodes are present
    let check = conn
        .graph_query(
            graph_name,
            "MATCH (n:TxNode) RETURN count(n)",
            &HashMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(check.rows[0][0].as_i64().unwrap(), 2);

    // 2. Transaction Rollback (DISCARD)
    let mut tx2 = conn
        .begin_transaction()
        .await
        .expect("Failed to begin transaction 2");
    assert!(tx2.is_active());

    tx2.execute("CREATE (n:RollbackNode {id: 999})", &HashMap::new())
        .await
        .expect("Failed to queue query in tx2");

    tx2.rollback()
        .await
        .expect("Failed to rollback transaction");
    assert!(!conn.is_in_transaction());

    // Verify rollback node does not exist
    let check2 = conn
        .graph_query(
            graph_name,
            "MATCH (n:RollbackNode) RETURN count(n)",
            &HashMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(check2.rows[0][0].as_i64().unwrap(), 0);

    let _ = conn.graph_delete(graph_name).await;
    conn.close().await.unwrap();
}

#[tokio::test]
async fn test_live_falkordb_async_connection_trait() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "test_falkor_async";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn: Box<dyn AsyncConnection> = Box::new(
        RedisConnection::connect(&uri)
            .await
            .expect("Failed to connect to FalkorDB"),
    );

    assert!(conn.is_valid());
    assert!(!conn.is_in_transaction());

    conn.ping().await.expect("PING failed");
    conn.reset().await.expect("RESET failed");

    // Clean any prior state in graph
    let _ = conn
        .execute("MATCH (n) DETACH DELETE n", &HashMap::new())
        .await;

    // Seed test nodes via execute
    conn.execute(
        "CREATE (:DevUser {name: 'Grace'}), (:DevUser {name: 'Linus'})",
        &HashMap::new(),
    )
    .await
    .expect("Execute CREATE failed");

    let res = conn
        .execute("MATCH (d:DevUser) RETURN d.name", &HashMap::new())
        .await
        .expect("Execute MATCH via AsyncConnection failed");

    assert_eq!(res.batches[0].num_columns(), 1);
    assert_eq!(res.batches[0].num_rows(), 2);

    let _ = conn
        .execute("MATCH (n) DETACH DELETE n", &HashMap::new())
        .await;

    conn.close().await.expect("CLOSE failed");
    assert!(!conn.is_valid());
}

#[tokio::test]
async fn test_live_valkey_uri_compatibility() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    // Connect using valkey:// URI scheme
    let uri = ParsedUri::parse("valkey://127.0.0.1:6379?graph=test_live_valkey").unwrap();
    assert_eq!(uri.protocol, voyager_net::DatabaseProtocol::Redis);

    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect via valkey:// URI");

    assert!(conn.is_valid());
    conn.ping().await.expect("PING failed over valkey://");

    let res = conn
        .graph_query("test_live_valkey", "RETURN 42", &HashMap::new())
        .await
        .expect("Failed to execute query over valkey://");

    assert_eq!(res.rows[0][0].as_i64().unwrap(), 42);

    conn.close().await.unwrap();
}
