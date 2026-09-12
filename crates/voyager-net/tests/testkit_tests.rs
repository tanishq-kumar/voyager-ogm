//! Official Neo4j TestKit protocol compliance and IPC wire tests for voyager-net.

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use voyager_net::bolt::{BoltNode, BoltRelationship, BoltValue, TestkitBackend};

const NEO4J_URI: &str = "bolt://127.0.0.1:7687";
const NEO4J_USER: &str = "neo4j";
const NEO4J_PASS: &str = "voyagerpass123";

async fn send_step(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
    request: Value,
) -> Value {
    let req_str = serde_json::to_string(&request).unwrap();
    let framed = format!(
        "#=== step: start =============\n{}\n#=== step: end =============\n",
        req_str
    );
    writer.write_all(framed.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();

    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        if line.trim() == "#=== step: start =============" {
            let mut json_str = String::new();
            loop {
                let mut step_line = String::new();
                reader.read_line(&mut step_line).await.unwrap();
                if step_line.trim() == "#=== step: end =============" {
                    break;
                }
                json_str.push_str(&step_line);
            }
            return serde_json::from_str(&json_str).unwrap();
        }
    }
}

#[test]
fn test_testkit_cypher_types_conversions() {
    // 1. Integer
    let val_int = BoltValue::Integer(42);
    let json_int = TestkitBackend::bolt_value_to_cypher_json(&val_int);
    assert_eq!(
        json_int,
        json!({"name": "CypherInt", "data": {"value": 42}})
    );
    assert_eq!(
        TestkitBackend::cypher_json_to_bolt_value(&json_int),
        val_int
    );

    // 2. String
    let val_str = BoltValue::String("hello testkit".to_string());
    let json_str = TestkitBackend::bolt_value_to_cypher_json(&val_str);
    assert_eq!(
        json_str,
        json!({"name": "CypherString", "data": {"value": "hello testkit"}})
    );
    assert_eq!(
        TestkitBackend::cypher_json_to_bolt_value(&json_str),
        val_str
    );

    // 3. Boolean
    let val_bool = BoltValue::Boolean(true);
    let json_bool = TestkitBackend::bolt_value_to_cypher_json(&val_bool);
    assert_eq!(
        json_bool,
        json!({"name": "CypherBool", "data": {"value": true}})
    );
    assert_eq!(
        TestkitBackend::cypher_json_to_bolt_value(&json_bool),
        val_bool
    );

    // 4. Float
    let val_float = BoltValue::Float(123.456);
    let json_float = TestkitBackend::bolt_value_to_cypher_json(&val_float);
    assert_eq!(
        json_float,
        json!({"name": "CypherFloat", "data": {"value": 123.456}})
    );
    assert_eq!(
        TestkitBackend::cypher_json_to_bolt_value(&json_float),
        val_float
    );

    // 5. Null
    let val_null = BoltValue::Null;
    let json_null = TestkitBackend::bolt_value_to_cypher_json(&val_null);
    assert_eq!(json_null, json!({"name": "CypherNull"}));
    assert_eq!(
        TestkitBackend::cypher_json_to_bolt_value(&json_null),
        val_null
    );

    // 6. Node
    let mut props = std::collections::HashMap::new();
    props.insert("name".to_string(), BoltValue::String("Alice".to_string()));
    let node = BoltNode {
        id: 1,
        labels: vec!["Person".to_string()],
        properties: props,
        element_id: Some("4:1".to_string()),
    };
    let json_node = TestkitBackend::bolt_value_to_cypher_json(&BoltValue::Node(node));
    assert_eq!(json_node["name"], "Node");
    assert_eq!(json_node["data"]["id"]["data"]["value"], 1);

    // 7. Relationship
    let rel = BoltRelationship {
        id: 10,
        start_node_id: 1,
        end_node_id: 2,
        rel_type: "KNOWS".to_string(),
        properties: std::collections::HashMap::new(),
        element_id: Some("5:10".to_string()),
        start_element_id: None,
        end_element_id: None,
    };
    let json_rel = TestkitBackend::bolt_value_to_cypher_json(&BoltValue::Relationship(rel));
    assert_eq!(json_rel["name"], "Relationship");
    assert_eq!(json_rel["data"]["type"]["data"]["value"], "KNOWS");
}

#[tokio::test]
async fn test_testkit_backend_handshake_and_features() {
    let backend = TestkitBackend::new();

    // GetFeatures
    let feat_resp = backend
        .process_request(json!({"name": "GetFeatures"}))
        .await;
    assert_eq!(feat_resp["name"], "FeatureList");
    let features = feat_resp["data"]["features"].as_array().unwrap();
    assert!(features.iter().any(|f| f == "Feature:Bolt:5.4"));
    assert!(features.iter().any(|f| f == "Feature:Bolt:5.0"));
    assert!(features.iter().any(|f| f == "Optimization:PullPipelining"));

    // StartTest
    let start_resp = backend
        .process_request(json!({"name": "StartTest", "data": {"testName": "test_connection"}}))
        .await;
    assert_eq!(start_resp["name"], "RunTest");
}

