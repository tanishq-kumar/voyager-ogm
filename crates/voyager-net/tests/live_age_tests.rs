use std::collections::HashMap;
use voyager_net::postgres::{AgeValue, PostgresConnection, PostgresTransaction, parse_agtype};
use voyager_net::{AsyncConnection, ParsedUri};

const AGE_URI: &str = "age://postgres:voyagerpass123@localhost:5455/postgres?graph=live_age_test";

async fn get_age_connection() -> Option<PostgresConnection> {
    let parsed = match ParsedUri::parse(AGE_URI) {
        Ok(p) => p,
        Err(_) => return None,
    };

    match PostgresConnection::connect(&parsed).await {
        Ok(conn) => Some(conn),
        Err(e) => {
            eprintln!("Skipping live AGE tests (container not accessible): {}", e);
            None
        }
    }
}

#[tokio::test]
async fn test_live_age_ping_and_parameters() {
    let mut conn = match get_age_connection().await {
        Some(c) => c,
        None => return,
    };

    assert!(conn.ping().await.is_ok());
    assert!(conn.is_valid());
    assert!(!conn.is_in_transaction());

    let params = conn.server_parameters();
    assert!(params.contains_key("server_version"));
    assert!(params.contains_key("client_encoding"));
}

#[tokio::test]
async fn test_live_age_graph_lifecycle_and_cypher() {
    let mut conn = match get_age_connection().await {
        Some(c) => c,
        None => return,
    };

    let graph_name = "net_test_graph";

    // Clean up graph if already existing
    let _ = conn
        .execute_simple(&format!(
            "SELECT * FROM ag_catalog.drop_graph('{}', true);",
            graph_name
        ))
        .await;

    // 1. Create Graph
    let create_res = conn
        .execute_simple(&format!(
            "SELECT * FROM ag_catalog.create_graph('{}');",
            graph_name
        ))
        .await;
    assert!(
        create_res.is_ok(),
        "Failed to create graph: {:?}",
        create_res
    );

    // 2. Create Vertices via Cypher
    let create_nodes_sql = format!(
        "SELECT * FROM cypher('{}', $$ CREATE (a:Person {{name: 'Alice', age: 30}}), (b:Person {{name: 'Bob', age: 35}}) $$) as (v agtype);",
        graph_name
    );
    let create_nodes_res = conn.execute_simple(&create_nodes_sql).await;
    assert!(
        create_nodes_res.is_ok(),
        "Failed to create vertices: {:?}",
        create_nodes_res
    );

    // 3. Create Edge via Cypher
    let create_edge_sql = format!(
        "SELECT * FROM cypher('{}', $$ MATCH (a:Person {{name: 'Alice'}}), (b:Person {{name: 'Bob'}}) CREATE (a)-[r:KNOWS {{since: 2022}}]->(b) $$) as (v agtype);",
        graph_name
    );
    let create_edge_res = conn.execute_simple(&create_edge_sql).await;
    assert!(
        create_edge_res.is_ok(),
        "Failed to create edge: {:?}",
        create_edge_res
    );

    // 4. Query Vertices and Parse agtype
    let query_vertices_sql = format!(
        "SELECT * FROM cypher('{}', $$ MATCH (p:Person) RETURN p ORDER BY p.name $$) as (p agtype);",
        graph_name
    );
    let query_res = conn.execute_simple(&query_vertices_sql).await.unwrap();
    assert_eq!(query_res.columns, vec!["p".to_string()]);
    assert_eq!(query_res.row_count(), 2);

    let batch = &query_res.batches[0];
    let string_array = batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .unwrap();

    let p1_raw = string_array.value(0);
    let p2_raw = string_array.value(1);

    let p1_val = parse_agtype(p1_raw).unwrap();
    let p2_val = parse_agtype(p2_raw).unwrap();

    if let AgeValue::Vertex(v1) = p1_val {
        assert_eq!(v1.label, "Person");
        assert_eq!(v1.properties.get("name").unwrap(), "Alice");
        assert_eq!(v1.properties.get("age").unwrap(), 30);
    } else {
        panic!("Expected AgeValue::Vertex for p1");
    }

    if let AgeValue::Vertex(v2) = p2_val {
        assert_eq!(v2.label, "Person");
        assert_eq!(v2.properties.get("name").unwrap(), "Bob");
        assert_eq!(v2.properties.get("age").unwrap(), 35);
    } else {
        panic!("Expected AgeValue::Vertex for p2");
    }

    // 5. Query Scalar Projections (Arrow Column Types)
    let query_scalars_sql = format!(
        "SELECT * FROM cypher('{}', $$ MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN a.name, r.since, b.name $$) as (a_name agtype, since agtype, b_name agtype);",
        graph_name
    );
    let scalar_res = conn.execute_simple(&query_scalars_sql).await.unwrap();
    assert_eq!(scalar_res.columns.len(), 3);
    assert_eq!(scalar_res.row_count(), 1);

    // 6. Query Path
    let query_path_sql = format!(
        "SELECT * FROM cypher('{}', $$ MATCH p = (a:Person)-[:KNOWS]->(b:Person) RETURN p $$) as (p agtype);",
        graph_name
    );
    let path_res = conn.execute_simple(&query_path_sql).await.unwrap();
    assert_eq!(path_res.row_count(), 1);
    let path_batch = &path_res.batches[0];
    let path_str_col = path_batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .unwrap();
    let path_raw = path_str_col.value(0);
    let path_val = parse_agtype(path_raw).unwrap();

    if let AgeValue::Path(path) = path_val {
        assert_eq!(path.vertices.len(), 2);
        assert_eq!(path.edges.len(), 1);
        assert_eq!(path.edges[0].label, "KNOWS");

        let bolt_path = path.to_bolt_path();
        assert_eq!(bolt_path.nodes.len(), 2);
        assert_eq!(bolt_path.relationships.len(), 1);
    } else {
        panic!("Expected AgeValue::Path");
    }

    // 7. Teardown / Drop Graph
    let drop_res = conn
        .execute_simple(&format!(
            "SELECT * FROM ag_catalog.drop_graph('{}', true);",
            graph_name
        ))
        .await;
    assert!(drop_res.is_ok());
}

