//! Unit tests for Redis RESP2/RESP3 protocol engine, FalkorDB result parsing, and Arrow conversion.

use bytes::BytesMut;
use std::collections::HashMap;
use tokio_util::codec::{Decoder, Encoder};

use voyager_net::redis::{
    FalkorNode, FalkorQueryResult, FalkorRelationship, FalkorStatistics, RespCodec, RespValue,
    format_cypher_with_params,
};

#[test]
fn test_resp2_simple_types_encoding_and_decoding() {
    // SimpleString
    let val = RespValue::SimpleString("OK".to_string());
    assert_eq!(val.to_bytes(), b"+OK\r\n");
    let mut buf = BytesMut::from(&val.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(val));

    // Error
    let err = RespValue::Error("ERR unknown command".to_string());
    assert_eq!(err.to_bytes(), b"-ERR unknown command\r\n");
    let mut buf = BytesMut::from(&err.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(err));

    // Integer
    let int_val = RespValue::Integer(42);
    assert_eq!(int_val.to_bytes(), b":42\r\n");
    let mut buf = BytesMut::from(&int_val.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(int_val));

    // Negative Integer
    let neg_val = RespValue::Integer(-100);
    assert_eq!(neg_val.to_bytes(), b":-100\r\n");
    let mut buf = BytesMut::from(&neg_val.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(neg_val));
}

#[test]
fn test_resp2_bulk_strings_and_arrays() {
    // Non-empty BulkString
    let bulk = RespValue::BulkString(Some(b"hello world".to_vec()));
    assert_eq!(bulk.to_bytes(), b"$11\r\nhello world\r\n");
    let mut buf = BytesMut::from(&bulk.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(bulk));

    // Empty BulkString
    let empty_bulk = RespValue::BulkString(Some(vec![]));
    assert_eq!(empty_bulk.to_bytes(), b"$0\r\n\r\n");
    let mut buf = BytesMut::from(&empty_bulk.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(empty_bulk));

    // Null BulkString ($-1)
    let null_bulk = RespValue::BulkString(None);
    assert_eq!(null_bulk.to_bytes(), b"$-1\r\n");
    assert!(null_bulk.is_null());
    let mut buf = BytesMut::from(&null_bulk.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(null_bulk));

    // Array of elements
    let arr = RespValue::Array(Some(vec![
        RespValue::SimpleString("foo".to_string()),
        RespValue::Integer(10),
    ]));
    assert_eq!(arr.to_bytes(), b"*2\r\n+foo\r\n:10\r\n");
    let mut buf = BytesMut::from(&arr.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(arr));

    // Null Array (*-1)
    let null_arr = RespValue::Array(None);
    assert_eq!(null_arr.to_bytes(), b"*-1\r\n");
    assert!(null_arr.is_null());
    let mut buf = BytesMut::from(&null_arr.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(null_arr));
}

