//! Property-based fuzz testing and memory safety invariant verification for Bolt PackStream and Redis RESP wire protocols.
//!
//! Verifies:
//! - Panic freedom on completely arbitrary, corrupted, and truncated byte streams
//! - Memory bomb defense (preventing multi-gigabyte pre-allocations from spoofed headers)
//! - Recursion depth limit enforcement (preventing stack overflows on deeply nested payloads)
//! - Partial frame handling and incremental truncation resistance
//! - Deterministic roundtrip consistency on valid data

use bytes::{Bytes, BytesMut};
use proptest::prelude::*;
use std::collections::HashMap;

use voyager_net::bolt::{BoltNode, BoltPath, BoltRelationship, BoltValue, PackStream};
use voyager_net::error::NetError;
use voyager_net::redis::RespValue;

// ============================================================================
// 1. Bolt PackStream Fuzzing & Safety Tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Property: PackStream::decode must NEVER panic on any arbitrary byte sequence.
    #[test]
    fn prop_packstream_never_panics_on_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut bytes = Bytes::copy_from_slice(&data);
        let result = PackStream::decode(&mut bytes);
        match result {
            Ok(_) => {}
            Err(NetError::ProtocolError(_)) => {}
            Err(other) => panic!("Unexpected non-protocol error from PackStream::decode: {:?}", other),
        }
    }

    /// Property: Encoded primitive scalars must roundtrip with exact equivalence.
    #[test]
    fn prop_packstream_scalar_roundtrip(
        i in any::<i64>(),
        s in "\\PC*",
        b in any::<bool>(),
        raw in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        // Integer
        let val_int = BoltValue::Integer(i);
        let mut buf = BytesMut::new();
        PackStream::encode(&val_int, &mut buf);
        let mut bytes = buf.freeze();
        assert_eq!(PackStream::decode(&mut bytes).unwrap(), val_int);

        // String
        let val_str = BoltValue::String(s);
        let mut buf = BytesMut::new();
        PackStream::encode(&val_str, &mut buf);
        let mut bytes = buf.freeze();
        assert_eq!(PackStream::decode(&mut bytes).unwrap(), val_str);

        // Boolean
        let val_bool = BoltValue::Boolean(b);
        let mut buf = BytesMut::new();
        PackStream::encode(&val_bool, &mut buf);
        let mut bytes = buf.freeze();
        assert_eq!(PackStream::decode(&mut bytes).unwrap(), val_bool);

        // Bytes
        let val_bytes = BoltValue::Bytes(raw);
        let mut buf = BytesMut::new();
        PackStream::encode(&val_bytes, &mut buf);
        let mut bytes = buf.freeze();
        assert_eq!(PackStream::decode(&mut bytes).unwrap(), val_bytes);
    }
}

#[test]
fn test_packstream_truncation_invariance() {
    let test_structures = vec![
        BoltValue::Node(BoltNode {
            id: 42,
            labels: vec!["Person".to_string(), "Developer".to_string()],
            properties: {
                let mut map = HashMap::new();
                map.insert("name".to_string(), BoltValue::String("Alice".to_string()));
                map.insert("age".to_string(), BoltValue::Integer(30));
                map.insert("active".to_string(), BoltValue::Boolean(true));
                map
            },
            element_id: Some("4:node:42".to_string()),
        }),
        BoltValue::Relationship(BoltRelationship {
            id: 101,
            start_node_id: 42,
            end_node_id: 43,
            rel_type: "KNOWS".to_string(),
            properties: HashMap::new(),
            element_id: Some("5:rel:101".to_string()),
            start_element_id: Some("4:node:42".to_string()),
            end_element_id: Some("4:node:43".to_string()),
        }),
        BoltValue::Path(BoltPath {
            nodes: vec![
                BoltNode {
                    id: 1,
                    labels: vec!["A".to_string()],
                    properties: HashMap::new(),
                    element_id: None,
                },
                BoltNode {
                    id: 2,
                    labels: vec!["B".to_string()],
                    properties: HashMap::new(),
                    element_id: None,
                },
            ],
            relationships: vec![voyager_net::bolt::BoltUnboundRelationship {
                id: 10,
                rel_type: "EDGE".to_string(),
                properties: HashMap::new(),
                element_id: None,
            }],
            sequence: vec![1, 1, 2],
        }),
    ];

    for complex_value in test_structures {
        let mut buf = BytesMut::new();
        PackStream::encode(&complex_value, &mut buf);
        let encoded = buf.freeze();

        // Verify truncated payloads at every single byte prefix never panic and return ProtocolError
        for prefix_len in 0..encoded.len() {
            let mut truncated = encoded.slice(..prefix_len);
            let result = PackStream::decode(&mut truncated);
            assert!(
                result.is_err(),
                "Expected decode error on truncated payload of length {}/{}",
                prefix_len,
                encoded.len()
            );
            match result.unwrap_err() {
                NetError::ProtocolError(_) => {}
                other => panic!(
                    "Expected ProtocolError on truncated stream, got: {:?}",
                    other
                ),
            }
        }
    }
}

