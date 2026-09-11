//! Live benchmarking suite for voyager-net Native Bolt Engine against live Neo4j.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

use voyager_net::bolt::BoltConnection;
use voyager_net::config::{ConnectionConfig, PoolConfig};
use voyager_net::engine::{AsyncConnection, ConnectionFactory};
use voyager_net::pool::ConnectionPool;

const NEO4J_URI: &str = "bolt://127.0.0.1:7687";
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

#[derive(Debug, serde::Serialize)]
struct BenchmarkMetrics {
    name: String,
    iterations: usize,
    concurrency: usize,
    duration_ms: f64,
    qps: f64,
    p50_ms: f64,
    p90_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

impl BenchmarkMetrics {
    fn compute(
        name: &str,
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
            name: name.to_string(),
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

    fn print(&self) {
        println!(
            "| {:<45} | {:>6} | {:>4} | {:>9.2} | {:>7.2} ms | {:>7.2} ms | {:>7.2} ms | {:>7.2} ms |",
            self.name,
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

async fn setup_dataset(conn: &mut BoltConnection<TcpStream>) -> voyager_net::Result<()> {
    println!("[SEED] Initializing benchmark schema and dataset...");

    // 1. Clean previous benchmark data
    conn.execute("MATCH (n:BenchPerson) DETACH DELETE n", &HashMap::new())
        .await?;
    conn.execute("MATCH (n:BenchBulk) DETACH DELETE n", &HashMap::new())
        .await?;

    // 2. Create constraints/indexes
    let _ = conn
        .execute(
            "CREATE CONSTRAINT IF NOT EXISTS FOR (p:BenchPerson) REQUIRE p.id IS UNIQUE",
            &HashMap::new(),
        )
        .await;
    let _ = conn
        .execute(
            "CREATE INDEX IF NOT EXISTS FOR (p:BenchPerson) ON (p.city)",
            &HashMap::new(),
        )
        .await;

    // 3. Batch insert 10,000 nodes with UNWIND in batches of 2,500
    for chunk in 0..4 {
        let mut batch = Vec::new();
        let cities = [
            "San Francisco",
            "New York",
            "London",
            "Tokyo",
            "Berlin",
            "Singapore",
            "Sydney",
            "Toronto",
        ];
        for i in 0..2500 {
            let id = (chunk * 2500 + i) as i64;
            let mut item = serde_json::Map::new();
            item.insert("id".to_string(), serde_json::Value::Number(id.into()));
            item.insert(
                "name".to_string(),
                serde_json::Value::String(format!("User_{}", id)),
            );
            item.insert(
                "age".to_string(),
                serde_json::Value::Number(((id % 60) + 18).into()),
            );
            item.insert(
                "city".to_string(),
                serde_json::Value::String(cities[(id as usize) % cities.len()].to_string()),
            );
            item.insert(
                "score".to_string(),
                serde_json::Value::from(85.5 + (id % 15) as f64),
            );
            item.insert("active".to_string(), serde_json::Value::Bool(id % 2 == 0));
            batch.push(serde_json::Value::Object(item));
        }

        let mut params = HashMap::new();
        params.insert("batch".to_string(), serde_json::Value::Array(batch));

        let query = "
            UNWIND $batch AS row
            CREATE (p:BenchPerson {
                id: row.id,
                name: row.name,
                age: row.age,
                city: row.city,
                score: row.score,
                active: row.active
            })
        ";
        conn.execute(query, &params).await?;
    }

    // 4. Create 20,000 relationships (2 outgoing per node)
    for chunk in 0..4 {
        let mut rel_batch = Vec::new();
        for i in 0..2500 {
            let src = (chunk * 2500 + i) as i64;
            let dst1 = (src * 7 + 1) % 10000;
            let dst2 = (src * 13 + 3) % 10000;

            let mut r1 = serde_json::Map::new();
            r1.insert("src".to_string(), serde_json::Value::Number(src.into()));
            r1.insert("dst".to_string(), serde_json::Value::Number(dst1.into()));
            r1.insert(
                "since".to_string(),
                serde_json::Value::Number((2015 + (src % 9)).into()),
            );
            rel_batch.push(serde_json::Value::Object(r1));

            let mut r2 = serde_json::Map::new();
            r2.insert("src".to_string(), serde_json::Value::Number(src.into()));
            r2.insert("dst".to_string(), serde_json::Value::Number(dst2.into()));
            r2.insert(
                "since".to_string(),
                serde_json::Value::Number((2018 + (src % 6)).into()),
            );
            rel_batch.push(serde_json::Value::Object(r2));
        }

        let mut params = HashMap::new();
        params.insert("batch".to_string(), serde_json::Value::Array(rel_batch));

        let query = "
            UNWIND $batch AS row
            MATCH (a:BenchPerson {id: row.src})
            MATCH (b:BenchPerson {id: row.dst})
            CREATE (a)-[:KNOWS {since: row.since}]->(b)
        ";
        conn.execute(query, &params).await?;
    }

    println!("[SEED] Successfully seeded 10,000 nodes and 20,000 edges.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--list") {
        return Ok(());
    }
    let config = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);

    // Connect initial connection for seeding
    let mut init_conn = BoltConnection::connect(&config).await?;
    setup_dataset(&mut init_conn).await?;
    init_conn.close().await?;

    let factory = Arc::new(LiveBoltFactory {
        config: config.clone(),
    });
    let pool_config = PoolConfig::new().with_min_idle(5).with_max_size(20);
    let pool = ConnectionPool::new(pool_config, factory.clone());
    pool.warm_up().await?;

    println!(
        "\n================================================================================================================="
    );
    println!("                               VOYAGER-NET NATIVE BOLT ENGINE BENCHMARK SUITE");
    println!(
        "================================================================================================================="
    );
    println!(
        "| {:<45} | {:>6} | {:>4} | {:>9} | {:>10} | {:>10} | {:>10} | {:>10} |",
        "Scenario", "Runs", "Conc", "QPS", "p50", "p90", "p99", "Max"
    );
    println!(
        "|-----------------------------------------------+--------+------+-----------+------------+------------+------------+------------|"
    );

    let mut all_results = Vec::new();

    // -------------------------------------------------------------------------
    // Scenario 1: High-Concurrency Point Reads (Indexed Key-Value Lookup)
    // -------------------------------------------------------------------------
    {
        let iterations = 1000;
        let concurrency = 20;
        let query = "MATCH (p:BenchPerson {id: $id}) RETURN p.id AS id, p.name AS name, p.age AS age, p.city AS city, p.score AS score";

        let start = Instant::now();
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::new();

        for i in 0..iterations {
            let pool = pool.clone();
            let sem = sem.clone();
            let id = (i * 17) % 10000;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let q_start = Instant::now();
                let mut conn = pool.acquire().await.expect("Acquire failed");
                let mut params = HashMap::new();
                params.insert(
                    "id".to_string(),
                    serde_json::Value::Number((id as i64).into()),
                );
                let res = conn.execute(query, &params).await.expect("Query failed");
                assert_eq!(res.row_count(), 1);
                q_start.elapsed().as_secs_f64() * 1000.0
            }));
        }

        let mut latencies = Vec::new();
        for h in handles {
            latencies.push(h.await?);
        }
        let metrics = BenchmarkMetrics::compute(
            "1. Point Reads (1-Hop Lookup, Concurrency 20)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        metrics.print();
        all_results.push(metrics);
    }

    // -------------------------------------------------------------------------
    // Scenario 2: Graph 2-Hop Traversal & Aggregation
    // -------------------------------------------------------------------------
    {
        let iterations = 200;
        let concurrency = 10;
        let query = "
            MATCH (p:BenchPerson {id: $id})-[:KNOWS]->(f:BenchPerson)-[:KNOWS]->(fof:BenchPerson)
            RETURN fof.city AS city, count(fof) AS cnt, avg(fof.score) AS avg_score
            ORDER BY cnt DESC
            LIMIT 10
        ";

        let start = Instant::now();
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::new();

        for i in 0..iterations {
            let pool = pool.clone();
            let sem = sem.clone();
            let id = (i * 31) % 10000;

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let q_start = Instant::now();
                let mut conn = pool.acquire().await.expect("Acquire failed");
                let mut params = HashMap::new();
                params.insert(
                    "id".to_string(),
                    serde_json::Value::Number((id as i64).into()),
                );
                let res = conn.execute(query, &params).await.expect("Query failed");
                assert!(res.row_count() > 0);
                q_start.elapsed().as_secs_f64() * 1000.0
            }));
        }

