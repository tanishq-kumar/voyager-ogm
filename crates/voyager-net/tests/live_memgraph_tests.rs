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

#[tokio::test]
async fn test_live_memgraph_fluent_builder_with_clause_and_pagination() {
    if !is_memgraph_online().await {
        eprintln!("[SKIP] Live Memgraph instance is not reachable on 127.0.0.1:7688");
        return;
    }

    use voyager_core::builder::QueryBuilder;
    use voyager_core::emitters::CypherEmitter;
    use voyager_core::visitor::AstVisitor;

    let config = ConnectionConfig::from_uri(MEMGRAPH_URI);
    let mut conn = BoltConnection::connect(&config)
        .await
        .expect("Failed to connect to live Memgraph");

    // Clean up
    let _ = conn
        .execute(
            "MATCH (n:FluentMemgraphTest) DETACH DELETE n",
            &HashMap::new(),
        )
        .await;

    // Seed test records
    let seed_cypher = "CREATE (:FluentMemgraphTest:Person {name: 'Bob', age: 20}), (:FluentMemgraphTest:Person {name: 'David', age: 25}), (:FluentMemgraphTest:Person {name: 'Alice', age: 30}), (:FluentMemgraphTest:Person {name: 'Charlie', age: 40})";
    conn.execute(seed_cypher, &HashMap::new())
        .await
        .expect("Failed to seed records in Memgraph");

    // 1. Test WITH clause pipeline: MATCH -> WHERE -> WITH p -> WHERE -> RETURN
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["FluentMemgraphTest", "Person"])
        .where_gte("p", "age", 20)
        .r#with()
        .field("p", "", None::<&str>)
        .where_gt("p", "age", 25)
        .r#return()
        .field("p", "name", None::<&str>)
        .field("p", "age", None::<&str>)
        .order_by_asc("p", "age");

    let (arena, root) = builder.build();
    let mut emitter = CypherEmitter::new();
    let compiled = emitter
        .visit_query(&arena, root)
        .expect("Cypher emission failed");

    let mut query_params = HashMap::new();
    for (k, v) in &compiled.parameters {
        if let voyager_core::ast::LiteralValue::Int64(i) = v {
            query_params.insert(k.clone(), serde_json::json!(i));
        }
    }

    let res = conn
        .execute(&compiled.statement, &query_params)
        .await
        .expect("Failed to execute WITH clause query on Memgraph");

    assert_eq!(res.row_count(), 2);
    let batch = res
        .into_single_batch()
        .unwrap()
        .expect("Missing Arrow RecordBatch");
    assert_eq!(batch.num_rows(), 2);

    let name_col = batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .unwrap();
    let age_col = batch
        .column(1)
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .unwrap();
    assert_eq!(name_col.value(0), "Alice");
    assert_eq!(age_col.value(0), 30);
    assert_eq!(name_col.value(1), "Charlie");
    assert_eq!(age_col.value(1), 40);

    // 2. Test pagination: offset / skip and limit
    let mut builder2 = QueryBuilder::new();
    builder2
        .r#match()
        .node(Some("p"), vec!["FluentMemgraphTest", "Person"])
        .r#return()
        .field("p", "name", None::<&str>)
        .order_by_asc("p", "age")
        .skip(1)
        .limit(2);

    let (arena2, root2) = builder2.build();
    let mut emitter2 = CypherEmitter::new();
    let compiled2 = emitter2
        .visit_query(&arena2, root2)
        .expect("Cypher emission failed");

    let res2 = conn
        .execute(&compiled2.statement, &HashMap::new())
        .await
        .expect("Failed to execute pagination query on Memgraph");

    assert_eq!(res2.row_count(), 2);
    let batch2 = res2
        .into_single_batch()
        .unwrap()
        .expect("Missing Arrow RecordBatch");
    let name_col2 = batch2
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .unwrap();
    assert_eq!(name_col2.value(0), "David");
    assert_eq!(name_col2.value(1), "Alice");

    // Clean up
    let _ = conn
        .execute(
            "MATCH (n:FluentMemgraphTest) DETACH DELETE n",
            &HashMap::new(),
        )
        .await;

    conn.close().await.expect("Failed to close connection");
}
