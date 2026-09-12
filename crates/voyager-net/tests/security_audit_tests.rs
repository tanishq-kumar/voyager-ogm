use bytes::{Bytes, BytesMut};
use std::collections::HashMap;
use std::io::Cursor;
use voyager_net::bolt::packstream::PackStream;
use voyager_net::bolt::stream::{MAX_BOLT_MESSAGE_SIZE, read_message_frame_buffered};
use voyager_net::client::connect_any;
use voyager_net::config::{ConnectionConfig, TlsMode};
use voyager_net::error::NetError;
use voyager_net::postgres::connection::format_postgres_query;
use voyager_net::redis::resp::RespValue;
use voyager_net::redis::transaction::format_cypher_with_params;
use voyager_net::uri::{DatabaseProtocol, ParsedUri};

#[test]
fn test_falkordb_backslash_quote_injection_defense() {
    let mut params = HashMap::new();
    // Attacker supplies a string ending with a backslash before a quote breakout payload
    params.insert(
        "p".to_string(),
        serde_json::Value::String(r#"malicious\" OR 1=1 --"#.to_string()),
    );

    let formatted = format_cypher_with_params("MATCH (n) RETURN n", &params);
    println!("FalkorDB secured query: {}", formatted);

    // Backslashes MUST be escaped before quotes, ensuring Cypher sees an escaped backslash
    // followed by an escaped quote, completely preventing string literal termination breakout.
    assert!(
        formatted.contains(r#"p="malicious\\\" OR 1=1 --""#),
        "Expected properly escaped backslash and quote in formatted query: {}",
        formatted
    );
}

#[test]
fn test_packstream_list32_and_map32_allocation_bomb_rejected() {
    // PackStream marker 0xD6 (List32) with length 0xFFFFFFFF (4.29 billion elements) followed by 0 bytes.
    let mut list_payload = Bytes::from_static(&[0xD6, 0xFF, 0xFF, 0xFF, 0xFF]);
    let list_res = PackStream::decode(&mut list_payload);
    match list_res {
        Err(NetError::ProtocolError(msg)) => {
            assert!(
                msg.contains("exceeds available buffer bytes"),
                "Expected buffer bounds rejection, got: {}",
                msg
            );
        }
        Ok(v) => panic!("Expected ProtocolError on List32 bomb, got Ok: {:?}", v),
        Err(e) => panic!("Expected ProtocolError on List32 bomb, got: {:?}", e),
    }

    // PackStream marker 0xDA (Map32) with length 0xFFFFFFFF followed by 0 bytes.
    let mut map_payload = Bytes::from_static(&[0xDA, 0xFF, 0xFF, 0xFF, 0xFF]);
    let map_res = PackStream::decode(&mut map_payload);
    match map_res {
        Err(NetError::ProtocolError(msg)) => {
            assert!(
                msg.contains("exceeds available buffer bytes"),
                "Expected buffer bounds rejection, got: {}",
                msg
            );
        }
        Ok(v) => panic!("Expected ProtocolError on Map32 bomb, got Ok: {:?}", v),
        Err(e) => panic!("Expected ProtocolError on Map32 bomb, got: {:?}", e),
    }
}

#[test]
fn test_resp_array_and_map_allocation_bomb_rejected() {
    // Malicious RESP array claiming 2,147,483,647 elements
    let mut buf = BytesMut::from(&b"*2147483647\r\n"[..]);
    let res = RespValue::parse(&mut buf);
    match res {
        Err(NetError::ProtocolError(msg)) => {
            assert!(
                msg.contains("exceeds maximum permitted collection size"),
                "Expected collection size rejection, got: {}",
                msg
            );
        }
        Ok(v) => panic!("Expected ProtocolError on RESP array bomb, got Ok: {:?}", v),
        Err(e) => panic!("Expected ProtocolError on RESP array bomb, got: {:?}", e),
    }

    // Malicious RESP3 map claiming 2,147,483,647 pairs
    let mut map_buf = BytesMut::from(&b"%2147483647\r\n"[..]);
    let map_res = RespValue::parse(&mut map_buf);
    match map_res {
        Err(NetError::ProtocolError(msg)) => {
            assert!(
                msg.contains("exceeds maximum permitted collection size"),
                "Expected collection size rejection, got: {}",
                msg
            );
        }
        Ok(v) => panic!("Expected ProtocolError on RESP map bomb, got Ok: {:?}", v),
        Err(e) => panic!("Expected ProtocolError on RESP map bomb, got: {:?}", e),
    }
}

#[test]
fn test_postgres_parameter_collision_and_escaping() {
    let mut params = HashMap::new();
    params.insert("p1".to_string(), serde_json::json!(42));
    params.insert("p10".to_string(), serde_json::json!(100));

    let query = "SELECT * FROM users WHERE id = $p10 AND age = $p1;";
    let formatted = format_postgres_query(query, &params);
    println!("PostgreSQL formatted query: {}", formatted);

    // Prefix collision elimination: $p10 must NOT become 420!
    assert!(
        formatted.contains("id = 100"),
        "Expected id = 100, got: {}",
        formatted
    );
    assert!(
        formatted.contains("age = 42"),
        "Expected age = 42, got: {}",
        formatted
    );

    // Single quote escaping for strings
    let mut quote_params = HashMap::new();
    quote_params.insert("name".to_string(), serde_json::json!("O'Reilly"));
    let quote_query = "SELECT * FROM authors WHERE name = $name;";
    let quote_formatted = format_postgres_query(quote_query, &quote_params);
    assert!(
        quote_formatted.contains("'O''Reilly'"),
        "Expected single quote to be escaped as '', got: {}",
        quote_formatted
    );
}

#[tokio::test]
async fn test_bolt_stream_maximum_payload_guard() {
    assert_eq!(MAX_BOLT_MESSAGE_SIZE, 128 * 1024 * 1024);

    // Test that valid chunked frame reads correctly
    let mut valid_data = Vec::new();
    valid_data.extend_from_slice(&4u16.to_be_bytes());
    valid_data.extend_from_slice(&[1, 2, 3, 4]);
    valid_data.extend_from_slice(&0u16.to_be_bytes());
    let mut valid_cursor = Cursor::new(valid_data);
    let mut read_buf = BytesMut::new();
    let res = read_message_frame_buffered(&mut valid_cursor, &mut read_buf).await;
    assert!(res.is_ok());
    assert_eq!(res.unwrap().as_ref(), &[1, 2, 3, 4]);
}

#[tokio::test]
async fn test_plaintext_tls_downgrade_prevention() {
    // 1. bolt+s scheme
    let uri = "bolt+s://neo4j:password@127.0.0.1:7687";
    let parsed = ParsedUri::parse(uri).expect("Parse should succeed");
    assert_eq!(parsed.protocol, DatabaseProtocol::Bolt);
    assert_eq!(parsed.tls, TlsMode::Required);

    let config = ConnectionConfig::from_uri(uri);
    assert_eq!(
        config.tls,
        TlsMode::Required,
        "ConnectionConfig::from_uri must preserve TLS mode!"
    );

    let res = connect_any(uri).await;
    match res {
        Err(NetError::TlsError(msg)) => {
            println!("Safely rejected bolt+s plaintext downgrade: {}", msg);
            assert!(msg.contains("Plaintext downgrade rejected"));
        }
        Ok(_) => panic!("Expected TlsError on bolt+s connection, got Ok"),
        Err(e) => panic!(
            "Expected TlsError on bolt+s connection, got other error: {:?}",
            e
        ),
    }

    // 2. rediss scheme
    let redis_uri = "rediss://default:password@127.0.0.1:6379";
    let redis_res = connect_any(redis_uri).await;
    match redis_res {
        Err(NetError::TlsError(msg)) => {
            println!("Safely rejected rediss plaintext downgrade: {}", msg);
            assert!(msg.contains("Plaintext downgrade rejected"));
        }
        Ok(_) => panic!("Expected TlsError on rediss connection, got Ok"),
        Err(e) => panic!(
            "Expected TlsError on rediss connection, got other error: {:?}",
            e
        ),
    }

    // 3. postgresql+s scheme
    let pg_uri = "postgresql+s://postgres:secret@127.0.0.1:5432/mydb";
    let pg_res = connect_any(pg_uri).await;
    match pg_res {
        Err(NetError::TlsError(msg)) => {
            println!("Safely rejected postgresql+s plaintext downgrade: {}", msg);
            assert!(msg.contains("Plaintext downgrade rejected"));
        }
        Ok(_) => panic!("Expected TlsError on postgresql+s connection, got Ok"),
        Err(e) => panic!(
            "Expected TlsError on postgresql+s connection, got other error: {:?}",
            e
        ),
    }
}

#[tokio::test]
async fn test_pool_discards_dead_connection_on_failed_reset() {
    use std::sync::Arc;
    use voyager_net::config::PoolConfig;
    use voyager_net::mock::MockConnectionFactory;
    use voyager_net::pool::ConnectionPool;

    let factory = Arc::new(MockConnectionFactory::new());
    let config = PoolConfig::new().with_min_idle(1).with_max_size(2);
    let pool = ConnectionPool::new(config, factory.clone());

    pool.warm_up().await.expect("Failed to warm up pool");
    assert_eq!(pool.idle_count(), 1);

    // Acquire the connection, set in_transaction = true and fail_reset = true
    {
        let mut conn = pool.acquire().await.expect("Failed to acquire connection");
        conn.in_transaction = true;
        conn.fail_reset = true;
        // On drop, it is returned to the pool's idle queue
    }

    // Now acquire again: the pool tries to reset the idle connection.
    // When reset() fails, the pool must discard it rather than leasing a broken connection,
    // and provision a fresh healthy connection from the factory.
    let conn2 = pool
        .acquire()
        .await
        .expect("Failed to acquire fresh connection");
    assert!(conn2.is_valid);
    assert!(!conn2.fail_reset);
    assert_eq!(factory.created_count(), 2);
}

#[test]
fn test_connection_query_timeout_defaults_and_mutation() {
    let config = ConnectionConfig::from_uri("bolt://localhost:7687");
    assert_eq!(
        config.query_timeout,
        Some(std::time::Duration::from_secs(30))
    );

    let parsed_pg = ParsedUri::parse("postgresql://localhost:5432/db?query_timeout=15").unwrap();
    assert_eq!(parsed_pg.params.get("query_timeout").unwrap(), "15");

    let parsed_redis = ParsedUri::parse("redis://localhost:6379?query_timeout=45").unwrap();
    assert_eq!(parsed_redis.params.get("query_timeout").unwrap(), "45");
}