#[tokio::test]
async fn test_live_age_transaction_rollback() {
    let mut conn = match get_age_connection().await {
        Some(c) => c,
        None => return,
    };

    let graph_name = "net_tx_graph";
    let _ = conn
        .execute_simple(&format!(
            "SELECT * FROM ag_catalog.drop_graph('{}', true);",
            graph_name
        ))
        .await;
    let _ = conn
        .execute_simple(&format!(
            "SELECT * FROM ag_catalog.create_graph('{}');",
            graph_name
        ))
        .await;

    // Start transaction, create vertex, and roll back
    {
        let mut tx = PostgresTransaction::begin(&mut conn).await.unwrap();
        let create_sql = format!(
            "SELECT * FROM cypher('{}', $$ CREATE (:Person {{name: 'Ghost'}}) $$) as (v agtype);",
            graph_name
        );
        let _ = tx.execute(&create_sql, &HashMap::new()).await.unwrap();
        tx.rollback().await.unwrap();
    }

    // Verify node does not exist
    let verify_sql = format!(
        "SELECT * FROM cypher('{}', $$ MATCH (p:Person {{name: 'Ghost'}}) RETURN p $$) as (p agtype);",
        graph_name
    );
    let res = conn.execute_simple(&verify_sql).await.unwrap();
    assert_eq!(res.row_count(), 0);

    // Clean up
    let _ = conn
        .execute_simple(&format!(
            "SELECT * FROM ag_catalog.drop_graph('{}', true);",
            graph_name
        ))
        .await;
}
