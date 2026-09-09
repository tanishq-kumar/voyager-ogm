//! Unit tests for PostgreSQL Frontend/Backend v3.0 message codec, authentication, and AGE agtype parser.

use bytes::BytesMut;
use voyager_net::postgres::{
    AgeValue, AuthenticationRequest, BackendMessage, FrontendMessage, PG_PROTOCOL_V3, ScramClient,
    StartupMessage, TransactionStatus, clean_agtype_string, compute_md5_password, parse_agtype,
    rows_to_record_batch,
};

#[test]
fn test_startup_message_encoding() {
    let startup = StartupMessage::new("testuser", Some("testdb"));
    let mut buf = BytesMut::new();
    startup.encode(&mut buf);

    // First 4 bytes are length, next 4 are protocol version
    assert!(buf.len() > 8);
    let mut bytes = buf.freeze();
    use bytes::Buf;
    let len = bytes.get_i32();
    let version = bytes.get_i32();

    assert_eq!(version, PG_PROTOCOL_V3);
    assert_eq!(len as usize, bytes.len() + 8);
}

#[test]
fn test_frontend_query_encoding() {
    let msg = FrontendMessage::Query("SELECT 1;".to_string());
    let mut buf = BytesMut::new();
    msg.encode(&mut buf);

    assert_eq!(buf[0], b'Q');
    let mut bytes = buf.freeze();
    use bytes::Buf;
    bytes.advance(1); // skip 'Q'
    let len = bytes.get_i32();
    assert_eq!(len, 4 + 9 + 1); // 4 (len) + 9 ("SELECT 1;") + 1 (\0)
    assert_eq!(&bytes[..], b"SELECT 1;\0");
}

#[test]
fn test_frontend_parse_bind_execute_sync() {
    let parse = FrontendMessage::Parse {
        name: "stmt1".to_string(),
        query: "SELECT $1::int;".to_string(),
        param_types: vec![23],
    };
    let mut buf = BytesMut::new();
    parse.encode(&mut buf);
    assert_eq!(buf[0], b'P');

    let bind = FrontendMessage::Bind {
        portal: "".to_string(),
        statement: "stmt1".to_string(),
        param_formats: vec![0],
        params: vec![Some(b"42".to_vec())],
        result_formats: vec![0],
    };
    buf.clear();
    bind.encode(&mut buf);
    assert_eq!(buf[0], b'B');

    let exec = FrontendMessage::Execute {
        portal: "".to_string(),
        max_rows: 0,
    };
    buf.clear();
    exec.encode(&mut buf);
    assert_eq!(buf[0], b'E');

    let sync = FrontendMessage::Sync;
    buf.clear();
    sync.encode(&mut buf);
    assert_eq!(buf[0], b'S');
    assert_eq!(buf.len(), 5);
}

#[test]
fn test_backend_message_decoding() {
    // Test AuthenticationOk ('R', 0)
    let mut payload = BytesMut::new();
    use bytes::BufMut;
    payload.put_i32(0);
    let msg = BackendMessage::decode(b'R', payload.freeze()).unwrap();
    assert_eq!(
        msg,
        BackendMessage::Authentication(AuthenticationRequest::Ok)
    );

    // Test AuthenticationMD5Password ('R', 5, salt)
    let mut payload = BytesMut::new();
    payload.put_i32(5);
    payload.put_slice(&[1, 2, 3, 4]);
    let msg = BackendMessage::decode(b'R', payload.freeze()).unwrap();
    assert_eq!(
        msg,
        BackendMessage::Authentication(AuthenticationRequest::MD5Password { salt: [1, 2, 3, 4] })
    );

    // Test ReadyForQuery ('Z', 'I')
    let mut payload = BytesMut::new();
    payload.put_u8(b'I');
    let msg = BackendMessage::decode(b'Z', payload.freeze()).unwrap();
    assert_eq!(msg, BackendMessage::ReadyForQuery(TransactionStatus::Idle));

    // Test CommandComplete ('C', "SELECT 10\0")
    let mut payload = BytesMut::new();
    payload.put_slice(b"SELECT 10\0");
    let msg = BackendMessage::decode(b'C', payload.freeze()).unwrap();
    assert_eq!(
        msg,
        BackendMessage::CommandComplete("SELECT 10".to_string())
    );

    // Test ErrorResponse ('E')
    let mut payload = BytesMut::new();
    payload.put_u8(b'S');
    payload.put_slice(b"ERROR\0");
    payload.put_u8(b'C');
    payload.put_slice(b"42P01\0");
    payload.put_u8(b'M');
    payload.put_slice(b"relation does not exist\0");
    payload.put_u8(0);
    let msg = BackendMessage::decode(b'E', payload.freeze()).unwrap();
    if let BackendMessage::ErrorResponse(diag) = msg {
        assert_eq!(diag.severity, "ERROR");
        assert_eq!(diag.code, "42P01");
        assert_eq!(diag.message, "relation does not exist");
    } else {
        panic!("Expected ErrorResponse");
    }
}

#[test]
fn test_md5_authentication_hash() {
    let user = "postgres";
    let password = "voyagerpass123";
    let salt = [0x12, 0x34, 0x56, 0x78];

    let hash = compute_md5_password(user, password, &salt);
    assert_eq!(hash.len(), 36); // "md5" (3) + 32 hex chars + '\0' (1) = 36
    assert!(hash.starts_with(b"md5"));
    assert_eq!(hash.last(), Some(&0));
}