#[test]
fn test_packstream_allocation_bomb_defense() {
    // 0xD6 is List32 followed by 4-byte big-endian length. Specify 2,000,000,000 elements with only 4 bytes following.
    // The parser must reject the impossible claimed count immediately without attempting large allocations.
    let mut bomb_list = BytesMut::from(&[0xD6, 0x77, 0x35, 0x94, 0x00, 0x01, 0x02, 0x03][..]);
    let mut bytes = bomb_list.split().freeze();
    let result = PackStream::decode(&mut bytes);
    assert!(
        matches!(result, Err(NetError::ProtocolError(_))),
        "Expected ProtocolError on allocation bomb, got: {:?}",
        result
    );

    // 0xDA is Map32 followed by 4-byte length. Specify 1,000,000,000 entries.
    let mut bomb_map = BytesMut::from(&[0xDA, 0x3B, 0x9A, 0xCA, 0x00, 0x01, 0x02][..]);
    let mut bytes = bomb_map.split().freeze();
    let result = PackStream::decode(&mut bytes);
    assert!(
        matches!(result, Err(NetError::ProtocolError(_))),
        "Expected ProtocolError on allocation bomb, got: {:?}",
        result
    );
}

#[test]
fn test_packstream_recursion_depth_limit() {
    // Construct nested lists exceeding MAX_PACKSTREAM_DEPTH (64)
    // 0x91 is TinyList of size 1. 70 levels deep:
    let mut payload = vec![0x91; 70];
    payload.push(0x01); // 1 (TinyInt)
    let mut bytes = Bytes::from(payload);

    let result = PackStream::decode(&mut bytes);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("maximum nesting depth"),
        "Expected recursion depth error, got: {}",
        err_msg
    );
}

// ============================================================================
// 2. Redis RESP Fuzzing & Safety Tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Property: RespValue::parse must NEVER panic on any arbitrary byte sequence.
    #[test]
    fn prop_resp_never_panics_on_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut buf = BytesMut::from(&data[..]);
        let result = RespValue::parse(&mut buf);
        match result {
            Ok(_) => {}
            Err(NetError::ProtocolError(_)) => {}
            Err(other) => panic!("Unexpected non-protocol error from RespValue::parse: {:?}", other),
        }
    }

    /// Property: Valid encoded RESP types must roundtrip losslessly.
    #[test]
    fn prop_resp_scalars_roundtrip(
        s in "[a-zA-Z0-9_ -]{1,100}",
        i in any::<i64>(),
        b in any::<bool>(),
    ) {
        // SimpleString
        let val_ss = RespValue::SimpleString(s.clone());
        let mut buf = BytesMut::from(&val_ss.to_bytes()[..]);
        assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(val_ss));

        // Integer
        let val_int = RespValue::Integer(i);
        let mut buf = BytesMut::from(&val_int.to_bytes()[..]);
        assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(val_int));

        // Boolean
        let val_bool = RespValue::Boolean(b);
        let mut buf = BytesMut::from(&val_bool.to_bytes()[..]);
        assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(val_bool));
    }
}

#[test]
fn test_resp_truncation_invariance() {
    let complex_resp = RespValue::Array(Some(vec![
        RespValue::SimpleString("FALKORDB".to_string()),
        RespValue::Integer(100),
        RespValue::BulkString(Some(b"MATCH (n) RETURN n".to_vec())),
        RespValue::Map(vec![
            (
                RespValue::SimpleString("nodes_created".to_string()),
                RespValue::Integer(5),
            ),
            (
                RespValue::SimpleString("cached".to_string()),
                RespValue::Boolean(true),
            ),
        ]),
        RespValue::VerbatimString {
            format: *b"txt",
            text: b"query plan details".to_vec(),
        },
    ]));

    let encoded = complex_resp.to_bytes();

    // Verify every single truncated prefix either returns Ok(None) or Err(ProtocolError), NEVER panics
    for prefix_len in 0..encoded.len() {
        let mut buf = BytesMut::from(&encoded[..prefix_len]);
        let result = RespValue::parse(&mut buf);
        match result {
            Ok(None) => {} // Waiting for more data (safe partial frame)
            Ok(Some(val)) => {
                // Should only happen if prefix happens to end at a valid sub-frame boundary
                assert!(
                    !buf.is_empty() || prefix_len == encoded.len(),
                    "Unexpected full match: {:?}",
                    val
                );
            }
            Err(NetError::ProtocolError(_)) => {}
            Err(other) => panic!(
                "Expected ProtocolError on truncated stream, got: {:?}",
                other
            ),
        }
    }
}

#[test]
fn test_resp_allocation_bomb_defense() {
    // Array specifying 10,000,000 elements (> MAX_RESP_COLLECTION_LEN)
    let mut bomb_array = BytesMut::from(&b"*10000000\r\n"[..]);
    let result = RespValue::parse(&mut bomb_array);
    assert!(
        matches!(result, Err(NetError::ProtocolError(_))),
        "Expected ProtocolError on collection limit exceeding elements, got: {:?}",
        result
    );

    // Map specifying 5,000,000 pairs
    let mut bomb_map = BytesMut::from(&b"%5000000\r\n"[..]);
    let result = RespValue::parse(&mut bomb_map);
    assert!(
        matches!(result, Err(NetError::ProtocolError(_))),
        "Expected ProtocolError on collection limit exceeding pairs, got: {:?}",
        result
    );

    // Bulk string requesting 2 GB when only 10 bytes present: should safely return Ok(None) without pre-allocating
    let mut bomb_bulk = BytesMut::from(&b"$2000000000\r\nsmall"[..]);
    let result = RespValue::parse(&mut bomb_bulk);
    assert_eq!(result.unwrap(), None);
}

#[test]
fn test_resp_recursion_depth_limit() {
    // Construct an array nested 140 levels deep (> MAX_RESP_DEPTH of 128)
    let mut nested = String::new();
    for _ in 0..140 {
        nested.push_str("*1\r\n");
    }
    nested.push_str(":1\r\n");

    let mut buf = BytesMut::from(nested.as_bytes());
    let result = RespValue::parse(&mut buf);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("maximum nesting depth"),
        "Expected nesting depth error, got: {}",
        err_msg
    );
}