#[test]
fn test_resp3_data_types() {
    // Null (_)
    let null_val = RespValue::Null;
    assert_eq!(null_val.to_bytes(), b"_\r\n");
    assert!(null_val.is_null());
    let mut buf = BytesMut::from(&null_val.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(null_val));

    // Boolean (#t, #f)
    let b_true = RespValue::Boolean(true);
    let b_false = RespValue::Boolean(false);
    assert_eq!(b_true.to_bytes(), b"#t\r\n");
    assert_eq!(b_false.to_bytes(), b"#f\r\n");
    let mut buf = BytesMut::from(&b_true.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(b_true));
    let mut buf = BytesMut::from(&b_false.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(b_false));

    // Double (,42.125)
    let d = RespValue::Double(42.125);
    let mut buf = BytesMut::from(&d.to_bytes()[..]);
    let parsed = RespValue::parse(&mut buf).unwrap().unwrap();
    assert!((parsed.as_f64().unwrap() - 42.125).abs() < 1e-5);

    // Double Infinity / NaN
    let pos_inf = RespValue::Double(f64::INFINITY);
    let neg_inf = RespValue::Double(f64::NEG_INFINITY);
    assert_eq!(pos_inf.to_bytes(), b",inf\r\n");
    assert_eq!(neg_inf.to_bytes(), b",-inf\r\n");
    let mut buf = BytesMut::from(&pos_inf.to_bytes()[..]);
    assert_eq!(
        RespValue::parse(&mut buf).unwrap(),
        Some(RespValue::Double(f64::INFINITY))
    );

    // BlobError (!21\r\nSYNTAX invalid syntax\r\n)
    let blob_err = RespValue::BlobError(b"SYNTAX invalid syntax".to_vec());
    assert_eq!(blob_err.to_bytes(), b"!21\r\nSYNTAX invalid syntax\r\n");
    let mut buf = BytesMut::from(&blob_err.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(blob_err));

    // VerbatimString (=15\r\ntxt:Some format\r\n)
    let verbatim = RespValue::VerbatimString {
        format: *b"txt",
        text: b"Some format".to_vec(),
    };
    assert_eq!(verbatim.to_bytes(), b"=15\r\ntxt:Some format\r\n");
    let mut buf = BytesMut::from(&verbatim.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(verbatim));

    // BigNumber ((3492890328409238509324850943850943825024385)
    let big_num = RespValue::BigNumber("3492890328409238509324850943850943825024385".to_string());
    assert_eq!(
        big_num.to_bytes(),
        b"(3492890328409238509324850943850943825024385\r\n"
    );
    let mut buf = BytesMut::from(&big_num.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(big_num));

    // Map (%2\r\n+key1\r\n:100\r\n+key2\r\n:200\r\n)
    let map = RespValue::Map(vec![
        (
            RespValue::SimpleString("key1".to_string()),
            RespValue::Integer(100),
        ),
        (
            RespValue::SimpleString("key2".to_string()),
            RespValue::Integer(200),
        ),
    ]);
    let mut buf = BytesMut::from(&map.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(map));

    // Set (~2\r\n+item1\r\n+item2\r\n)
    let set = RespValue::Set(vec![
        RespValue::SimpleString("item1".to_string()),
        RespValue::SimpleString("item2".to_string()),
    ]);
    let mut buf = BytesMut::from(&set.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(set));

    // Push (>1\r\n+pubsub_msg\r\n)
    let push = RespValue::Push(vec![RespValue::SimpleString("pubsub_msg".to_string())]);
    assert_eq!(push.to_bytes(), b">1\r\n+pubsub_msg\r\n");
    let mut buf = BytesMut::from(&push.to_bytes()[..]);
    assert_eq!(RespValue::parse(&mut buf).unwrap(), Some(push));
}

#[test]
fn test_resp_codec_partial_frames() {
    let mut codec = RespCodec::new();
    let mut buf = BytesMut::new();

    // Incomplete SimpleString
    buf.extend_from_slice(b"+OK");
    assert_eq!(codec.decode(&mut buf).unwrap(), None);

    // Complete the line
    buf.extend_from_slice(b"\r\n");
    assert_eq!(
        codec.decode(&mut buf).unwrap(),
        Some(RespValue::SimpleString("OK".to_string()))
    );
    assert!(buf.is_empty());

    // Incomplete BulkString header
    buf.extend_from_slice(b"$5\r\nhe");
    assert_eq!(codec.decode(&mut buf).unwrap(), None);

    // Complete BulkString payload
    buf.extend_from_slice(b"llo\r\n");
    assert_eq!(
        codec.decode(&mut buf).unwrap(),
        Some(RespValue::BulkString(Some(b"hello".to_vec())))
    );
    assert!(buf.is_empty());
}

#[test]
fn test_resp_codec_encoder() {
    let mut codec = RespCodec::new();
    let mut buf = BytesMut::new();

    let val = RespValue::Array(Some(vec![
        RespValue::BulkString(Some(b"GRAPH.QUERY".to_vec())),
        RespValue::BulkString(Some(b"social".to_vec())),
        RespValue::BulkString(Some(b"MATCH (n) RETURN n".to_vec())),
    ]));

    codec.encode(val, &mut buf).unwrap();
    assert_eq!(
        &buf[..],
        b"*3\r\n$11\r\nGRAPH.QUERY\r\n$6\r\nsocial\r\n$18\r\nMATCH (n) RETURN n\r\n"
    );
}