#[test]
fn test_scram_sha256_client_flow() {
    let mut client = ScramClient::new();
    let first_msg = client.client_first_message();
    let first_msg_str = String::from_utf8(first_msg).unwrap();
    assert!(first_msg_str.starts_with("n,,n=,r="));

    // Extract client nonce
    let client_nonce = first_msg_str.strip_prefix("n,,n=,r=").unwrap();

    // Mock server challenge with iterations=4096 and salt
    let server_nonce = format!("{}mock_server_nonce_12345", client_nonce);
    let salt_b64 = "c2FsdF9leGFtcGxlXzEyMzQ="; // "salt_example_1234"
    let challenge = format!("r={},s={},i=4096", server_nonce, salt_b64);

    let client_final = client
        .process_challenge(challenge.as_bytes(), "secretpassword")
        .unwrap();
    let final_str = String::from_utf8(client_final).unwrap();

    assert!(final_str.starts_with("c=biws,r="));
    assert!(final_str.contains(",p="));
}

#[test]
fn test_agtype_clean_and_parse_scalars() {
    assert_eq!(clean_agtype_string("  42::numeric  "), "42");
    assert_eq!(clean_agtype_string("{\"id\": 1}::vertex"), "{\"id\": 1}");

    // Integer
    let val = parse_agtype("12345").unwrap();
    assert_eq!(val, AgeValue::Integer(12345));

    // Float
    let val = parse_agtype("12.3456").unwrap();
    assert_eq!(val, AgeValue::Float(12.3456));

    // Boolean
    let val = parse_agtype("true").unwrap();
    assert_eq!(val, AgeValue::Boolean(true));

    // String
    let val = parse_agtype("\"hello world\"").unwrap();
    assert_eq!(val, AgeValue::String("hello world".to_string()));

    // Null
    let val = parse_agtype("null").unwrap();
    assert_eq!(val, AgeValue::Null);
}

#[test]
fn test_agtype_vertex_and_edge_parsing() {
    let raw_vertex = r#"{"id": 844424930131969, "label": "Person", "properties": {"name": "Alice", "age": 30}}::vertex"#;
    let parsed = parse_agtype(raw_vertex).unwrap();

    if let AgeValue::Vertex(v) = parsed {
        assert_eq!(v.id, 844424930131969);
        assert_eq!(v.label, "Person");
        assert_eq!(v.properties.get("name").unwrap(), "Alice");
        assert_eq!(v.properties.get("age").unwrap(), 30);

        let bolt_node = v.to_bolt_node();
        assert_eq!(bolt_node.id, 844424930131969);
        assert_eq!(bolt_node.labels, vec!["Person".to_string()]);
    } else {
        panic!("Expected AgeValue::Vertex");
    }

    let raw_edge = r#"{"id": 1125899906842625, "label": "KNOWS", "end_id": 844424930131970, "start_id": 844424930131969, "properties": {"since": 2022}}::edge"#;
    let parsed = parse_agtype(raw_edge).unwrap();

    if let AgeValue::Edge(e) = parsed {
        assert_eq!(e.id, 1125899906842625);
        assert_eq!(e.label, "KNOWS");
        assert_eq!(e.start_id, 844424930131969);
        assert_eq!(e.end_id, 844424930131970);
        assert_eq!(e.properties.get("since").unwrap(), 2022);

        let bolt_rel = e.to_bolt_relationship();
        assert_eq!(bolt_rel.id, 1125899906842625);
        assert_eq!(bolt_rel.rel_type, "KNOWS");
    } else {
        panic!("Expected AgeValue::Edge");
    }
}

#[test]
fn test_agtype_path_parsing() {
    let raw_path = r#"[{"id": 1, "label": "Person", "properties": {"name": "Alice"}}::vertex, {"id": 10, "label": "KNOWS", "start_id": 1, "end_id": 2, "properties": {}}::edge, {"id": 2, "label": "Person", "properties": {"name": "Bob"}}::vertex]::path"#;
    let parsed = parse_agtype(raw_path).unwrap();

    if let AgeValue::Path(p) = parsed {
        assert_eq!(p.vertices.len(), 2);
        assert_eq!(p.edges.len(), 1);
        assert_eq!(p.vertices[0].label, "Person");
        assert_eq!(p.edges[0].label, "KNOWS");

        let bolt_path = p.to_bolt_path();
        assert_eq!(bolt_path.nodes.len(), 2);
        assert_eq!(bolt_path.relationships.len(), 1);
    } else {
        panic!("Expected AgeValue::Path");
    }
}

#[test]
fn test_rows_to_arrow_record_batch() {
    let cols = vec![
        "id".to_string(),
        "name".to_string(),
        "score".to_string(),
        "active".to_string(),
    ];
    let rows = vec![
        vec![
            Some("101".to_string()),
            Some("Alice".to_string()),
            Some("98.5".to_string()),
            Some("t".to_string()),
        ],
        vec![
            Some("102".to_string()),
            Some("Bob".to_string()),
            Some("85.0".to_string()),
            Some("f".to_string()),
        ],
        vec![
            Some("103".to_string()),
            None,
            Some("91.2".to_string()),
            Some("true".to_string()),
        ],
    ];

    let batch = rows_to_record_batch(&cols, &rows).unwrap();
    assert_eq!(batch.num_rows(), 3);
    assert_eq!(batch.num_columns(), 4);

    let schema = batch.schema();
    assert_eq!(schema.field(0).data_type(), &arrow_schema::DataType::Int64);
    assert_eq!(schema.field(1).data_type(), &arrow_schema::DataType::Utf8);
    assert_eq!(
        schema.field(2).data_type(),
        &arrow_schema::DataType::Float64
    );
    assert_eq!(
        schema.field(3).data_type(),
        &arrow_schema::DataType::Boolean
    );
}
