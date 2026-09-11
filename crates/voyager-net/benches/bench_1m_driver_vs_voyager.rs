//! 1 Million Nodes & 2 Million Edges Head-to-Head Comparative Benchmark:
//! voyager-net (Zero-Copy Wire-to-Arrow) vs. neo4rs (Standard Rust Driver).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

use voyager_net::bolt::BoltConnection;
use voyager_net::config::{ConnectionConfig, PoolConfig};
use voyager_net::engine::{AsyncConnection, ConnectionFactory};
use voyager_net::pool::ConnectionPool;

use neo4rs::{ConfigBuilder, Graph, query};

const NEO4J_URI: &str = "bolt://127.0.0.1:7687";
const NEO4J_HOST_PORT: &str = "127.0.0.1:7687";
const NEO4J_USER: &str = "neo4j";
const NEO4J_PASS: &str = "voyagerpass123";

struct LiveBoltFactory {
    config: ConnectionConfig,
}

#[async_trait::async_trait]
impl ConnectionFactory<BoltConnection<TcpStream>> for LiveBoltFactory {
    async fn create(&self) -> voyager_net::Result<BoltConnection<TcpStream>> {
        BoltConnection::connect(&self.config).await
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
    sorted[idx]
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BenchmarkMetrics {
    pub engine: String,
    pub scenario: String,
    pub iterations: usize,
    pub concurrency: usize,
    pub duration_ms: f64,
    pub qps: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

impl BenchmarkMetrics {
    pub fn compute(
        engine: &str,
        scenario: &str,
        mut latencies: Vec<f64>,
        total_duration: Duration,
        concurrency: usize,
    ) -> Self {
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let iterations = latencies.len();
        let duration_ms = total_duration.as_secs_f64() * 1000.0;
        let qps = if duration_ms > 0.0 {
            (iterations as f64) / (total_duration.as_secs_f64())
        } else {
            0.0
        };

        Self {
            engine: engine.to_string(),
            scenario: scenario.to_string(),
            iterations,
            concurrency,
            duration_ms,
            qps,
            p50_ms: percentile(&latencies, 50.0),
            p90_ms: percentile(&latencies, 90.0),
            p95_ms: percentile(&latencies, 95.0),
            p99_ms: percentile(&latencies, 99.0),
            max_ms: *latencies.last().unwrap_or(&0.0),
        }
    }

    pub fn print_row(&self) {
        println!(
            "| {:<13} | {:<42} | {:>6} | {:>4} | {:>9.2} | {:>7.2} ms | {:>7.2} ms | {:>7.2} ms | {:>7.2} ms |",
            self.engine,
            self.scenario,
            self.iterations,
            self.concurrency,
            self.qps,
            self.p50_ms,
            self.p90_ms,
            self.p99_ms,
            self.max_ms
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--list") {
        return Ok(());
    }
    println!(
        "=========================================================================================================================="
    );
    println!("     SCALE BENCHMARK: 1 MILLION NODES & 2 MILLION EDGES (voyager-net vs neo4rs)");
    println!(
        "=========================================================================================================================="
    );

    // 1. Setup voyager-net pool (pre-warmed to 20 connections)
    let voyager_config = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);
    let voyager_factory = Arc::new(LiveBoltFactory {
        config: voyager_config.clone(),
    });
    let pool_config = PoolConfig::new().with_min_idle(20).with_max_size(20);
    let voyager_pool = ConnectionPool::new(pool_config, voyager_factory);
    voyager_pool.warm_up().await?;

    // Verify dataset scale
    {
        let mut conn = voyager_pool.acquire().await?;
        let res = conn
            .execute(
                "MATCH (n:BenchPerson) RETURN count(n) AS node_count",
                &HashMap::new(),
            )
            .await?;
        let batch = res.into_single_batch()?.expect("node count batch");
        let int_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Int64Array>()
            .expect("int array");
        let count = int_col.value(0);
        println!("[SCALE VERIFIED] Total Nodes in live database: {:}", count);
        assert!(
            count >= 1_000_000,
            "Database must contain at least 1,000,000 nodes. Found: {}",
            count
        );
    }

    // 2. Setup neo4rs client (max 20 connections)
    let neo4rs_config = ConfigBuilder::default()
        .uri(NEO4J_HOST_PORT)
        .user(NEO4J_USER)
        .password(NEO4J_PASS)
        .db("neo4j")
        .max_connections(20)
        .build()?;
    let neo4rs_graph = Arc::new(Graph::connect(neo4rs_config).await?);

    println!(
        "\n=========================================================================================================================="
    );
    println!("                   1 MILLION ENTITY HEAD-TO-HEAD BENCHMARK: voyager-net vs neo4rs");
    println!(
        "=========================================================================================================================="
    );
    println!(
        "| {:<13} | {:<42} | {:>6} | {:>4} | {:>9} | {:>10} | {:>10} | {:>10} | {:>10} |",
        "Driver", "Scenario", "Runs", "Conc", "QPS", "p50", "p90", "p99", "Max"
    );
    println!(
        "|---------------+--------------------------------------------+--------+------+-----------+------------+------------+------------+------------|"
    );

    let mut all_metrics = Vec::new();

    // -------------------------------------------------------------------------
    // Scenario 1: Large-Scale Bulk Streaming (50,000 Rows with 6 Columns)
    // -------------------------------------------------------------------------
    let bulk_query = "MATCH (p:BenchPerson) RETURN p.id AS id, p.name AS name, p.age AS age, p.city AS city, p.score AS score, p.active AS active LIMIT 50000";

    // 1a. voyager-net (Bulk 50k rows -> Arrow RecordBatch)
    {
        let iterations = 10;
        let mut latencies = Vec::new();
        let start = Instant::now();

        for _ in 0..iterations {
            let q_start = Instant::now();
            let mut conn = voyager_pool.acquire().await?;
            let res = conn.execute(bulk_query, &HashMap::new()).await?;
            let batch = res.into_single_batch()?.expect("RecordBatch");
            assert_eq!(batch.num_rows(), 50000);
            latencies.push(q_start.elapsed().as_secs_f64() * 1000.0);
        }

        let m = BenchmarkMetrics::compute(
            "voyager-net",
            "1. Bulk 50k Rows (Direct Wire->Arrow)",
            latencies,
            start.elapsed(),
            1,
        );
        m.print_row();
        all_metrics.push(m);
    }

    // 1b. neo4rs (Bulk 50k rows -> Row-by-Row Unpack)
    {
        let iterations = 10;
        let mut latencies = Vec::new();
        let start = Instant::now();

        for _ in 0..iterations {
            let q_start = Instant::now();
            let mut stream = neo4rs_graph.execute(query(bulk_query)).await?;
            let mut count = 0;
            while let Ok(Some(row)) = stream.next().await {
                let _id: i64 = row.get("id")?;
                let _name: String = row.get("name")?;
                let _age: i64 = row.get("age")?;
                let _city: String = row.get("city")?;
                let _score: f64 = row.get("score")?;
                let _active: bool = row.get("active")?;
                count += 1;
            }
            assert_eq!(count, 50000);
            latencies.push(q_start.elapsed().as_secs_f64() * 1000.0);
        }

        let m = BenchmarkMetrics::compute(
            "neo4rs",
            "1. Bulk 50k Rows (Row-by-Row Unpack)",
            latencies,
            start.elapsed(),
            1,
        );
        m.print_row();
        all_metrics.push(m);
    }

    println!(
        "|---------------+--------------------------------------------+--------+------+-----------+------------+------------+------------+------------|"
    );

    // -------------------------------------------------------------------------
    // Scenario 2: High Concurrency Point Reads across 1,000,000 Node Space
    // -------------------------------------------------------------------------
    let point_query_tmpl = "MATCH (p:BenchPerson {id: $id}) RETURN p.id AS id, p.name AS name, p.age AS age, p.score AS score";

    // 2a. voyager-net Point Reads
    {
        let iterations = 1000;
        let concurrency = 20;
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::with_capacity(iterations);
        let start = Instant::now();

        for i in 0..iterations {
            let pool = voyager_pool.clone();
            let permit = sem.clone().acquire_owned().await.unwrap();
            // Pseudo-random index spread across 1,000,000 nodes
            let id = ((i as i64 * 7919 + 31) % 1_000_000).abs();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let q_start = Instant::now();
                let mut conn = pool.acquire().await.unwrap();
                let mut params = HashMap::new();
                params.insert("id".to_string(), serde_json::Value::Number(id.into()));
                let res = conn.execute(point_query_tmpl, &params).await.unwrap();
                assert_eq!(res.row_count(), 1);
                q_start.elapsed().as_secs_f64() * 1000.0
            }));
        }

        let mut latencies = Vec::new();
        for h in handles {
            latencies.push(h.await?);
        }

        let m = BenchmarkMetrics::compute(
            "voyager-net",
            "2. Point Reads (1M space, Conc 20, 1k)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        m.print_row();
        all_metrics.push(m);
    }

    // 2b. neo4rs Point Reads
    {
        let iterations = 1000;
        let concurrency = 20;
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::with_capacity(iterations);
        let start = Instant::now();

        for i in 0..iterations {
            let g = neo4rs_graph.clone();
            let permit = sem.clone().acquire_owned().await.unwrap();
            let id = ((i as i64 * 7919 + 31) % 1_000_000).abs();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let q_start = Instant::now();
                let mut stream = g
                    .execute(query(point_query_tmpl).param("id", id))
                    .await
                    .unwrap();
                let mut found = false;
                if let Ok(Some(row)) = stream.next().await {
                    let _id: i64 = row.get("id").unwrap();
                    let _name: String = row.get("name").unwrap();
                    let _age: i64 = row.get("age").unwrap();
                    let _score: f64 = row.get("score").unwrap();
                    found = true;
                }
                assert!(found);
                q_start.elapsed().as_secs_f64() * 1000.0
            }));
        }

        let mut latencies = Vec::new();
        for h in handles {
            latencies.push(h.await?);
        }

        let m = BenchmarkMetrics::compute(
            "neo4rs",
            "2. Point Reads (1M space, Conc 20, 1k)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        m.print_row();
        all_metrics.push(m);
    }

    println!(
        "|---------------+--------------------------------------------+--------+------+-----------+------------+------------+------------+------------|"
    );

    // -------------------------------------------------------------------------
    // Scenario 3: Graph 2-Hop Traversal on 2,000,000 Edge Dense Graph
    // -------------------------------------------------------------------------
    let traversal_query = "
        MATCH (a:BenchPerson {id: $id})-[:KNOWS]->(b:BenchPerson)-[:KNOWS]->(c:BenchPerson)
        RETURN a.id AS a_id, b.id AS b_id, c.id AS c_id, c.name AS c_name, c.city AS c_city
        LIMIT 50
    ";

    // 3a. voyager-net 2-Hop Traversal
    {
        let iterations = 200;
        let concurrency = 10;
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::with_capacity(iterations);
        let start = Instant::now();

        for i in 0..iterations {
            let pool = voyager_pool.clone();
            let permit = sem.clone().acquire_owned().await.unwrap();
            let id = ((i as i64 * 4999 + 17) % 1_000_000).abs();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let q_start = Instant::now();
                let mut conn = pool.acquire().await.unwrap();
                let mut params = HashMap::new();
                params.insert("id".to_string(), serde_json::Value::Number(id.into()));
                let res = conn.execute(traversal_query, &params).await.unwrap();
                let _count = res.row_count();
                q_start.elapsed().as_secs_f64() * 1000.0
            }));
        }

        let mut latencies = Vec::new();
        for h in handles {
            latencies.push(h.await?);
        }

        let m = BenchmarkMetrics::compute(
            "voyager-net",
            "3. 2-Hop Traversal (2M Edges, Conc 10)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        m.print_row();
        all_metrics.push(m);
    }

    // 3b. neo4rs 2-Hop Traversal
    {
        let iterations = 200;
        let concurrency = 10;
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::with_capacity(iterations);
        let start = Instant::now();

        for i in 0..iterations {
            let g = neo4rs_graph.clone();
            let permit = sem.clone().acquire_owned().await.unwrap();
            let id = ((i as i64 * 4999 + 17) % 1_000_000).abs();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let q_start = Instant::now();
                let mut stream = g
                    .execute(query(traversal_query).param("id", id))
                    .await
                    .unwrap();
                let mut count = 0;
                while let Ok(Some(row)) = stream.next().await {
                    let _a_id: i64 = row.get("a_id").unwrap();
                    let _b_id: i64 = row.get("b_id").unwrap();
                    let _c_id: i64 = row.get("c_id").unwrap();
                    let _c_name: String = row.get("c_name").unwrap();
                    let _c_city: String = row.get("c_city").unwrap();
                    count += 1;
                }
                assert!(count <= 50);
                q_start.elapsed().as_secs_f64() * 1000.0
            }));
        }

        let mut latencies = Vec::new();
        for h in handles {
            latencies.push(h.await?);
        }

        let m = BenchmarkMetrics::compute(
            "neo4rs",
            "3. 2-Hop Traversal (2M Edges, Conc 10)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        m.print_row();
        all_metrics.push(m);
    }

    println!(
        "|---------------+--------------------------------------------+--------+------+-----------+------------+------------+------------+------------|"
    );

    // -------------------------------------------------------------------------
    // Scenario 4: Analytical Aggregation across 1,000,000 Nodes
    // -------------------------------------------------------------------------
    let agg_query = "
        MATCH (p:BenchPerson)
        RETURN p.city AS city, count(p) AS cnt, avg(p.score) AS avg_score, max(p.age) AS max_age
        ORDER BY cnt DESC
    ";

    // 4a. voyager-net Aggregation
    {
        let iterations = 10;
        let mut latencies = Vec::new();
        let start = Instant::now();

        for _ in 0..iterations {
            let q_start = Instant::now();
            let mut conn = voyager_pool.acquire().await?;
            let res = conn.execute(agg_query, &HashMap::new()).await?;
            let batch = res.into_single_batch()?.expect("RecordBatch");
            assert_eq!(batch.num_rows(), 8);
            latencies.push(q_start.elapsed().as_secs_f64() * 1000.0);
        }

        let m = BenchmarkMetrics::compute(
            "voyager-net",
            "4. Analytical Aggregation (1M Nodes Scan)",
            latencies,
            start.elapsed(),
            1,
        );
        m.print_row();
        all_metrics.push(m);
    }

    // 4b. neo4rs Aggregation
    {
        let iterations = 10;
        let mut latencies = Vec::new();
        let start = Instant::now();

        for _ in 0..iterations {
            let q_start = Instant::now();
            let mut stream = neo4rs_graph.execute(query(agg_query)).await?;
            let mut count = 0;
            while let Ok(Some(row)) = stream.next().await {
                let _city: String = row.get("city")?;
                let _cnt: i64 = row.get("cnt")?;
                let _avg_score: f64 = row.get("avg_score")?;
                let _max_age: i64 = row.get("max_age")?;
                count += 1;
            }
            assert_eq!(count, 8);
            latencies.push(q_start.elapsed().as_secs_f64() * 1000.0);
        }

        let m = BenchmarkMetrics::compute(
            "neo4rs",
            "4. Analytical Aggregation (1M Nodes Scan)",
            latencies,
            start.elapsed(),
            1,
        );
        m.print_row();
        all_metrics.push(m);
    }

    println!(
        "==========================================================================================================================\n"
    );

    // Save output to JSON
    let json = serde_json::to_string_pretty(&all_metrics)?;
    std::fs::write("benchmarks/driver_vs_voyager_net_1m_results.json", json)?;
    println!(
        "[SAVED] 1M Entity comparative results written to benchmarks/driver_vs_voyager_net_1m_results.json"
    );

    Ok(())
}
