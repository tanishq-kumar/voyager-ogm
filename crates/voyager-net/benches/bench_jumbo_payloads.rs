//! Large-payload (10MB, 25MB, 50MB) TCP stream stress benchmark for voyager-net.

use std::collections::HashMap;
use std::time::Instant;
use voyager_net::bolt::BoltConnection;
use voyager_net::config::ConnectionConfig;
use voyager_net::engine::AsyncConnection;

const NEO4J_URI: &str = "bolt://127.0.0.1:7687";
const NEO4J_USER: &str = "neo4j";
const NEO4J_PASS: &str = "voyagerpass123";

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
    sorted[idx]
}

#[derive(Debug, serde::Serialize)]
struct LargePayloadMetric {
    payload_size_mb: usize,
    iterations: usize,
    p50_ms: f64,
    p90_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    throughput_mb_s: f64,
}

impl LargePayloadMetric {
    fn print(&self) {
        println!(
            "| {:>15} MB | {:>6} | {:>10.2} ms | {:>10.2} ms | {:>10.2} ms | {:>10.2} ms | {:>12.2} MB/s |",
            self.payload_size_mb,
            self.iterations,
            self.p50_ms,
            self.p90_ms,
            self.p99_ms,
            self.max_ms,
            self.throughput_mb_s
        );
    }
}

async fn run_size_benchmark(
    conn: &mut BoltConnection,
    size_mb: usize,
    iterations: usize,
) -> Result<LargePayloadMetric, Box<dyn std::error::Error>> {
    let payload = "X".repeat(size_mb * 1024 * 1024);
    let mut params = HashMap::new();
    params.insert("data".to_string(), serde_json::Value::String(payload));
    let query = "RETURN $data AS payload, size($data) AS byte_len";

    let mut latencies = Vec::new();
    for _ in 0..iterations {
        let t0 = Instant::now();
        let res = conn.execute(query, &params).await?;
        assert_eq!(res.row_count(), 1);
        let batch = res.into_single_batch()?.unwrap();
        assert_eq!(batch.num_rows(), 1);
        latencies.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = percentile(&latencies, 50.0);
    let p90 = percentile(&latencies, 90.0);
    let p99 = percentile(&latencies, 99.0);
    let max = *latencies.last().unwrap_or(&0.0);

    // Throughput in MB/s (size_mb transmitted and received = 2x size_mb per roundtrip)
    let avg_ms = latencies.iter().sum::<f64>() / (latencies.len() as f64);
    let throughput_mb_s = if avg_ms > 0.0 {
        ((size_mb * 2) as f64) / (avg_ms / 1000.0)
    } else {
        0.0
    };

    Ok(LargePayloadMetric {
        payload_size_mb: size_mb,
        iterations,
        p50_ms: p50,
        p90_ms: p90,
        p99_ms: p99,
        max_ms: max,
        throughput_mb_s,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--list") {
        return Ok(());
    }
    let config = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);
    let mut conn = BoltConnection::connect(&config).await?;

    println!(
        "\n========================================================================================================"
    );
    println!("                     VOYAGER-NET JUMBO PAYLOAD STREAMING BENCHMARK (NATIVE RUST)");
    println!(
        "========================================================================================================"
    );
    println!(
        "| {:>18} | {:>6} | {:>13} | {:>13} | {:>13} | {:>13} | {:>17} |",
        "Payload Size",
        "Runs",
        "p50 Latency",
        "p90 Latency",
        "p99 Latency",
        "Max Latency",
        "Throughput (MB/s)"
    );
    println!(
        "|--------------------+--------+---------------+---------------+---------------+---------------+-------------------|"
    );

    let sizes = [5, 10, 20, 35, 50];
    let mut results = Vec::new();

    for &size_mb in &sizes {
        let iterations = if size_mb >= 35 { 5 } else { 10 };
        let metric = run_size_benchmark(&mut conn, size_mb, iterations).await?;
        metric.print();
        results.push(metric);
    }

    println!(
        "========================================================================================================\n"
    );

    let json = serde_json::to_string_pretty(&results)?;
    std::fs::write("benchmarks/native_jumbo_results.json", json)?;

    conn.close().await?;
    Ok(())
}
