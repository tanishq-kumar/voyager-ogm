use bytes::{Bytes, BytesMut};
use std::collections::HashMap;
use std::io::Cursor;
use voyager_net::bolt::packstream::{BoltValue, PackStream};
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
fn test_packstream_recursion_depth_limit_rejected() {
    // Construct an adversarial payload with 70 levels of nested single-element TinyLists:
    // 0x91 is TinyList of length 1.
    let mut payload = vec![0x91; 70];
    payload.push(0x01); // Innermost TinyInt(1)

    let mut bytes = Bytes::from(payload);
    let res = PackStream::decode(&mut bytes);
    match res {
        Err(NetError::ProtocolError(msg)) => {
            assert!(
                msg.contains("exceeded maximum nesting depth of 64"),
                "Expected recursion depth rejection, got: {}",
                msg
            );
        }
        Ok(v) => panic!(
            "Expected ProtocolError on nested list depth bomb, got Ok: {:?}",
            v
        ),
        Err(e) => panic!(
            "Expected ProtocolError on nested list depth bomb, got: {:?}",
            e
        ),
    }

    // Verify that a deeply nested payload within safe limits (e.g. 30 levels) decodes properly
    let mut safe_payload = vec![0x91; 30];
    safe_payload.push(0x2A); // TinyInt(42)

    let mut safe_bytes = Bytes::from(safe_payload);
    let safe_res = PackStream::decode(&mut safe_bytes);
    assert!(
        safe_res.is_ok(),
        "Payload within depth limit should decode successfully"
    );
}

#[test]
fn test_packstream_map_recursion_depth_limit_rejected() {
    // Construct an adversarial payload with 70 levels of nested single-entry TinyMaps:
    // 0xA1 is TinyMap of size 1; 0x81, b'k' is key "k".
    let mut payload = Vec::with_capacity(70 * 3 + 1);
    for _ in 0..70 {
        payload.push(0xA1); // TinyMap(1)
        payload.push(0x81); // TinyString(1)
        payload.push(b'k');
    }
    payload.push(0x01); // Innermost TinyInt(1)

    let mut bytes = Bytes::from(payload);
    let res = PackStream::decode(&mut bytes);
    match res {
        Err(NetError::ProtocolError(msg)) => {
            assert!(
                msg.contains("exceeded maximum nesting depth of 64"),
                "Expected recursion depth rejection, got: {}",
                msg
            );
        }
        Ok(v) => panic!(
            "Expected ProtocolError on nested map depth bomb, got Ok: {:?}",
            v
        ),
        Err(e) => panic!(
            "Expected ProtocolError on nested map depth bomb, got: {:?}",
            e
        ),
    }
}

#[test]
fn test_packstream_structure_recursion_depth_limit_rejected() {
    // Construct an adversarial payload with 70 levels of nested single-field TinyStructs:
    // 0xB1 is TinyStruct with 1 field; 0x72 is tag.
    let mut payload = Vec::with_capacity(70 * 2 + 1);
    for _ in 0..70 {
        payload.push(0xB1); // TinyStruct(1)
        payload.push(0x72); // Tag
    }
    payload.push(0x01); // Innermost TinyInt(1)

    let mut bytes = Bytes::from(payload);
    let res = PackStream::decode(&mut bytes);
    match res {
        Err(NetError::ProtocolError(msg)) => {
            assert!(
                msg.contains("exceeded maximum nesting depth of 64"),
                "Expected recursion depth rejection, got: {}",
                msg
            );
        }
        Ok(v) => panic!(
            "Expected ProtocolError on nested struct depth bomb, got Ok: {:?}",
            v
        ),
        Err(e) => panic!(
            "Expected ProtocolError on nested struct depth bomb, got: {:?}",
            e
        ),
    }
}

#[test]
fn test_packstream_map_incomplete_minimum_entry_bytes_rejected() {
    // Map32 (0xDA) claiming size 100, which requires at least 200 bytes (minimum 2 bytes per key-value entry).
    // Buffer only provides 150 bytes.
    // Under old check: size (100) > buf.remaining() (150) was FALSE, passing invalid state into parser.
    // Under new check: min_required (200) > buf.remaining() (150) is TRUE, properly rejecting upfront.
    let mut payload = vec![0xDA, 0x00, 0x00, 0x00, 0x64]; // Map32 with size = 100
    payload.extend(vec![0xC0; 150]); // 150 bytes of dummy payload

    let mut bytes = Bytes::from(payload);
    let res = PackStream::decode(&mut bytes);
    match res {
        Err(NetError::ProtocolError(msg)) => {
            assert!(
                msg.contains(
                    "requires at least 200 bytes, which exceeds available buffer bytes 150"
                ),
                "Expected 2-byte minimum entry rejection, got: {}",
                msg
            );
        }
        Ok(v) => panic!(
            "Expected ProtocolError on incomplete map entries, got Ok: {:?}",
            v
        ),
        Err(e) => panic!(
            "Expected ProtocolError on incomplete map entries, got: {:?}",
            e
        ),
    }
}

#[test]
fn test_packstream_deep_hierarchical_document_roundtrip() {
    // Construct a valid hierarchical graph document nested 25 levels deep (within the 64 limit)
    // with alternating maps and lists, simulating deep JSON / AST trees stored in graph properties.
    let mut current = BoltValue::Integer(1337);
    for i in 0..25 {
        if i % 2 == 0 {
            let mut map = HashMap::new();
            map.insert("child".to_string(), current);
            map.insert("level".to_string(), BoltValue::Integer(i));
            current = BoltValue::Map(map);
        } else {
            current = BoltValue::List(vec![current, BoltValue::String(format!("node_{}", i))]);
        }
    }

    let mut buf = BytesMut::new();
    PackStream::encode(&current, &mut buf);

    let mut bytes = buf.freeze();
    let decoded = PackStream::decode(&mut bytes)
        .expect("25-level hierarchical document should decode cleanly");
    assert_eq!(decoded, current);
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
