//! Live integration tests verifying CircuitRouter failover with live Bolt TCP sockets.

use std::collections::HashMap;
use std::time::Duration;

use voyager_net::bolt::BoltStubServer;
use voyager_net::circuit::{BackendRoute, CircuitConfig, CircuitRouter, CircuitState};
use voyager_net::client::NativeClient;

#[tokio::test]
async fn test_live_bolt_circuit_router_trip_and_recovery() {
    // 1. Start real live Bolt TCP server on ephemeral port
    let stub = BoltStubServer::start().await.expect("Failed to start stub");
    let uri = stub.uri();

    // 2. Initialize NativeClient connecting to live TCP stub server
    let client = NativeClient::new(&uri, None).expect("Client init failed");

    // 3. Initialize CircuitRouter with 20ms cooldown and 1 failure threshold
    let router = CircuitRouter::new(CircuitConfig::new(1, Duration::from_millis(20)));
    assert_eq!(router.state(), CircuitState::Closed);
    assert_eq!(router.route(), BackendRoute::Native);

    // 4. Normal query over live TCP socket succeeds
    let res = client.execute("RETURN 1", &HashMap::new()).await;
    assert!(res.is_ok(), "Query to live Bolt server should succeed");
    router.record_success();
    assert_eq!(router.state(), CircuitState::Closed);

    // 5. Semantic syntax error returned from live engine does NOT trip the circuit breaker
    let sem_res = client.execute("FAIL SYNTAX", &HashMap::new()).await;
    assert!(sem_res.is_err(), "FAIL query should return database error");
    let sem_err = sem_res.unwrap_err();
    let is_transient = router.record_net_error(&sem_err);
    assert!(
        !is_transient,
        "SyntaxError from live database must be classified as semantic"
    );
    assert_eq!(router.state(), CircuitState::Closed);
    assert_eq!(router.route(), BackendRoute::Native);

    // 6. Network socket failure (connecting to unreachable TCP endpoint)
    // Find an ephemeral port without a listener
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener); // Free port so connection will be refused by OS

    let dead_client = NativeClient::new(&format!("bolt://127.0.0.1:{}", dead_port), None).unwrap();
    let fail_res = dead_client.execute("RETURN 1", &HashMap::new()).await;
    assert!(fail_res.is_err(), "Unreachable socket must fail");
    let net_err = fail_res.unwrap_err();

    // 7. Circuit router classifies live network connection refusal as transient and trips to Open
    let is_transient = router.record_net_error(&net_err);
    assert!(
        is_transient,
        "ConnectionRefused must be classified as transient"
    );
    assert_eq!(router.state(), CircuitState::Open);
    assert_eq!(router.route(), BackendRoute::Fallback);

    // 8. Subsequent requests route to fallback during cooldown
    assert_eq!(router.route(), BackendRoute::Fallback);

    // 9. Wait for cooldown duration to elapse
    tokio::time::sleep(Duration::from_millis(35)).await;

    // 10. Circuit enters HalfOpen and routes trial Probe
    assert_eq!(router.state(), CircuitState::HalfOpen);
    assert_eq!(router.route(), BackendRoute::Probe);

    // 11. Trial probe query against the live healthy Bolt engine succeeds
    let probe_res = client.execute("RETURN 1", &HashMap::new()).await;
    assert!(probe_res.is_ok(), "Live probe query should succeed");
    router.record_success();

    // 12. Circuit fully recovers to Closed state
    assert_eq!(router.state(), CircuitState::Closed);
    assert_eq!(router.route(), BackendRoute::Native);
}
