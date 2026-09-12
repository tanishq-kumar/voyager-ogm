//! Live integration tests for voyager-net BoltConnection against running Memgraph container on port 7688.

use std::collections::HashMap;

use voyager_net::bolt::BoltConnection;
use voyager_net::config::ConnectionConfig;
use voyager_net::engine::AsyncConnection;

const MEMGRAPH_URI: &str = "bolt://127.0.0.1:7688";

async fn is_memgraph_online() -> bool {
    let config = ConnectionConfig::from_uri(MEMGRAPH_URI);
    match BoltConnection::connect(&config).await {
        Ok(mut conn) => {
            let _ = conn.close().await;
            true
        }
        Err(e) => {
            eprintln!("[DEBUG ERROR CONNECTING TO MEMGRAPH]: {:?}", e);
            false
        }
    }
}

#[tokio::test]
async fn test_live_memgraph_bolt_crud_and_arrow() {
    if !is_memgraph_online().await {
        eprintln!("[SKIP] Live Memgraph instance is not reachable on 127.0.0.1:7688");
        return;
    }

    let config = ConnectionConfig::from_uri(MEMGRAPH_URI);
    let mut conn = BoltConnection::connect(&config)
        .await
        .expect("Failed to connect to live Memgraph");

    assert!(conn.is_valid());
    if let Some(agent) = conn.server_agent() {
        println!("Connected to live Memgraph agent: {}", agent);
    }

    // 1. Wipe test database
    let _ = conn
        .execute("MATCH (n:MemgraphTest) DETACH DELETE n", &HashMap::new())
        .await
        .expect("Failed to clean database");

    // 2. Insert nodes and relationship
    let mut create_params = HashMap::new();
    create_params.insert(
        "name1".to_string(),
        serde_json::Value::String("Charlie".to_string()),
    );
    create_params.insert("score".to_string(), serde_json::Value::Number(95.into()));

    let insert_query = "
        CREATE (a:MemgraphTest:Student {name: $name1, score: $score})
        RETURN a.name AS name, a.score AS score
    ";

    let insert_result = conn
        .execute(insert_query, &create_params)
        .await
        .expect("Failed to insert records in Memgraph");

    assert_eq!(insert_result.row_count(), 1);

    // 3. Query records with Arrow conversion
    let select_result = conn
        .execute(
            "MATCH (a:MemgraphTest:Student) RETURN a.name AS name, a.score AS score",
            &HashMap::new(),
        )
        .await
        .expect("Failed to query Memgraph");

    assert_eq!(select_result.row_count(), 1);
    assert_eq!(
        select_result.columns,
        vec!["name".to_string(), "score".to_string()]
    );

    let batch = select_result
        .into_single_batch()
        .unwrap()
        .expect("Missing Arrow RecordBatch");
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(batch.num_columns(), 2);

    // 4. Clean up
    let _ = conn
        .execute("MATCH (n:MemgraphTest) DETACH DELETE n", &HashMap::new())
        .await
        .expect("Failed to clean up Memgraph test records");

    conn.close()
        .await
        .expect("Failed to close Memgraph connection");
}