#[test]
fn test_falkor_node_parsing() {
    // FalkorDB node structure in RESP:
    // [[ "id", 10 ], [ "labels", [ "Person", "Employee" ] ], [ "properties", [ [ "name", "Alice" ], [ "age", 30 ] ] ]]
    let raw = RespValue::Array(Some(vec![
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"id".to_vec())),
            RespValue::Integer(10),
        ])),
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"labels".to_vec())),
            RespValue::Array(Some(vec![
                RespValue::BulkString(Some(b"Person".to_vec())),
                RespValue::BulkString(Some(b"Employee".to_vec())),
            ])),
        ])),
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"properties".to_vec())),
            RespValue::Array(Some(vec![
                RespValue::Array(Some(vec![
                    RespValue::BulkString(Some(b"name".to_vec())),
                    RespValue::BulkString(Some(b"Alice".to_vec())),
                ])),
                RespValue::Array(Some(vec![
                    RespValue::BulkString(Some(b"age".to_vec())),
                    RespValue::Integer(30),
                ])),
            ])),
        ])),
    ]));

    let node = FalkorNode::parse(&raw).expect("Failed to parse FalkorNode");
    assert_eq!(node.id, 10);
    assert_eq!(node.labels, vec!["Person", "Employee"]);
    assert_eq!(node.properties.get("name").unwrap().as_str(), Some("Alice"));
    assert_eq!(node.properties.get("age").unwrap().as_i64(), Some(30));

    let bolt_node = node.to_bolt_node();
    assert_eq!(bolt_node.id, 10);
    assert_eq!(bolt_node.labels, vec!["Person", "Employee"]);
    assert_eq!(
        bolt_node.properties.get("name").unwrap().as_str(),
        Some("Alice")
    );
}

#[test]
fn test_falkor_relationship_parsing() {
    // FalkorDB edge structure in RESP:
    // [[ "id", 55 ], [ "type", "KNOWS" ], [ "src_node", 10 ], [ "dest_node", 20 ], [ "properties", [ [ "since", 2021 ] ] ]]
    let raw = RespValue::Array(Some(vec![
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"id".to_vec())),
            RespValue::Integer(55),
        ])),
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"type".to_vec())),
            RespValue::BulkString(Some(b"KNOWS".to_vec())),
        ])),
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"src_node".to_vec())),
            RespValue::Integer(10),
        ])),
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"dest_node".to_vec())),
            RespValue::Integer(20),
        ])),
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"properties".to_vec())),
            RespValue::Array(Some(vec![RespValue::Array(Some(vec![
                RespValue::BulkString(Some(b"since".to_vec())),
                RespValue::Integer(2021),
            ]))])),
        ])),
    ]));

    let rel = FalkorRelationship::parse(&raw).expect("Failed to parse FalkorRelationship");
    assert_eq!(rel.id, 55);
    assert_eq!(rel.rel_type, "KNOWS");
    assert_eq!(rel.src_node, 10);
    assert_eq!(rel.dest_node, 20);
    assert_eq!(rel.properties.get("since").unwrap().as_i64(), Some(2021));

    let bolt_rel = rel.to_bolt_relationship();
    assert_eq!(bolt_rel.id, 55);
    assert_eq!(bolt_rel.rel_type, "KNOWS");
    assert_eq!(bolt_rel.start_node_id, 10);
    assert_eq!(bolt_rel.end_node_id, 20);
}

#[test]
fn test_falkor_statistics_parsing() {
    let raw = vec![
        RespValue::BulkString(Some(b"Nodes created: 2".to_vec())),
        RespValue::BulkString(Some(b"Properties set: 5".to_vec())),
        RespValue::BulkString(Some(b"Relationships created: 1".to_vec())),
        RespValue::BulkString(Some(b"Cached execution: 1".to_vec())),
        RespValue::BulkString(Some(
            b"Query internal execution time: 0.456789 milliseconds".to_vec(),
        )),
    ];

    let stats = FalkorStatistics::parse(&raw);
    assert_eq!(stats.nodes_created, 2);
    assert_eq!(stats.properties_set, 5);
    assert_eq!(stats.relationships_created, 1);
    assert!(stats.cached_execution);
    assert!((stats.internal_execution_time_ms - 0.456789).abs() < 1e-4);

    let summary = stats.to_query_summary();
    assert_eq!(summary.nodes_created, 2);
    assert_eq!(summary.properties_set, 5);
    assert_eq!(summary.relationships_created, 1);
    assert_eq!(summary.execution_time_ms, 0); // floor of 0.45 ms
}

