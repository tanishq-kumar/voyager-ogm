//! Standalone Neo4j TestKit backend binary for official TestKit integration.

use tokio::net::TcpListener;
use voyager_net::bolt::TestkitBackend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::var("TEST_BACKEND_PORT")
        .unwrap_or_else(|_| "9876".to_string())
        .parse::<u16>()
        .unwrap_or(9876);
    let host = std::env::var("TEST_BACKEND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr = format!("{}:{}", host, port);

    println!("[TESTKIT BACKEND] Listening on TCP {}", addr);
    let listener = TcpListener::bind(&addr).await?;
    TestkitBackend::run_server(listener).await?;
    Ok(())
}
