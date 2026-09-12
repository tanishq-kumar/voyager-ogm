//! Adversarial stress test probe uncovering failure modes and architectural limits in voyager-net.

use std::collections::HashMap;
use voyager_net::bolt::BoltConnection;
use voyager_net::config::ConnectionConfig;
use voyager_net::engine::AsyncConnection;

const NEO4J_URI: &str = "bolt://127.0.0.1:7687";
const NEO4J_USER: &str = "neo4j";
const NEO4J_PASS: &str = "voyagerpass123";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);
    let mut conn = BoltConnection::connect(&config).await?;

    println!(
        "\n=========================================================================================="
    );
    println!("                  ADVERSARIAL STRESS TEST & FAILURE MODE PROBE");
    println!(
        "=========================================================================================="
    );

    // -------------------------------------------------------------------------
    // Probe 1: Heterogeneous Column Types (Cypher Schema-less Type Drift)
    // -------------------------------------------------------------------------
    println!(
        "\n[PROBE 1] Testing Heterogeneous Column Types (Integer in row 0, String in row 1, Float in row 2):"
    );
    let query_hetero = "
        UNWIND [
            {id: 1, val: 100},
            {id: 2, val: 'string_value_instead_of_int'},
            {id: 3, val: 99.99}
        ] AS row
        RETURN row.id AS id, row.val AS val
    ";

    let res_hetero = conn.execute(query_hetero, &HashMap::new()).await?;
    let batch_hetero = res_hetero.into_single_batch()?.unwrap();
    println!("  * Schema generated: {:?}", batch_hetero.schema());
    println!("  * Row count: {}", batch_hetero.num_rows());

    // Check column 1 (val)
    let val_col = batch_hetero.column(1);
    println!("  * Value Column Data Array: {:?}", val_col);
    let str_arr = val_col
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .unwrap();
    assert_eq!(str_arr.value(0), "100");
    assert_eq!(str_arr.value(1), "string_value_instead_of_int");
    assert_eq!(str_arr.value(2), "99.99");
    println!(
        "  * Verified: Column safely promoted to Utf8 with ZERO DATA LOSS (all 3 values preserved)!"
    );

    // -------------------------------------------------------------------------
    // Probe 2: Jumbo Chunk Fragmentation (10 Megabyte String Property)
    // -------------------------------------------------------------------------
    println!("\n[PROBE 2] Testing Jumbo Chunk Fragmentation (10 MB Single String Property):");
    let large_string = "V".repeat(10 * 1024 * 1024); // 10 Megabytes
    let mut params = HashMap::new();
    params.insert(
        "payload".to_string(),
        serde_json::Value::String(large_string.clone()),
    );

    let t0 = std::time::Instant::now();
    let query_jumbo = "RETURN $payload AS jumbo_str, size($payload) AS char_len";
    let res_jumbo = conn.execute(query_jumbo, &params).await?;
    let elapsed = t0.elapsed();
    let batch_jumbo = res_jumbo.into_single_batch()?.unwrap();
    println!(
        "  * Transmitted and received 10MB chunked frame across TCP in {:.2?}",
        elapsed
    );
    println!(
        "  * Rows returned: {}, Column count: {}",
        batch_jumbo.num_rows(),
        batch_jumbo.num_columns()
    );

    // -------------------------------------------------------------------------
    // Probe 3: Syntax Error & Intermediate Recovery State
    // -------------------------------------------------------------------------
    println!("\n[PROBE 3] Testing Bad Cypher Syntax Recovery & Pipeline Desynchronization:");
    let bad_query = "THIS IS NOT VALID CYPHER SYNTAX !!!";
    match conn.execute(bad_query, &HashMap::new()).await {
        Ok(_) => println!("  * Unexpected success!"),
        Err(e) => println!("  * Handled cleanly via Bolt RESET: {:?}", e),
    }

    // Check if subsequent valid query works on the same socket
    let good_query = "RETURN 1 + 1 AS two";
    let res_good = conn.execute(good_query, &HashMap::new()).await?;
    println!(
        "  * Socket state after syntax error recovery: Valid (Result: {:?})",
        res_good.row_count()
    );

    conn.close().await?;
    println!(
        "\n==========================================================================================\n"
    );

    Ok(())
}
