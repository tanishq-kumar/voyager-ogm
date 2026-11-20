//! Voyager OGM: Live Engine Connection & Parameterized Query Execution
//!
//! Teaches:
//! - Connecting to live graph engines over Bolt protocol using `voyager-drivers`
//! - Testing connectivity with connection pooling via `NativeClient`
//! - Executing parameterized queries using `client.execute(&query, &params)`
//! - Inspecting columnar `QueryResult` and Apache Arrow record batches
//! - Graceful offline skip handling when database containers are not running
//!
//! Prerequisites:
//! - Engine: Neo4j (bolt://127.0.0.1:7687) and/or Memgraph (bolt://127.0.0.1:7688) running via:
//!   `docker compose -f containers/compose.yaml up -d neo4j memgraph`
//!   (or `podman compose -f containers/compose.yaml up -d neo4j memgraph`)
//! - Crates: voyager-drivers, voyager-ogm, tokio
//!
//! Run with:
//! `cargo run -p voyager-drivers --example live_connect_and_query`

use std::collections::HashMap;
use voyager_drivers::bolt::BoltConnection;
use voyager_drivers::client::NativeClient;
use voyager_drivers::config::ConnectionConfig;
use voyager_drivers::engine::AsyncConnection;

const NEO4J_URI: &str = "bolt://127.0.0.1:7687";
const NEO4J_USER: &str = "neo4j";
const NEO4J_PASS: &str = "voyagerpass123";

const MEMGRAPH_URI: &str = "bolt://127.0.0.1:7688";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Voyager OGM (Rust) - Live Engine Connection & Query Execution ===\n");

    let mut engines_tested = 0;

    // -----------------------------------------------------------------------
    // 1. Live Neo4j Direct Wire Execution via BoltConnection
    // -----------------------------------------------------------------------
    println!("--- [1] Neo4j Bolt Connection (Port 7687) ---");
    let neo4j_cfg = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);

    match BoltConnection::connect(&neo4j_cfg).await {
        Ok(mut conn) => {
            if let Some(agent) = conn.server_agent() {
                println!("Connected to Neo4j agent: {}", agent);
            }

            let mut params = HashMap::new();
            params.insert("num".to_string(), serde_json::json!(42));
            params.insert("msg".to_string(), serde_json::json!("Hello from Rust!"));

            let query = "RETURN $num AS answer, $msg AS greeting";
            println!("Executing parameterized query: {}", query);

            let res = conn.execute(query, &params).await?;
            println!(
                "Summary: execution_time = {}ms",
                res.summary.execution_time_ms
            );
            println!("Returned RecordBatches: {}", res.batches.len());
            for (idx, batch) in res.batches.iter().enumerate() {
                println!(
                    "  Batch #{}: {} rows, {} columns",
                    idx,
                    batch.num_rows(),
                    batch.num_columns()
                );
            }

            conn.close().await?;
            engines_tested += 1;
        }
        Err(e) => {
            println!(
                "[SKIP] Neo4j is not reachable on {} (start with `docker compose -f containers/compose.yaml up -d neo4j` or `podman compose ...`): {}",
                NEO4J_URI, e
            );
        }
    }
    println!();

    // -----------------------------------------------------------------------
    // 2. Live Memgraph Connection via High-Level NativeClient Pool
    // -----------------------------------------------------------------------
    println!("--- [2] Memgraph via NativeClient Pool (Port 7688) ---");
    let client = NativeClient::new(MEMGRAPH_URI, None)?;

    match client.ping().await {
        Ok(()) => {
            println!("Successfully connected and pinged Memgraph via NativeClient pool!");

            let mut params = HashMap::new();
            params.insert("service".to_string(), serde_json::json!("auth-gateway"));
            params.insert("latency_ms".to_string(), serde_json::json!(4.25));

            let query = "RETURN $service AS name, $latency_ms AS latency";
            let res = client.execute(query, &params).await?;
            println!(
                "Query executed successfully via connection pool! RecordBatches: {}",
                res.batches.len()
            );
            engines_tested += 1;
        }
        Err(e) => {
            println!(
                "[SKIP] Memgraph is not reachable on {} (start with `docker compose -f containers/compose.yaml up -d memgraph` or `podman compose ...`): {}",
                MEMGRAPH_URI, e
            );
        }
    }
    println!();

    if engines_tested == 0 {
        println!(
            "Note: No live engine containers were running. Start the engines with your Compose runner:"
        );
        println!("  docker compose -f containers/compose.yaml up -d neo4j memgraph");
        println!("  # or: podman compose -f containers/compose.yaml up -d neo4j memgraph");
    } else {
        println!(
            "[SUCCESS] Live engine connection and query execution verified across {} engine(s).",
            engines_tested
        );
    }

    Ok(())
}
