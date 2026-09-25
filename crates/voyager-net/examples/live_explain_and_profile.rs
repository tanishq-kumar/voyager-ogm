//! Live Engine Example: Query Profiling and Plan Explanation (`explain` & `profile`)
//!
//! Connects to live Neo4j and Memgraph instances, executes `EXPLAIN` and `PROFILE` queries,
//! and prints plan statistics and returned Arrow record batches.
//!
//! Run with: `cargo run -p voyager-net --example live_explain_and_profile`

use std::collections::HashMap;

use voyager_core::builder::QueryBuilder;
use voyager_core::emitters::CypherEmitter;
use voyager_core::visitor::AstVisitor;
use voyager_net::bolt::BoltConnection;
use voyager_net::config::ConnectionConfig;
use voyager_net::engine::AsyncConnection;

const NEO4J_URI: &str = "bolt://127.0.0.1:7687";
const NEO4J_USER: &str = "neo4j";
const NEO4J_PASS: &str = "voyagerpass123";

const MEMGRAPH_URI: &str = "bolt://127.0.0.1:7688";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Voyager OGM (Rust) - Live Engine Explain & Profile Demo ===\n");

    // -----------------------------------------------------------------------
    // 1. Live Neo4j Connection & Execution
    // -----------------------------------------------------------------------
    println!("--- 1. Live Neo4j (Port 7687) ---");
    let neo4j_cfg = ConnectionConfig::from_uri(NEO4J_URI).with_auth(NEO4J_USER, NEO4J_PASS);

    match BoltConnection::connect(&neo4j_cfg).await {
        Ok(mut conn) => {
            if let Some(agent) = conn.server_agent() {
                println!("Connected to Neo4j agent: {}", agent);
            }

            let mut emitter = CypherEmitter::new();

            // 1. Clean up test graph using QueryBuilder
            let mut b_clean = QueryBuilder::new();
            b_clean
                .r#match()
                .node(Some("n"), vec!["DemoLiveUser"])
                .detach_delete(vec!["n"]);
            let (arena_clean, root_clean) = b_clean.build();
            let compiled_clean = emitter.visit_query(&arena_clean, root_clean)?;
            let _ = conn
                .execute(&compiled_clean.statement, &HashMap::new())
                .await?;

            // 2. Seed test graph using QueryBuilder fluent mutations
            let mut b_seed = QueryBuilder::new();
            b_seed
                .create()
                .node(Some("u1"), vec!["DemoLiveUser"])
                .to(vec!["FRIENDS_WITH"], Some("r"))
                .node(Some("u2"), vec!["DemoLiveUser"])
                .set_property("u1", "name", "Alice")
                .set_property("u1", "age", 30i64)
                .set_property("u2", "name", "Bob")
                .set_property("u2", "age", 25i64)
                .set_property("r", "since", 2024i64);
            let (arena_seed, root_seed) = b_seed.build();
            let compiled_seed = emitter.visit_query(&arena_seed, root_seed)?;

            let mut seed_params = HashMap::new();
            for (k, v) in &compiled_seed.parameters {
                if let voyager_core::ast::LiteralValue::Int64(i) = v {
                    seed_params.insert(k.clone(), serde_json::json!(i));
                } else if let voyager_core::ast::LiteralValue::String(s) = v {
                    seed_params.insert(k.clone(), serde_json::json!(s));
                }
            }
            conn.execute(&compiled_seed.statement, &seed_params).await?;

            // 3. Build explain query with QueryBuilder
            let mut b_exp = QueryBuilder::new();
            b_exp
                .r#match()
                .node(Some("u"), vec!["DemoLiveUser"])
                .where_gte("u", "age", 20)
                .r#return()
                .field("u", "name", None::<&str>)
                .field("u", "age", None::<&str>)
                .order_by_asc("u", "age")
                .explain();

            let (arena_exp, root_exp) = b_exp.build();
            let compiled_exp = emitter.visit_query(&arena_exp, root_exp)?;

            let mut exp_params = HashMap::new();
            for (k, v) in &compiled_exp.parameters {
                if let voyager_core::ast::LiteralValue::Int64(i) = v {
                    exp_params.insert(k.clone(), serde_json::json!(i));
                }
            }

            println!("Executing on Neo4j: {}", compiled_exp.statement);
            let res_exp = conn.execute(&compiled_exp.statement, &exp_params).await?;
            println!(
                "-> EXPLAIN completed! Result rows: {} (EXPLAIN plans without executing records)",
                res_exp.row_count()
            );

            // 4. Build profile query with QueryBuilder
            let mut b_prof = QueryBuilder::new();
            b_prof
                .r#match()
                .node(Some("u"), vec!["DemoLiveUser"])
                .where_gte("u", "age", 20)
                .r#return()
                .field("u", "name", None::<&str>)
                .field("u", "age", None::<&str>)
                .order_by_asc("u", "age")
                .profile();

            let (arena_prof, root_prof) = b_prof.build();
            let compiled_prof = emitter.visit_query(&arena_prof, root_prof)?;

            let mut prof_params = HashMap::new();
            for (k, v) in &compiled_prof.parameters {
                if let voyager_core::ast::LiteralValue::Int64(i) = v {
                    prof_params.insert(k.clone(), serde_json::json!(i));
                }
            }

            println!("\nExecuting on Neo4j: {}", compiled_prof.statement);
            let res_prof = conn.execute(&compiled_prof.statement, &prof_params).await?;
            println!(
                "-> PROFILE completed! Result rows: {} (PROFILE executes query and captures runtime metrics)",
                res_prof.row_count()
            );

            // 5. Clean up using QueryBuilder
            conn.execute(&compiled_clean.statement, &HashMap::new())
                .await?;
            conn.close().await?;
        }
        Err(e) => {
            println!(
                "[SKIP] Neo4j instance not reachable on {}: {}",
                NEO4J_URI, e
            );
        }
    }

    // -----------------------------------------------------------------------
    // 2. Live Memgraph Connection & Execution
    // -----------------------------------------------------------------------
    println!("\n--- 2. Live Memgraph (Port 7688) ---");
    let memgraph_cfg = ConnectionConfig::from_uri(MEMGRAPH_URI);

    match BoltConnection::connect(&memgraph_cfg).await {
        Ok(mut conn) => {
            if let Some(agent) = conn.server_agent() {
                println!("Connected to Memgraph agent: {}", agent);
            }

            let mut emitter = CypherEmitter::new();

            // 1. Clean up test graph using QueryBuilder
            let mut b_clean = QueryBuilder::new();
            b_clean
                .r#match()
                .node(Some("n"), vec!["DemoMemgraphUser"])
                .detach_delete(vec!["n"]);
            let (arena_clean, root_clean) = b_clean.build();
            let compiled_clean = emitter.visit_query(&arena_clean, root_clean)?;
            let _ = conn
                .execute(&compiled_clean.statement, &HashMap::new())
                .await?;

            // 2. Seed test graph using QueryBuilder
            let mut b_seed = QueryBuilder::new();
            b_seed
                .create()
                .node(Some("u"), vec!["DemoMemgraphUser"])
                .set_property("u", "name", "Charlie")
                .set_property("u", "age", 35i64);
            let (arena_seed, root_seed) = b_seed.build();
            let compiled_seed = emitter.visit_query(&arena_seed, root_seed)?;
            let mut seed_params = HashMap::new();
            for (k, v) in &compiled_seed.parameters {
                if let voyager_core::ast::LiteralValue::Int64(i) = v {
                    seed_params.insert(k.clone(), serde_json::json!(i));
                } else if let voyager_core::ast::LiteralValue::String(s) = v {
                    seed_params.insert(k.clone(), serde_json::json!(s));
                }
            }
            conn.execute(&compiled_seed.statement, &seed_params).await?;

            // 3. Build explain query with QueryBuilder
            let mut b_exp = QueryBuilder::new();
            b_exp
                .r#match()
                .node(Some("u"), vec!["DemoMemgraphUser"])
                .r#return()
                .field("u", "name", None::<&str>)
                .explain();

            let (arena_exp, root_exp) = b_exp.build();
            let compiled_exp = emitter.visit_query(&arena_exp, root_exp)?;

            println!("Executing on Memgraph: {}", compiled_exp.statement);
            let res_exp = conn
                .execute(&compiled_exp.statement, &HashMap::new())
                .await?;
            println!(
                "-> Memgraph EXPLAIN returned {} plan rows across columns: {:?}",
                res_exp.row_count(),
                res_exp.columns
            );

            // 4. Build profile query with QueryBuilder
            let mut b_prof = QueryBuilder::new();
            b_prof
                .r#match()
                .node(Some("u"), vec!["DemoMemgraphUser"])
                .r#return()
                .field("u", "name", None::<&str>)
                .profile();

            let (arena_prof, root_prof) = b_prof.build();
            let compiled_prof = emitter.visit_query(&arena_prof, root_prof)?;

            println!("\nExecuting on Memgraph: {}", compiled_prof.statement);
            let res_prof = conn
                .execute(&compiled_prof.statement, &HashMap::new())
                .await?;
            println!(
                "-> Memgraph PROFILE returned {} profile rows across columns: {:?}",
                res_prof.row_count(),
                res_prof.columns
            );

            // 5. Clean up using QueryBuilder
            let _ = conn
                .execute(&compiled_clean.statement, &HashMap::new())
                .await?;
            conn.close().await?;
        }
        Err(e) => {
            println!(
                "[SKIP] Memgraph instance not reachable on {}: {}",
                MEMGRAPH_URI, e
            );
        }
    }

    println!("\nLive engine explain and profile execution completed successfully!");
    Ok(())
}
