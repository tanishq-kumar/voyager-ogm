use std::collections::HashMap;
use voyager_core::bridge::{DatabaseBridge, MockDatabaseBridge, QueryResult, QuerySummary};
use voyager_core::visitor::CompiledQuery;

#[test]
fn test_mock_database_bridge_execution_recording() {
    let bridge = MockDatabaseBridge::new();

    let query1 = CompiledQuery {
        statement: "MATCH (p:Person) RETURN p.name".to_string(),
        parameters: HashMap::new(),
    };

    let query2 = CompiledQuery {
        statement: "CREATE (p:Person {name: $p0})".to_string(),
        parameters: {
            let mut map = HashMap::new();
            map.insert("p0".to_string(), "Alice".into());
            map
        },
    };

    let res1 = bridge.execute(&query1).unwrap();
    assert!(res1.rows.is_empty());

    let res2 = bridge.execute(&query2).unwrap();
    assert!(res2.rows.is_empty());

    let history = bridge.get_executed_queries();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].statement, "MATCH (p:Person) RETURN p.name");
    assert_eq!(history[1].statement, "CREATE (p:Person {name: $p0})");
}

#[test]
fn test_mock_database_bridge_canned_results() {
    let bridge = MockDatabaseBridge::new();

    let canned = QueryResult {
        columns: vec!["name".to_string(), "age".to_string()],
        rows: vec![{
            let mut row = HashMap::new();
            row.insert("name".to_string(), "Alice".to_string());
            row.insert("age".to_string(), "30".to_string());
            row
        }],
        summary: QuerySummary {
            nodes_affected: 1,
            relationships_affected: 0,
            execution_time: std::time::Duration::from_millis(5),
        },
    };

    bridge.queue_result(canned);

    let query = CompiledQuery {
        statement: "MATCH (p:Person) RETURN p.name, p.age".to_string(),
        parameters: HashMap::new(),
    };

    let result = bridge.execute(&query).unwrap();
    assert_eq!(result.columns, vec!["name", "age"]);
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get("name").unwrap(), "Alice");
    assert_eq!(result.rows[0].get("age").unwrap(), "30");
    assert_eq!(result.summary.nodes_affected, 1);
}
