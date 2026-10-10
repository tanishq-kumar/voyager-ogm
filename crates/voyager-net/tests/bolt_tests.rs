//! Comprehensive unit and integration tests for Bolt wire protocol, PackStream, and stub-server.

use bytes::BytesMut;
use std::collections::HashMap;
use std::sync::Arc;

use voyager_net::bolt::{
    BoltConnection, BoltDate, BoltDateTime, BoltDateTimeZoneId, BoltDuration, BoltLocalDateTime,
    BoltLocalTime, BoltNode, BoltPath, BoltPoint2D, BoltPoint3D, BoltRelationship, BoltRequest,
    BoltResponse, BoltStubServer, BoltTime, BoltUnboundRelationship, BoltValue, PackStream,
    encode_chunks, read_message_frame, srid,
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
fn test_packstream_spatial_points_roundtrip() {
    // 1. Point2D WGS-84 (SRID 4326)
    let p2d_wgs = BoltValue::Point2D(BoltPoint2D {
        srid: srid::WGS_84_2D,
        x: 12.56,
        y: 55.67,
    });
    let mut buf = BytesMut::new();
    PackStream::encode(&p2d_wgs, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_p2d = PackStream::decode(&mut bytes).expect("Failed to decode Point2D");
    assert_eq!(p2d_wgs, decoded_p2d);

    // 2. Point2D Cartesian (SRID 7203)
    let p2d_cart = BoltValue::Point2D(BoltPoint2D {
        srid: srid::CARTESIAN_2D,
        x: 100.0,
        y: 200.0,
    });
    let mut buf = BytesMut::new();
    PackStream::encode(&p2d_cart, &mut buf);
    let mut bytes = buf.freeze();
    assert_eq!(p2d_cart, PackStream::decode(&mut bytes).unwrap());

    // 3. Point3D WGS-84 (SRID 4979)
    let p3d_wgs = BoltValue::Point3D(BoltPoint3D {
        srid: srid::WGS_84_3D,
        x: 13.4,
        y: 52.5,
        z: 35.0,
    });
    let mut buf = BytesMut::new();
    PackStream::encode(&p3d_wgs, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_p3d = PackStream::decode(&mut bytes).expect("Failed to decode Point3D");
    assert_eq!(p3d_wgs, decoded_p3d);

    // 4. Point3D Cartesian (SRID 9157)
    let p3d_cart = BoltValue::Point3D(BoltPoint3D {
        srid: srid::CARTESIAN_3D,
        x: 10.0,
        y: 20.0,
        z: 30.0,
    });
    let mut buf = BytesMut::new();
    PackStream::encode(&p3d_cart, &mut buf);
    let mut bytes = buf.freeze();
    assert_eq!(p3d_cart, PackStream::decode(&mut bytes).unwrap());
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

#[test]
fn test_packstream_temporal_types_roundtrip() {
    // 1. Date
    let date_epoch = BoltValue::Date(BoltDate::new(0));
    let mut buf = BytesMut::new();
    PackStream::encode(&date_epoch, &mut buf);
    let mut bytes = buf.freeze();
    let decoded = PackStream::decode(&mut bytes).expect("Failed to decode epoch date");
    assert_eq!(decoded, date_epoch);
    assert_eq!(date_epoch.as_date().unwrap().to_iso_string(), "1970-01-01");

    // Leap year date (Feb 29, 2024)
    let date_leap = BoltValue::Date(BoltDate::from_ymd(2024, 2, 29).expect("valid leap date"));
    let mut buf = BytesMut::new();
    PackStream::encode(&date_leap, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_leap = PackStream::decode(&mut bytes).expect("Failed to decode leap date");
    assert_eq!(decoded_leap, date_leap);
    assert_eq!(date_leap.as_date().unwrap().to_iso_string(), "2024-02-29");
    assert_eq!(date_leap.as_date().unwrap().to_ymd(), (2024, 2, 29));

    // Pre-epoch date (Moon Landing: July 20, 1969)
    let date_moon = BoltValue::Date(BoltDate::from_ymd(1969, 7, 20).expect("valid date"));
    let mut buf = BytesMut::new();
    PackStream::encode(&date_moon, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_moon = PackStream::decode(&mut bytes).expect("Failed to decode pre-epoch date");
    assert_eq!(decoded_moon, date_moon);
    assert_eq!(date_moon.as_date().unwrap().to_iso_string(), "1969-07-20");

    // 2. Time (with timezone offset)
    let time_val = BoltValue::Time(BoltTime::new(52215_123456000, 7200));
    let mut buf = BytesMut::new();
    PackStream::encode(&time_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_time = PackStream::decode(&mut bytes).expect("Failed to decode Time");
    assert_eq!(decoded_time, time_val);
    assert_eq!(
        time_val.as_time().unwrap().to_iso_string(),
        "14:30:15.123456+02:00"
    );

    // Negative offset time (14:30:15-05:00)
    let time_neg = BoltValue::Time(BoltTime::new(52215_000000000, -18000));
    assert_eq!(
        time_neg.as_time().unwrap().to_iso_string(),
        "14:30:15-05:00"
    );

    // 3. LocalTime (no timezone offset)
    let ltime_val = BoltValue::LocalTime(BoltLocalTime::new(52215_123456789));
    let mut buf = BytesMut::new();
    PackStream::encode(&ltime_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_ltime = PackStream::decode(&mut bytes).expect("Failed to decode LocalTime");
    assert_eq!(decoded_ltime, ltime_val);
    assert_eq!(
        ltime_val.as_local_time().unwrap().to_iso_string(),
        "14:30:15.123456789"
    );

    // 4. DateTime (with timezone offset)
    let dt_val = BoltValue::DateTime(BoltDateTime::new(1710513015, 123456000, 7200));
    let mut buf = BytesMut::new();
    PackStream::encode(&dt_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_dt = PackStream::decode(&mut bytes).expect("Failed to decode DateTime");
    assert_eq!(decoded_dt, dt_val);
    assert_eq!(
        dt_val.as_datetime().unwrap().to_iso_string(),
        "2024-03-15T16:30:15.123456+02:00"
    );

    // 5. LocalDateTime
    let ldt_val = BoltValue::LocalDateTime(BoltLocalDateTime::new(1710513015, 500000000));
    let mut buf = BytesMut::new();
    PackStream::encode(&ldt_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_ldt = PackStream::decode(&mut bytes).expect("Failed to decode LocalDateTime");
    assert_eq!(decoded_ldt, ldt_val);
    assert_eq!(
        ldt_val.as_local_datetime().unwrap().to_iso_string(),
        "2024-03-15T14:30:15.500"
    );

    // 6. DateTimeZoneId (with IANA timezone name)
    let dtz_val = BoltValue::DateTimeZoneId(BoltDateTimeZoneId::new(
        1710513015,
        123456000,
        "Europe/Berlin",
    ));
    let mut buf = BytesMut::new();
    PackStream::encode(&dtz_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_dtz = PackStream::decode(&mut bytes).expect("Failed to decode DateTimeZoneId");
    assert_eq!(decoded_dtz, dtz_val);
    assert_eq!(
        dtz_val.as_datetime_zone_id().unwrap().to_iso_string(),
        "2024-03-15T14:30:15.123456Z[Europe/Berlin]"
    );

    // 7. Duration
    let dur_val = BoltValue::Duration(BoltDuration::new(14, 3, 14706, 789000000));
    let mut buf = BytesMut::new();
    PackStream::encode(&dur_val, &mut buf);
    let mut bytes = buf.freeze();
    let decoded_dur = PackStream::decode(&mut bytes).expect("Failed to decode Duration");
    assert_eq!(decoded_dur, dur_val);
    assert_eq!(
        dur_val.as_duration().unwrap().to_iso_string(),
        "P1Y2M3DT4H5M6.789S"
    );

    // Zero duration
    let dur_zero = BoltValue::Duration(BoltDuration::new(0, 0, 0, 0));
    assert_eq!(dur_zero.as_duration().unwrap().to_iso_string(), "PT0S");
}