#[tokio::test]
async fn test_testkit_backend_over_tcp_socket() {
    // 1. Start TestKit backend TCP server on ephemeral port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let _ = TestkitBackend::run_server(listener).await;
    });

    // 2. Connect client to TestKit backend server
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let (reader, mut writer) = socket.split();
    let mut buf_reader = BufReader::new(reader);

    // 3. Handshake: GetFeatures
    let feat_resp = send_step(&mut buf_reader, &mut writer, json!({"name": "GetFeatures"})).await;
    assert_eq!(feat_resp["name"], "FeatureList");

    // 4. StartTest
    let start_resp = send_step(
        &mut buf_reader,
        &mut writer,
        json!({"name": "StartTest", "data": {"testName": "test_basic_query"}}),
    )
    .await;
    assert_eq!(start_resp["name"], "RunTest");

    // 5. Connect to running Neo4j container
    let driver_resp = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "NewDriver",
            "data": {
                "uri": NEO4J_URI,
                "authorizationToken": {
                    "scheme": "basic",
                    "principal": NEO4J_USER,
                    "credentials": NEO4J_PASS
                }
            }
        }),
    )
    .await;

    if driver_resp["name"] == "DriverError" {
        eprintln!("[SKIP] Neo4j container not available for live TestKit test");
        return;
    }

    assert_eq!(driver_resp["name"], "Driver");
    let driver_id = driver_resp["data"]["id"].as_str().unwrap();

    // 6. NewSession
    let session_resp = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "NewSession",
            "data": {
                "driverId": driver_id
            }
        }),
    )
    .await;
    if session_resp["name"] == "DriverError" {
        eprintln!("[SKIP] Neo4j container not available for live TestKit test");
        return;
    }
    assert_eq!(session_resp["name"], "Session");
    let session_id = session_resp["data"]["id"].as_str().unwrap();

    // 7. SessionRun: Parameterized query
    let run_resp = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "SessionRun",
            "data": {
                "sessionId": session_id,
                "cypher": "UNWIND [1, 2, 3] AS n RETURN n, $msg AS greeting",
                "params": {
                    "msg": {
                        "name": "CypherString",
                        "data": {"value": "hello_from_testkit"}
                    }
                }
            }
        }),
    )
    .await;
    assert_eq!(run_resp["name"], "Result");
    assert_eq!(run_resp["data"]["keys"], json!(["n", "greeting"]));
    let result_id = run_resp["data"]["id"].as_str().unwrap();

    // 8. ResultNext: 1st record
    let next_resp1 = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "ResultNext",
            "data": {
                "resultId": result_id
            }
        }),
    )
    .await;
    assert_eq!(next_resp1["name"], "Record");
    assert_eq!(next_resp1["data"]["values"][0]["data"]["value"], 1);
    assert_eq!(
        next_resp1["data"]["values"][1]["data"]["value"],
        "hello_from_testkit"
    );

    // 9. ResultNext: 2nd record
    let next_resp2 = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "ResultNext",
            "data": {
                "resultId": result_id
            }
        }),
    )
    .await;
    assert_eq!(next_resp2["name"], "Record");
    assert_eq!(next_resp2["data"]["values"][0]["data"]["value"], 2);

    // 10. ResultNext: 3rd record
    let next_resp3 = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "ResultNext",
            "data": {
                "resultId": result_id
            }
        }),
    )
    .await;
    assert_eq!(next_resp3["name"], "Record");
    assert_eq!(next_resp3["data"]["values"][0]["data"]["value"], 3);

    // 11. ResultNext: end of stream (NullRecord)
    let next_resp_end = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "ResultNext",
            "data": {
                "resultId": result_id
            }
        }),
    )
    .await;
    assert_eq!(next_resp_end["name"], "NullRecord");

    // 12. ResultConsume
    let consume_resp = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "ResultConsume",
            "data": {
                "resultId": result_id
            }
        }),
    )
    .await;
    assert_eq!(consume_resp["name"], "Summary");

    // 13. SessionClose
    let session_close_resp = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "SessionClose",
            "data": {
                "sessionId": session_id
            }
        }),
    )
    .await;
    assert_eq!(session_close_resp["name"], "Session");

    // 14. DriverClose
    let driver_close_resp = send_step(
        &mut buf_reader,
        &mut writer,
        json!({
            "name": "DriverClose",
            "data": {
                "driverId": driver_id
            }
        }),
    )
    .await;
    assert_eq!(driver_close_resp["name"], "Driver");
}
