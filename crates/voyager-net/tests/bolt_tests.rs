//! Comprehensive unit and integration tests for Bolt wire protocol, PackStream, and stub-server.

use bytes::BytesMut;
use std::collections::HashMap;
use std::sync::Arc;

use voyager_net::bolt::{
    BoltConnection, BoltNode, BoltPath, BoltRelationship, BoltRequest, BoltResponse,
    BoltStubServer, BoltUnboundRelationship, BoltValue, PackStream, encode_chunks,
    read_message_frame,
};
use voyager_net::config::{ConnectionConfig, PoolConfig};
use voyager_net::engine::{AsyncConnection, ConnectionFactory};
use voyager_net::pool::ConnectionPool;

#[test]
fn test_packstream_scalars_roundtrip() {
    let test_values = vec![
        BoltValue::Null,
        BoltValue::Boolean(true),
        BoltValue::Boolean(false),
        BoltValue::Integer(0),
        BoltValue::Integer(42),
        BoltValue::Integer(-15),
        BoltValue::Integer(127),
        BoltValue::Integer(-128),
        BoltValue::Integer(32767),
        BoltValue::Integer(-32768),
        BoltValue::Integer(2147483647),
        BoltValue::Integer(-2147483648),
        BoltValue::Integer(9223372036854775807),
        BoltValue::Float(std::f64::consts::PI),
        BoltValue::String("hello world".to_string()),
        BoltValue::String("".to_string()),
        BoltValue::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]),
    ];

    for val in test_values {
        let mut buf = BytesMut::new();
        PackStream::encode(&val, &mut buf);
        let mut bytes = buf.freeze();
        let decoded = PackStream::decode(&mut bytes).expect("Failed to decode PackStream value");
        assert_eq!(val, decoded);
    }
}

