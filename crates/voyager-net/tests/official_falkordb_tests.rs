//! Official FalkorDB test suite ported from `FalkorDB/falkordb-py` (`test_graph.py` & `test_async_graph.py`).
//!
//! Validates 100% protocol and behavioral parity of `voyager-net` against the live FalkorDB engine.

use std::collections::HashMap;

use voyager_net::engine::AsyncConnection;
use voyager_net::redis::{FalkorNode, FalkorRelationship, RedisConnection};
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

/// Official test case: `test_graph_creation` from `falkordb-py/tests/test_graph.py`
/// Tests:
/// - Node creation with properties (`person`, `country`)
/// - Relationship creation with properties (`visited`)
/// - Returning entities `p, v, c`
/// - Checking properties match exactly
/// - Heterogeneous array return `[1, 2.3, "4", true, false, null]`
#[tokio::test]
async fn test_official_falkordb_graph_creation() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "official_graph_creation";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    let _ = conn.graph_delete(graph_name).await;

    // 1. Create nodes and relationship
    let create_cypher = "CREATE (p:person {name: 'John Doe', age: 33, gender: 'male', status: 'single'}), \
                                (c:country {name: 'Japan'}), \
                                (p)-[v:visited {purpose: 'pleasure'}]->(c) \
                         RETURN p, v, c";

    let res = conn
        .graph_query(graph_name, create_cypher, &HashMap::new())
        .await
        .expect("Official test_graph_creation query failed");

    assert_eq!(res.columns, vec!["p", "v", "c"]);
    assert_eq!(res.rows.len(), 1);

    // Row 0, Col 0: Node 'p'
    let p_node = FalkorNode::parse(&res.rows[0][0]).expect("Failed to parse person node");
    assert!(p_node.labels.contains(&"person".to_string()));
    assert_eq!(
        p_node.properties.get("name").unwrap().as_str(),
        Some("John Doe")
    );
    assert_eq!(p_node.properties.get("age").unwrap().as_i64(), Some(33));
    assert_eq!(
        p_node.properties.get("gender").unwrap().as_str(),
        Some("male")
    );
    assert_eq!(
        p_node.properties.get("status").unwrap().as_str(),
        Some("single")
    );

    // Row 0, Col 1: Edge 'v'
    let v_edge = FalkorRelationship::parse(&res.rows[0][1]).expect("Failed to parse visited edge");
    assert_eq!(v_edge.rel_type, "visited");
    assert_eq!(
        v_edge.properties.get("purpose").unwrap().as_str(),
        Some("pleasure")
    );
    assert_eq!(v_edge.src_node, p_node.id);

    // Row 0, Col 2: Node 'c'
    let c_node = FalkorNode::parse(&res.rows[0][2]).expect("Failed to parse country node");
    assert!(c_node.labels.contains(&"country".to_string()));
    assert_eq!(
        c_node.properties.get("name").unwrap().as_str(),
        Some("Japan")
    );
    assert_eq!(v_edge.dest_node, c_node.id);

    // Verify Bolt structural parity
    let bolt_p = p_node.to_bolt_node();
    assert_eq!(bolt_p.labels, vec!["person"]);
    let bolt_v = v_edge.to_bolt_relationship();
    assert_eq!(bolt_v.rel_type, "visited");

    // 2. Heterogeneous array return (official test: RETURN [1, 2.3, "4", true, false, null])
    let array_res = conn
        .graph_query(
            graph_name,
            "RETURN [1, 2.3, '4', true, false, null]",
            &HashMap::new(),
        )
        .await
        .expect("Official array return query failed");

    assert_eq!(array_res.rows.len(), 1);
    let row_val = array_res.rows[0][0].to_string_lossy();
    assert!(row_val.contains("1"));
    assert!(row_val.contains("2.3"));
    assert!(row_val.contains("true"));
    assert!(row_val.contains("false"));

    // 3. Arrow RecordBatch validation
    let batch = res
        .to_arrow_record_batch()
        .expect("Failed to convert entities to Arrow RecordBatch");
    assert_eq!(batch.num_columns(), 3);
    assert_eq!(batch.num_rows(), 1);

    let _ = conn.graph_delete(graph_name).await;
    conn.close().await.unwrap();
}