#[test]
fn test_falkor_query_result_to_arrow_record_batch() {
    // Simulate 3-element RESP response: [Headers, Rows, Statistics]
    let headers = RespValue::Array(Some(vec![
        RespValue::BulkString(Some(b"name".to_vec())),
        RespValue::BulkString(Some(b"age".to_vec())),
        RespValue::BulkString(Some(b"score".to_vec())),
        RespValue::BulkString(Some(b"active".to_vec())),
    ]));

    let rows = RespValue::Array(Some(vec![
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"Alice".to_vec())),
            RespValue::Integer(30),
            RespValue::Double(95.5),
            RespValue::Boolean(true),
        ])),
        RespValue::Array(Some(vec![
            RespValue::BulkString(Some(b"Bob".to_vec())),
            RespValue::Integer(25),
            RespValue::Double(88.0),
            RespValue::Boolean(false),
        ])),
        RespValue::Array(Some(vec![
            RespValue::Null,
            RespValue::Null,
            RespValue::Null,
            RespValue::Null,
        ])),
    ]));

    let statistics = RespValue::Array(Some(vec![
        RespValue::BulkString(Some(b"Cached execution: 0".to_vec())),
        RespValue::BulkString(Some(
            b"Query internal execution time: 1.25 milliseconds".to_vec(),
        )),
    ]));

    let full_resp = RespValue::Array(Some(vec![headers, rows, statistics]));
    let query_res = FalkorQueryResult::parse(full_resp).expect("Failed to parse FalkorQueryResult");

    assert_eq!(query_res.columns, vec!["name", "age", "score", "active"]);
    assert_eq!(query_res.rows.len(), 3);

    // Convert to Arrow RecordBatch
    let batch = query_res
        .to_record_batch()
        .expect("Failed to convert to Arrow RecordBatch");

    assert_eq!(batch.num_columns(), 4);
    assert_eq!(batch.num_rows(), 3);

    use arrow::array::{Array, BooleanArray, Float64Array, Int64Array, StringArray};
    use arrow::datatypes::DataType;

    // Col 0: Utf8
    assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
    let str_col = batch
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(str_col.value(0), "Alice");
    assert_eq!(str_col.value(1), "Bob");
    assert!(str_col.is_null(2));

    // Col 1: Int64
    assert_eq!(batch.schema().field(1).data_type(), &DataType::Int64);
    let int_col = batch
        .column(1)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(int_col.value(0), 30);
    assert_eq!(int_col.value(1), 25);
    assert!(int_col.is_null(2));

    // Col 2: Float64
    assert_eq!(batch.schema().field(2).data_type(), &DataType::Float64);
    let float_col = batch
        .column(2)
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    assert_eq!(float_col.value(0), 95.5);
    assert_eq!(float_col.value(1), 88.0);
    assert!(float_col.is_null(2));

    // Col 3: Boolean
    assert_eq!(batch.schema().field(3).data_type(), &DataType::Boolean);
    let bool_col = batch
        .column(3)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .unwrap();
    assert!(bool_col.value(0));
    assert!(!bool_col.value(1));
    assert!(bool_col.is_null(2));
}

#[test]
fn test_format_cypher_with_params() {
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("Alice"));
    params.insert("age".to_string(), serde_json::json!(30));
    params.insert("active".to_string(), serde_json::json!(true));

    let query = "MATCH (n:Person {name: $name, age: $age}) RETURN n";
    let formatted = format_cypher_with_params(query, &params);

    assert!(formatted.starts_with("CYPHER "));
    assert!(formatted.contains("name=\"Alice\""));
    assert!(formatted.contains("age=30"));
    assert!(formatted.contains("active=true"));
    assert!(formatted.ends_with(query));

    // Empty params returns unchanged query
    let empty_params = HashMap::new();
    let unchanged = format_cypher_with_params(query, &empty_params);
    assert_eq!(unchanged, query);
}
