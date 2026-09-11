use bytes::BytesMut;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use voyager_net::bolt::packstream::{BoltValue, PackStream};
use voyager_net::config::PoolConfig;
use voyager_net::mock::MockConnectionFactory;
use voyager_net::pool::ConnectionPool;
use voyager_net::postgres::connection::format_postgres_query;
use voyager_net::redis::resp::RespValue;
use voyager_net::redis::transaction::format_cypher_with_params;

#[tokio::main]
async fn main() {
    println!("=========================================================================");
    println!("     VOYAGER-NET: SECURITY HARDENING PERFORMANCE OVERHEAD BENCHMARK      ");
    println!("=========================================================================");

    // 1. PackStream List Deserialization Benchmark (100,000 elements)
    {
        let items: Vec<BoltValue> = (0..100_000).map(|i| BoltValue::Integer(i as i64)).collect();
        let list_val = BoltValue::List(items);
        let mut buf = BytesMut::new();
        PackStream::encode(&list_val, &mut buf);
        let encoded_bytes = buf.freeze();

        // Warmup
        let mut test_bytes = encoded_bytes.clone();
        let _ = PackStream::decode(&mut test_bytes).unwrap();

        let iters = 50;
        let start = Instant::now();
        for _ in 0..iters {
            let mut test_bytes = encoded_bytes.clone();
            let _ = PackStream::decode(&mut test_bytes).unwrap();
        }
        let elapsed = start.elapsed();
        let per_op_ms = elapsed.as_secs_f64() * 1000.0 / iters as f64;
        let throughput = (100_000.0 * iters as f64) / elapsed.as_secs_f64();
        println!(
            "PackStream List Decode (100K items, guarded capacity & bounds): {:>6.2} ms/op | {:>10.0} items/sec",
            per_op_ms, throughput
        );
    }

    // 2. RESP Array Parsing Benchmark (100,000 elements)
    {
        let mut resp_buf = BytesMut::new();
        resp_buf.extend_from_slice(b"*100000\r\n");
        for i in 0..100_000 {
            resp_buf.extend_from_slice(format!(":{}\r\n", i).as_bytes());
        }

        // Warmup
        let mut test_buf = resp_buf.clone();
        let _ = RespValue::parse(&mut test_buf).unwrap();

        let iters = 50;
        let start = Instant::now();
        for _ in 0..iters {
            let mut test_buf = resp_buf.clone();
            let _ = RespValue::parse(&mut test_buf).unwrap();
        }
        let elapsed = start.elapsed();
        let per_op_ms = elapsed.as_secs_f64() * 1000.0 / iters as f64;
        let throughput = (100_000.0 * iters as f64) / elapsed.as_secs_f64();
        println!(
            "RESP Array Parsing (100K items, capped capacity & length check):  {:>6.2} ms/op | {:>10.0} items/sec",
            per_op_ms, throughput
        );
    }

    // 3. PostgreSQL Parameter Sorting & Escaping Benchmark (100,000 queries)
    {
        let mut params = HashMap::new();
        params.insert("p1".to_string(), serde_json::json!(42));
        params.insert("p2".to_string(), serde_json::json!("O'Reilly"));
        params.insert("p10".to_string(), serde_json::json!(100));
        params.insert("p11".to_string(), serde_json::json!(true));
        params.insert("p12".to_string(), serde_json::json!("escaped'value'here"));

        let query =
            "SELECT * FROM t WHERE a = $p10 AND b = $p1 AND c = $p2 AND d = $p11 AND e = $p12;";

        let iters = 100_000;
        let start = Instant::now();
        for _ in 0..iters {
            let _ = format_postgres_query(query, &params);
        }
        let elapsed = start.elapsed();
        let per_op_us = elapsed.as_secs_f64() * 1_000_000.0 / iters as f64;
        let qps = iters as f64 / elapsed.as_secs_f64();
        println!(
            "PostgreSQL Param Formatting (Sorted Keys + Escaping):            {:>6.2} µs/op | {:>10.0} qps",
            per_op_us, qps
        );
    }

    // 4. FalkorDB Double Escaping Benchmark (100,000 queries)
    {
        let mut params = HashMap::new();
        params.insert("p1".to_string(), serde_json::json!(42));
        params.insert(
            "name".to_string(),
            serde_json::json!(r#"Alice \"The Great\""#),
        );
        params.insert(
            "path".to_string(),
            serde_json::json!(r#"C:\Users\supri\data"#),
        );
        let query = "MATCH (n:Person {name: $name, path: $path}) RETURN n;";

        let iters = 100_000;
        let start = Instant::now();
        for _ in 0..iters {
            let _ = format_cypher_with_params(query, &params);
        }
        let elapsed = start.elapsed();
        let per_op_us = elapsed.as_secs_f64() * 1_000_000.0 / iters as f64;
        let qps = iters as f64 / elapsed.as_secs_f64();
        println!(
            "FalkorDB Param Formatting (Backslash + Quote Escaping):          {:>6.2} µs/op | {:>10.0} qps",
            per_op_us, qps
        );
    }

    // 5. ConnectionPool Acquire / Release Lifecycle with Reset Check (100,000 cycles)
    {
        let factory = Arc::new(MockConnectionFactory::new());
        let config = PoolConfig::new().with_min_idle(5).with_max_size(10);
        let pool = ConnectionPool::new(config, factory);
        pool.warm_up().await.expect("Failed to warm up");

        let iters = 100_000;
        let start = Instant::now();
        for _ in 0..iters {
            let conn = pool.acquire().await.unwrap();
            drop(conn);
        }
        let elapsed = start.elapsed();
        let per_op_us = elapsed.as_secs_f64() * 1_000_000.0 / iters as f64;
        let qps = iters as f64 / elapsed.as_secs_f64();
        println!(
            "ConnectionPool Check-out & Verified Reset (100K cycles):         {:>6.2} µs/op | {:>10.0} checkouts/sec",
            per_op_us, qps
        );
    }

    println!("=========================================================================");
}