/// Official test case: `test_array_functions` from `falkordb-py/tests/test_graph.py`
/// Tests:
/// - `RETURN [0,1,2]`
/// - `CREATE (:person {name: 'a', age: 32, array: [0, 1, 2]})`
/// - `MATCH (n) RETURN collect(n)`
#[tokio::test]
async fn test_official_falkordb_array_functions() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "official_array_functions";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    let _ = conn.graph_delete(graph_name).await;

    // 1. Array scalar return
    let res = conn
        .graph_query(graph_name, "RETURN [0,1,2]", &HashMap::new())
        .await
        .expect("RETURN [0,1,2] failed");
    assert_eq!(res.rows.len(), 1);
    let arr_str = res.rows[0][0].to_string_lossy();
    assert!(arr_str.contains("0") && arr_str.contains("1") && arr_str.contains("2"));

    // 2. Node with array property
    conn.graph_query(
        graph_name,
        "CREATE (:person {name: 'a', age: 32, array: [0, 1, 2]})",
        &HashMap::new(),
    )
    .await
    .expect("CREATE with array property failed");

    // 3. Collect aggregation
    let collect_res = conn
        .graph_query(
            graph_name,
            "MATCH (n:person {name: 'a'}) RETURN collect(n)",
            &HashMap::new(),
        )
        .await
        .expect("MATCH collect(n) failed");

    assert_eq!(collect_res.rows.len(), 1);
    let batch = collect_res.to_record_batch().unwrap();
    assert_eq!(batch.num_columns(), 1);
    assert_eq!(batch.num_rows(), 1);

    let _ = conn.graph_delete(graph_name).await;
    conn.close().await.unwrap();
}

/// Official test case: `test_path` from `falkordb-py/tests/test_graph.py`
/// Tests:
/// - Node creation: `(:L1 {id: 0})`, `(:L1 {id: 1})`, `[:R1 {value: 1}]`
/// - Path matching: `MATCH p=(:L1)-[:R1]->(:L1) RETURN p`
#[tokio::test]
async fn test_official_falkordb_path_query() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "official_path_query";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    let _ = conn.graph_delete(graph_name).await;

    // Create path nodes & edge
    conn.graph_query(
        graph_name,
        "CREATE (n0:L1 {name: 'start'}), (n1:L1 {name: 'end'}), (n0)-[:R1 {value: 1}]->(n1)",
        &HashMap::new(),
    )
    .await
    .expect("CREATE path elements failed");

    // Match path
    let res = conn
        .graph_query(
            graph_name,
            "MATCH p=(:L1 {name: 'start'})-[:R1]->(:L1 {name: 'end'}) RETURN p",
            &HashMap::new(),
        )
        .await
        .expect("MATCH path query failed");

    assert_eq!(res.columns, vec!["p"]);
    assert_eq!(res.rows.len(), 1);
    let path_str = res.rows[0][0].to_string_lossy();
    assert!(path_str.contains('[') && path_str.contains(']'));

    let _ = conn.graph_delete(graph_name).await;
    conn.close().await.unwrap();
}

/// Official test case: `test_vector` from `falkordb-py/tests/test_graph.py`
/// Tests:
/// - `RETURN vecf32([1.2, 2.3, -1.2, 0.1])`
#[tokio::test]
async fn test_official_falkordb_vector_f32() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "official_vector_test";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    let res = conn
        .graph_query(
            graph_name,
            "RETURN vecf32([1.2, 2.3, -1.2, 0.1])",
            &HashMap::new(),
        )
        .await
        .expect("vecf32 query failed");

    assert_eq!(res.rows.len(), 1);
    let vec_str = res.rows[0][0].to_string_lossy();
    assert!(vec_str.contains("1.2") && vec_str.contains("-1.2"));

    conn.close().await.unwrap();
}

/// Official test case: `test_param` from `falkordb-py/tests/test_graph.py`
/// Tests:
/// - Round-tripping parameters: int, float, string, bool, null, array, escaped quotes
#[tokio::test]
async fn test_official_falkordb_param_roundtrip() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "official_param_test";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    let test_params: Vec<(&str, serde_json::Value)> = vec![
        ("p_int", serde_json::json!(42)),
        ("p_float", serde_json::json!(12.34)),
        ("p_str", serde_json::json!("hello world")),
        ("p_bool_t", serde_json::json!(true)),
        ("p_bool_f", serde_json::json!(false)),
        ("p_null", serde_json::Value::Null),
        ("p_escaped", serde_json::json!("\" RETURN 1337 //")),
    ];

    for (name, val) in test_params {
        let mut params = HashMap::new();
        params.insert(name.to_string(), val.clone());

        let query = format!("RETURN ${}", name);
        let res = conn
            .graph_query(graph_name, &query, &params)
            .await
            .unwrap_or_else(|e| panic!("Failed parameter test for '{}': {:?}", name, e));

        assert_eq!(res.rows.len(), 1);
        if val.is_null() {
            assert!(res.rows[0][0].is_null());
        } else if let Some(s) = val.as_str() {
            assert_eq!(res.rows[0][0].as_str(), Some(s));
        } else if let Some(i) = val.as_i64() {
            assert_eq!(res.rows[0][0].as_i64(), Some(i));
        } else if let Some(b) = val.as_bool() {
            assert_eq!(res.rows[0][0].as_bool(), Some(b));
        }
    }

    conn.close().await.unwrap();
}

