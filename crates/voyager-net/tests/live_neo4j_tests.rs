//! Live integration tests for voyager-net BoltConnection against running Neo4j container.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;

use voyager_net::bolt::BoltConnection;
use voyager_net::config::{ConnectionConfig, PoolConfig};
use voyager_net::engine::{AsyncConnection, ConnectionFactory};
use voyager_net::pool::ConnectionPool;

const NEO4J_URI: &str = "bolt://127.0.0.1:7687";
const NEO4J_USER: &str = "neo4j";
const NEO4J_PASS: &str = "voyagerpass123";

async fn is_neo4j_online() -> bool {
    let config = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);
    match BoltConnection::connect(&config).await {
        Ok(mut conn) => {
            let _ = conn.close().await;
            true
        }
        Err(e) => {
            eprintln!("[DEBUG ERROR CONNECTING TO NEO4J]: {:?}", e);
            false
        }
    }
}

#[tokio::test]
async fn test_live_neo4j_bolt_crud_and_arrow() {
    if !is_neo4j_online().await {
        eprintln!("[SKIP] Live Neo4j instance is not reachable on 127.0.0.1:7687");
        return;
    }

    let config = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);
    let mut conn = BoltConnection::connect(&config)
        .await
        .expect("Failed to connect to live Neo4j");

    assert!(conn.is_valid());
    if let Some(agent) = conn.server_agent() {
        println!("Connected to live Neo4j agent: {}", agent);
    }

    // 1. Wipe test database
    let _ = conn
        .execute("MATCH (n:VoyagerTest) DETACH DELETE n", &HashMap::new())
        .await
        .expect("Failed to clean database");

    // 2. Insert nodes and relationship
    let mut create_params = HashMap::new();
    create_params.insert(
        "name1".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );
    create_params.insert("age1".to_string(), serde_json::Value::Number(30.into()));
    create_params.insert(
        "name2".to_string(),
        serde_json::Value::String("Bob".to_string()),
    );
    create_params.insert("age2".to_string(), serde_json::Value::Number(25.into()));
    create_params.insert("since".to_string(), serde_json::Value::Number(2022.into()));

    let insert_query = "
        CREATE (a:VoyagerTest:Person {name: $name1, age: $age1})
        CREATE (b:VoyagerTest:Person {name: $name2, age: $age2})
        CREATE (a)-[r:KNOWS {since: $since}]->(b)
        RETURN a.name AS a_name, b.name AS b_name
    ";

    let insert_result = conn
        .execute(insert_query, &create_params)
        .await
        .expect("Failed to insert live records");

    assert_eq!(insert_result.row_count(), 1);
    assert_eq!(insert_result.summary.nodes_created, 2);
    assert_eq!(insert_result.summary.relationships_created, 1);
    assert_eq!(insert_result.summary.properties_set, 5);

    // 3. Query records with Arrow conversion
    let mut query_params = HashMap::new();
    query_params.insert("min_age".to_string(), serde_json::Value::Number(20.into()));

    let select_query = "
        MATCH (a:VoyagerTest:Person)-[r:KNOWS]->(b:VoyagerTest:Person)
        WHERE a.age >= $min_age
        RETURN a.name AS sender, a.age AS sender_age, r.since AS since_year, b.name AS recipient
    ";

    let select_result = conn
        .execute(select_query, &query_params)
        .await
        .expect("Failed to select records");

    assert_eq!(select_result.row_count(), 1);
    assert_eq!(
        select_result.columns,
        vec![
            "sender".to_string(),
            "sender_age".to_string(),
            "since_year".to_string(),
            "recipient".to_string()
        ]
    );

    let batch = select_result
        .into_single_batch()
        .unwrap()
        .expect("Missing Arrow RecordBatch");
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(batch.num_columns(), 4);

    // 4. Clean up
    let _ = conn
        .execute("MATCH (n:VoyagerTest) DETACH DELETE n", &HashMap::new())
        .await
        .expect("Failed to clean up test records");

    conn.close().await.expect("Failed to close live connection");
}

/// Factory for pooled live connections.
struct LiveBoltFactory {
    config: ConnectionConfig,
}

#[async_trait::async_trait]
impl ConnectionFactory<BoltConnection<TcpStream>> for LiveBoltFactory {
    async fn create(&self) -> voyager_net::Result<BoltConnection<TcpStream>> {
        BoltConnection::connect(&self.config).await
    }
}

#[tokio::test]
async fn test_live_neo4j_connection_pool_concurrency() {
    if !is_neo4j_online().await {
        eprintln!("[SKIP] Live Neo4j instance is not reachable on 127.0.0.1:7687");
        return;
    }

    let config = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);
    let factory = Arc::new(LiveBoltFactory {
        config: config.clone(),
    });
    let pool_config = PoolConfig::new().with_min_idle(2).with_max_size(5);
    let pool = ConnectionPool::new(pool_config, factory);

    pool.warm_up().await.expect("Failed to warm up live pool");
    assert!(pool.idle_count() >= 2);

    let mut tasks = Vec::new();
    for i in 0..10 {
        let pool_clone = pool.clone();
        tasks.push(tokio::spawn(async move {
            let mut conn = pool_clone.acquire().await.expect("Live acquire failed");
            let mut params = HashMap::new();
            params.insert("val".to_string(), serde_json::Value::Number(i.into()));
            let res = conn
                .execute("RETURN $val AS res", &params)
                .await
                .expect("Query failed");
            assert_eq!(res.row_count(), 1);
        }));
    }

    for task in tasks {
        task.await.expect("Task failed");
    }

    assert_eq!(pool.active_count(), 0);
}

#[tokio::test]
async fn test_live_neo4j_fluent_builder_with_clause_and_pagination() {
    if !is_neo4j_online().await {
        eprintln!("[SKIP] Live Neo4j instance is not reachable on 127.0.0.1:7687");
        return;
    }

    use voyager_core::builder::QueryBuilder;
    use voyager_core::emitters::CypherEmitter;
    use voyager_core::visitor::AstVisitor;

    let config = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);
    let mut conn = BoltConnection::connect(&config)
        .await
        .expect("Failed to connect to live Neo4j");

    // Clean up
    let _ = conn
        .execute("MATCH (n:FluentNeo4jTest) DETACH DELETE n", &HashMap::new())
        .await;

    // Seed test records
    let seed_cypher = "CREATE (:FluentNeo4jTest:Person {name: 'Bob', age: 20}), (:FluentNeo4jTest:Person {name: 'David', age: 25}), (:FluentNeo4jTest:Person {name: 'Alice', age: 30}), (:FluentNeo4jTest:Person {name: 'Charlie', age: 40})";
    conn.execute(seed_cypher, &HashMap::new())
        .await
        .expect("Failed to seed records in Neo4j");

    // 1. Test WITH clause pipeline: MATCH -> WHERE -> WITH p -> WHERE -> RETURN
    let mut builder = QueryBuilder::new();
    builder
        .r#match()
        .node(Some("p"), vec!["FluentNeo4jTest", "Person"])
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
        .expect("Failed to execute WITH clause query on Neo4j");

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
        .node(Some("p"), vec!["FluentNeo4jTest", "Person"])
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
        .expect("Failed to execute pagination query on Neo4j");

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
        .execute("MATCH (n:FluentNeo4jTest) DETACH DELETE n", &HashMap::new())
        .await;

    conn.close().await.expect("Failed to close connection");
}