        let mut latencies = Vec::new();
        for h in handles {
            latencies.push(h.await?);
        }
        let metrics = BenchmarkMetrics::compute(
            "2. Graph 2-Hop Traversal & Aggregation",
            latencies,
            start.elapsed(),
            concurrency,
        );
        metrics.print();
        all_results.push(metrics);
    }

    // -------------------------------------------------------------------------
    // Scenario 3: High-Volume Bulk Result Streaming (10,000 Records -> Arrow)
    // -------------------------------------------------------------------------
    {
        let iterations = 20;
        let concurrency = 1;
        let query = "
            MATCH (p:BenchPerson)
            RETURN p.id AS id, p.name AS name, p.age AS age, p.city AS city, p.score AS score, p.active AS active
            LIMIT 10000
        ";

        let start = Instant::now();
        let mut latencies = Vec::new();

        for _ in 0..iterations {
            let q_start = Instant::now();
            let mut conn = pool.acquire().await.expect("Acquire failed");
            let res = conn
                .execute(query, &HashMap::new())
                .await
                .expect("Query failed");
            assert_eq!(res.row_count(), 10000);
            let batch = res
                .into_single_batch()
                .unwrap()
                .expect("RecordBatch conversion failed");
            assert_eq!(batch.num_rows(), 10000);
            latencies.push(q_start.elapsed().as_secs_f64() * 1000.0);
        }

        let metrics = BenchmarkMetrics::compute(
            "3. Bulk Result Streaming (10k Rows -> Arrow)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        metrics.print();
        all_results.push(metrics);
    }

    // -------------------------------------------------------------------------
    // Scenario 4: Bulk Mutation Ingestion (UNWIND $batch 5,000 Nodes)
    // -------------------------------------------------------------------------
    {
        let iterations = 10;
        let concurrency = 1;
        let query = "
            UNWIND $batch AS row
            CREATE (p:BenchBulk {
                id: row.id,
                name: row.name,
                age: row.age,
                city: row.city,
                score: row.score,
                active: row.active
            })
        ";

        let start = Instant::now();
        let mut latencies = Vec::new();

        for b in 0..iterations {
            let mut batch = Vec::new();
            for i in 0..5000 {
                let id = (b * 5000 + i) as i64;
                let mut item = serde_json::Map::new();
                item.insert("id".to_string(), serde_json::Value::Number(id.into()));
                item.insert(
                    "name".to_string(),
                    serde_json::Value::String(format!("BulkUser_{}", id)),
                );
                item.insert("age".to_string(), serde_json::Value::Number(28.into()));
                item.insert(
                    "city".to_string(),
                    serde_json::Value::String("Tokyo".to_string()),
                );
                item.insert("score".to_string(), serde_json::Value::from(99.9));
                item.insert("active".to_string(), serde_json::Value::Bool(true));
                batch.push(serde_json::Value::Object(item));
            }

            let mut params = HashMap::new();
            params.insert("batch".to_string(), serde_json::Value::Array(batch));

            let q_start = Instant::now();
            let mut conn = pool.acquire().await.expect("Acquire failed");
            let res = conn
                .execute(query, &params)
                .await
                .expect("Bulk insert failed");
            assert_eq!(res.summary.nodes_created, 5000);
            latencies.push(q_start.elapsed().as_secs_f64() * 1000.0);
        }

        let metrics = BenchmarkMetrics::compute(
            "4. Bulk Ingestion (5,000 Nodes UNWIND $batch)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        metrics.print();
        all_results.push(metrics);
    }

    // -------------------------------------------------------------------------
    // Scenario 5: Connection Pool Contention (100 Concurrent Tasks on Pool Size 5)
    // -------------------------------------------------------------------------
    {
        let small_pool_config = PoolConfig::new().with_min_idle(2).with_max_size(5);
        let small_pool = ConnectionPool::new(small_pool_config, factory.clone());
        small_pool.warm_up().await?;

        let iterations = 1000;
        let concurrency = 100;
        let query = "RETURN $val AS res";

        let start = Instant::now();
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut handles = Vec::new();

        for i in 0..iterations {
            let pool = small_pool.clone();
            let sem = sem.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let q_start = Instant::now();
                let mut conn = pool.acquire().await.expect("Acquire failed");
                let mut params = HashMap::new();
                params.insert(
                    "val".to_string(),
                    serde_json::Value::Number((i as i64).into()),
                );
                let res = conn.execute(query, &params).await.expect("Query failed");
                assert_eq!(res.row_count(), 1);
                q_start.elapsed().as_secs_f64() * 1000.0
            }));
        }

        let mut latencies = Vec::new();
        for h in handles {
            latencies.push(h.await?);
        }
        let metrics = BenchmarkMetrics::compute(
            "5. Pool Contention (100 Tasks / 5 Conns)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        metrics.print();
        all_results.push(metrics);
    }

    // -------------------------------------------------------------------------
    // Scenario 6: Mixed Heterogeneous Column Types (5,000 Rows Variant Types)
    // -------------------------------------------------------------------------
    {
        let iterations = 10;
        let concurrency = 1;
        let query = "
            UNWIND range(1, 5000) AS i
            RETURN i AS id,
                   CASE WHEN i % 3 = 0 THEN i
                        WHEN i % 3 = 1 THEN 'str_val_' + toString(i)
                        ELSE toFloat(i) + 0.5 END AS mixed_val,
                   CASE WHEN i % 2 = 0 THEN true ELSE false END AS bool_val
        ";

        let start = Instant::now();
        let mut latencies = Vec::new();

        for _ in 0..iterations {
            let q_start = Instant::now();
            let mut conn = pool.acquire().await.expect("Acquire failed");
            let res = conn
                .execute(query, &HashMap::new())
                .await
                .expect("Query failed");
            assert_eq!(res.row_count(), 5000);
            let batch = res
                .into_single_batch()
                .unwrap()
                .expect("RecordBatch conversion failed");
            assert_eq!(batch.num_rows(), 5000);
            assert_eq!(
                batch.schema().field(1).data_type(),
                &arrow_schema::DataType::Utf8
            );
            latencies.push(q_start.elapsed().as_secs_f64() * 1000.0);
        }

        let metrics = BenchmarkMetrics::compute(
            "6. Mixed Types Streaming (5k Variant Rows)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        metrics.print();
        all_results.push(metrics);
    }

    // -------------------------------------------------------------------------
    // Scenario 7: Jumbo Chunk Payload (5MB Multi-Chunk Frame)
    // -------------------------------------------------------------------------
    {
        let iterations = 10;
        let concurrency = 1;
        let jumbo_str = "V".repeat(5 * 1024 * 1024); // 5 Megabytes across 80+ chunks
        let mut params = HashMap::new();
        params.insert("payload".to_string(), serde_json::Value::String(jumbo_str));
        let query = "RETURN $payload AS jumbo_str, size($payload) AS len";

        let start = Instant::now();
        let mut latencies = Vec::new();

        for _ in 0..iterations {
            let q_start = Instant::now();
            let mut conn = pool.acquire().await.expect("Acquire failed");
            let res = conn.execute(query, &params).await.expect("Query failed");
            assert_eq!(res.row_count(), 1);
            let batch = res
                .into_single_batch()
                .unwrap()
                .expect("RecordBatch conversion failed");
            assert_eq!(batch.num_rows(), 1);
            latencies.push(q_start.elapsed().as_secs_f64() * 1000.0);
        }

        let metrics = BenchmarkMetrics::compute(
            "7. Jumbo Frame Streaming (5MB Payload / TCP)",
            latencies,
            start.elapsed(),
            concurrency,
        );
        metrics.print();
        all_results.push(metrics);
    }

    println!(
        "=================================================================================================================\n"
    );

    // Write JSON output for comparison
    let json = serde_json::to_string_pretty(&all_results)?;
    std::fs::write("benchmarks/native_bolt_results.json", json)?;
    println!("[SAVED] Saved native benchmark metrics to benchmarks/native_bolt_results.json");

    Ok(())
}