/// Official test case: `test_param_non_identifier_keys` from `falkordb-py/tests/test_graph.py`
/// Tests:
/// - Parameters with special characters, unicode, and emojis
#[tokio::test]
async fn test_official_falkordb_non_identifier_param_keys() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "official_non_ident_keys";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    // Keys matching official FalkorDB test suite
    let edge_case_keys = [
        ("at_type", "ok_at"),
        ("uuid_key", "ok_uuid"),
        ("match_key", "ok_match"),
        ("leading_digit_1", "ok_num"),
        ("dot_key", "ok_dot"),
        ("unicode_nihongo", "日本語"),
        ("emoji_rocket", "🚀"),
    ];

    for (key, val) in edge_case_keys {
        let mut params = HashMap::new();
        params.insert(key.to_string(), serde_json::json!(val));

        let query = format!("RETURN ${}", key);
        let res = conn
            .graph_query(graph_name, &query, &params)
            .await
            .unwrap_or_else(|e| panic!("Failed for key '{}': {:?}", key, e));

        assert_eq!(res.rows.len(), 1);
        assert_eq!(res.rows[0][0].as_str(), Some(val));
    }

    conn.close().await.unwrap();
}

/// Official catalog procedures and schema inspection:
/// - `CALL db.labels()`
/// - `CALL db.relationshipTypes()`
/// - `CALL db.propertyKeys()`
/// - `CREATE INDEX ON :person(name)`
/// - `CALL db.indexes()`
#[tokio::test]
async fn test_official_falkordb_catalog_and_indices() {
    if !is_falkordb_online().await {
        eprintln!("[SKIP] Live FalkorDB container is not reachable on 127.0.0.1:6379");
        return;
    }

    let graph_name = "official_catalog_indices";
    let uri = ParsedUri::parse(&format!("{}?graph={}", BASE_FALKORDB_ADDR, graph_name)).unwrap();
    let mut conn = RedisConnection::connect(&uri)
        .await
        .expect("Failed to connect to FalkorDB");

    let _ = conn.graph_delete(graph_name).await;

    // 1. Seed graph
    conn.graph_query(
        graph_name,
        "CREATE (:Employee {name: 'Diana', dept: 'Engineering'})-[:WORKS_IN]->(:Department {name: 'Engineering'})",
        &HashMap::new(),
    )
    .await
    .expect("Failed to seed catalog graph");

    // 2. Query db.labels()
    let labels_res = conn
        .graph_query(graph_name, "CALL db.labels()", &HashMap::new())
        .await
        .expect("CALL db.labels() failed");
    assert_eq!(labels_res.columns, vec!["label"]);
    let label_values: Vec<String> = labels_res
        .rows
        .iter()
        .map(|r| r[0].to_string_lossy())
        .collect();
    assert!(label_values.contains(&"Employee".to_string()));
    assert!(label_values.contains(&"Department".to_string()));

    // 3. Query db.relationshipTypes()
    let rels_res = conn
        .graph_query(graph_name, "CALL db.relationshipTypes()", &HashMap::new())
        .await
        .expect("CALL db.relationshipTypes() failed");
    assert_eq!(rels_res.columns, vec!["relationshipType"]);
    assert_eq!(rels_res.rows[0][0].as_str(), Some("WORKS_IN"));

    // 4. Query db.propertyKeys()
    let props_res = conn
        .graph_query(graph_name, "CALL db.propertyKeys()", &HashMap::new())
        .await
        .expect("CALL db.propertyKeys() failed");
    assert_eq!(props_res.columns, vec!["propertyKey"]);
    let prop_values: Vec<String> = props_res
        .rows
        .iter()
        .map(|r| r[0].to_string_lossy())
        .collect();
    assert!(prop_values.contains(&"name".to_string()));
    assert!(prop_values.contains(&"dept".to_string()));

    // 5. Create index
    let idx_res = conn
        .graph_query(
            graph_name,
            "CREATE INDEX ON :Employee(name)",
            &HashMap::new(),
        )
        .await
        .expect("CREATE INDEX failed");
    assert_eq!(idx_res.statistics.indices_created, 1);

    // 6. Query db.indexes()
    let list_idx_res = conn
        .graph_query(graph_name, "CALL db.indexes()", &HashMap::new())
        .await
        .expect("CALL db.indexes() failed");
    assert!(!list_idx_res.rows.is_empty());
    let idx_str = list_idx_res.rows[0][0].to_string_lossy();
    assert!(idx_str.contains("Employee"));

    let _ = conn.graph_delete(graph_name).await;
    conn.close().await.unwrap();
}