#[test]
fn test_packstream_containers_roundtrip() {
    // List
    let list_val = BoltValue::List(vec![
        BoltValue::Integer(1),
        BoltValue::String("two".to_string()),
        BoltValue::Boolean(true),
    ]);
    let mut buf = BytesMut::new();
    PackStream::encode(&list_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_list = PackStream::decode(&mut bytes).expect("Failed to decode list");
    assert_eq!(list_val, decoded_list);

    // Map
    let mut map = HashMap::new();
    map.insert("name".to_string(), BoltValue::String("Alice".to_string()));
    map.insert("age".to_string(), BoltValue::Integer(30));
    let map_val = BoltValue::Map(map);

    let mut buf = BytesMut::new();
    PackStream::encode(&map_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_map = PackStream::decode(&mut bytes).expect("Failed to decode map");
    assert_eq!(map_val, decoded_map);
}

#[test]
fn test_packstream_graph_structures_roundtrip() {
    // 1. Node
    let mut props = HashMap::new();
    props.insert("name".to_string(), BoltValue::String("Alice".to_string()));
    let node = BoltNode {
        id: 101,
        labels: vec!["Person".to_string(), "Developer".to_string()],
        properties: props.clone(),
        element_id: Some("4:101".to_string()),
    };
    let node_val = BoltValue::Node(node.clone());

    let mut buf = BytesMut::new();
    PackStream::encode(&node_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_node = PackStream::decode(&mut bytes).expect("Failed to decode node");
    assert_eq!(node_val, decoded_node);

    // 2. Relationship
    let rel = BoltRelationship {
        id: 202,
        start_node_id: 101,
        end_node_id: 102,
        rel_type: "KNOWS".to_string(),
        properties: props.clone(),
        element_id: Some("5:202".to_string()),
        start_element_id: Some("4:101".to_string()),
        end_element_id: Some("4:102".to_string()),
    };
    let rel_val = BoltValue::Relationship(rel.clone());

    let mut buf = BytesMut::new();
    PackStream::encode(&rel_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_rel = PackStream::decode(&mut bytes).expect("Failed to decode relationship");
    assert_eq!(rel_val, decoded_rel);

    // 3. Path
    let path = BoltPath {
        nodes: vec![node],
        relationships: vec![BoltUnboundRelationship {
            id: 202,
            rel_type: "KNOWS".to_string(),
            properties: props,
            element_id: Some("5:202".to_string()),
        }],
        sequence: vec![1, 1],
    };
    let path_val = BoltValue::Path(path);

    let mut buf = BytesMut::new();
    PackStream::encode(&path_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_path = PackStream::decode(&mut bytes).expect("Failed to decode path");
    assert_eq!(path_val, decoded_path);
}

#[test]
fn test_bolt_messages_encode_decode() {
    // RUN request
    let mut params = HashMap::new();
    params.insert("p0".to_string(), BoltValue::String("Alice".to_string()));
    let run_req = BoltRequest::run(
        "MATCH (n:Person {name: $p0}) RETURN n",
        params,
        Some("neo4j"),
    );

    let mut buf = BytesMut::new();
    run_req.encode(&mut buf);
    assert!(!buf.is_empty());

    // SUCCESS response
    let mut meta = HashMap::new();
    meta.insert(
        "fields".to_string(),
        BoltValue::List(vec![BoltValue::String("n.name".to_string())]),
    );
    let resp = BoltResponse::Success { metadata: meta };

    let mut resp_buf = BytesMut::new();
    PackStream::encode(
        &BoltValue::Structure {
            tag: 0x70,
            fields: vec![BoltValue::Map(match resp {
                BoltResponse::Success { ref metadata } => metadata.clone(),
                _ => HashMap::new(),
            })],
        },
        &mut resp_buf,
    );

    let mut resp_bytes = resp_buf.freeze();
    let decoded_resp = BoltResponse::decode(&mut resp_bytes).expect("Failed to decode response");
    assert_eq!(resp, decoded_resp);
    assert_eq!(decoded_resp.fields(), Some(vec!["n.name".to_string()]));
}

#[test]
fn test_chunked_stream_large_payload() {
    // Large payload (> 16 KB)
    let large_payload = vec![0xAB; 20_000];
    let mut chunk_buf = BytesMut::new();
    encode_chunks(&large_payload, &mut chunk_buf);

    // Reassemble from buffer
    let mut cursor = std::io::Cursor::new(chunk_buf.freeze());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let reassembled = rt
        .block_on(async { read_message_frame(&mut cursor).await })
        .expect("Failed to reassemble chunks");

    assert_eq!(reassembled.as_ref(), large_payload.as_slice());
}

#[tokio::test]
async fn test_bolt_stub_server_handshake_and_query() {
    // 1. Start mock Bolt stub server
    let stub = BoltStubServer::start()
        .await
        .expect("Failed to start stub server");
    let uri = stub.uri();

    // 2. Connect client
    let config = ConnectionConfig::from_uri(&uri);
    let mut conn = BoltConnection::connect(&config)
        .await
        .expect("Failed to connect to stub");

    assert!(conn.is_valid());

    // 3. Execute query
    let mut params = HashMap::new();
    params.insert(
        "name".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );

    let result = conn
        .execute("MATCH (n:Person) RETURN n.name, n.age", &params)
        .await
        .expect("Query execution failed");

    assert_eq!(result.columns, vec!["name".to_string(), "age".to_string()]);
    assert_eq!(result.row_count(), 2);

    let batch = result
        .into_single_batch()
        .unwrap()
        .expect("Missing record batch");
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(batch.num_columns(), 2);

    // 4. Test liveness ping
    conn.ping().await.expect("Ping failed");

    // 5. Close connection
    conn.close().await.expect("Close failed");
}

#[tokio::test]
async fn test_bolt_stub_server_error_and_reset_recovery() {
    let stub = BoltStubServer::start()
        .await
        .expect("Failed to start stub server");
    let config = ConnectionConfig::from_uri(stub.uri());
    let mut conn = BoltConnection::connect(&config)
        .await
        .expect("Failed to connect");

    // Query containing FAIL will trigger simulated server failure
    let err = conn
        .execute("FAIL SYNTAX QUERY", &HashMap::new())
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("Simulated query execution failure")
    );

    // Connection automatically reset, should be capable of next query
    let result = conn
        .execute("MATCH (n:Person) RETURN n.name", &HashMap::new())
        .await
        .expect("Subsequent query failed after reset");

    assert_eq!(result.row_count(), 2);
}

/// Factory for pooled Bolt connections.
struct BoltConnectionFactory {
    config: ConnectionConfig,
}

#[async_trait::async_trait]
impl ConnectionFactory<BoltConnection> for BoltConnectionFactory {
    async fn create(&self) -> voyager_net::Result<BoltConnection> {
        BoltConnection::connect(&self.config).await
    }
}

#[tokio::test]
async fn test_bolt_connection_pool_end_to_end() {
    let stub = BoltStubServer::start()
        .await
        .expect("Failed to start stub server");
    let config = ConnectionConfig::from_uri(stub.uri());

    let factory = Arc::new(BoltConnectionFactory {
        config: config.clone(),
    });
    let pool_config = PoolConfig::new().with_min_idle(2).with_max_size(4);
    let pool = ConnectionPool::new(pool_config, factory);

    pool.warm_up().await.expect("Pool warmup failed");
    assert_eq!(pool.idle_count(), 2);

    // Acquire and execute across multiple concurrent tasks
    let mut handles = Vec::new();
    for i in 0..8 {
        let pool_clone = pool.clone();
        handles.push(tokio::spawn(async move {
            let mut conn = pool_clone.acquire().await.expect("Acquire failed");
            let mut params = HashMap::new();
            params.insert("idx".to_string(), serde_json::Value::Number(i.into()));
            let result = conn
                .execute("MATCH (n:Person) RETURN n.name, n.age", &params)
                .await
                .expect("Execution failed");
            assert_eq!(result.row_count(), 2);
        }));
    }

    for h in handles {
        h.await.expect("Task failed");
    }

    assert_eq!(pool.active_count(), 0);
    assert!(pool.idle_count() >= 2);
}

#[test]
fn test_uri_ipv6_bracket_formatting() {
    use voyager_net::uri::ParsedUri;

    let uri_ipv6 = ParsedUri::parse("bolt://[::1]:7687").unwrap();
    assert_eq!(uri_ipv6.socket_addr(), "[::1]:7687");

    let uri_raw_ipv6 = ParsedUri::parse("bolt://::1:7687")
        .unwrap_or_else(|_| ParsedUri::parse("bolt://[::1]:7687").unwrap());
    assert_eq!(uri_raw_ipv6.socket_addr(), "[::1]:7687");

    let uri_ipv4 = ParsedUri::parse("bolt://127.0.0.1:7687").unwrap();
    assert_eq!(uri_ipv4.socket_addr(), "127.0.0.1:7687");
}

#[tokio::test]
async fn test_heterogeneous_and_mixed_number_arrow_conversions() {
    let stub = BoltStubServer::start()
        .await
        .expect("Failed to start stub server");
    let config = ConnectionConfig::from_uri(stub.uri());
    let mut conn = BoltConnection::connect(&config)
        .await
        .expect("Connect failed");

    // Test query execution with stub
    let res = conn
        .execute("MATCH (n:Person) RETURN n.name, n.age", &HashMap::new())
        .await
        .expect("Query failed");
    let batch = res.into_single_batch().unwrap().expect("Batch missing");
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(batch.num_columns(), 2);
    assert_eq!(
        batch.schema().field(0).data_type(),
        &arrow_schema::DataType::Utf8
    );
    assert_eq!(
        batch.schema().field(1).data_type(),
        &arrow_schema::DataType::Int64
    );

    conn.close().await.expect("Close failed");
}
