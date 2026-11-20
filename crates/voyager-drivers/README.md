# voyager-drivers

**Native asynchronous driver layer for Voyager OGM: connection pooling, wire protocols, and Arrow results.**

`voyager-drivers` is the pure safe Rust driver layer for Voyager OGM. It opens asynchronous Bolt (Neo4j, Memgraph), PostgreSQL wire (Apache AGE, PostgreSQL 19) and Redis RESP (FalkorDB) connections over raw TCP, pools them, runs native transactions, and returns Apache Arrow record batches. It builds on [Tokio](https://tokio.rs) and contains no unsafe code.

## Install

```bash
# Default: bolt, postgres, redis, arrow
cargo add voyager-drivers

# Only the Bolt protocol
cargo add voyager-drivers --no-default-features --features bolt
```

| Feature | Default | Enables |
| :--- | :--- | :--- |
| `bolt` | yes | Bolt client (Neo4j, Memgraph) |
| `postgres` | yes | PostgreSQL wire client (Apache AGE, PostgreSQL 19) |
| `redis` | yes | Redis RESP2/RESP3 client (FalkorDB) |
| `arrow` | yes | Arrow result batches (`rows_to_record_batch`) |
| `testkit` | no | Mock engine and `BoltStubServer` for tests |

A URI or client that targets a protocol whose feature is disabled returns a `NetError` naming the missing feature.

## Example

The snippet below is a `no_run` doctest: it compiles but needs a live database to execute.

```rust,no_run
# use std::collections::HashMap;
# use voyager_drivers::NativeClient;
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let client = NativeClient::new("bolt://127.0.0.1:7687", None)?;
client.ping().await?;

let mut params = HashMap::new();
params.insert("name".to_string(), serde_json::json!("Alice"));
let result = client.execute(
    "MATCH (p:Person {name: $name}) RETURN p.name AS name",
    &params,
).await?;

println!("{} column(s), {} record batch(es)", result.columns.len(), result.batches.len());
client.close().await;
# Ok(())
# }
```

For a single connection with credentials, use `ConnectionConfig::from_uri(uri).with_auth(user, pass)` and `BoltConnection::connect(&config).await`. A runnable version is in [`crates/voyager-drivers/examples/live_connect_and_query.rs`](https://github.com/tanishq-kumar/voyager/blob/main/crates/voyager-drivers/examples/live_connect_and_query.rs).

## Protocols and URI schemes

| Protocol | URI schemes | Default port | Versions |
| :--- | :--- | :--- | :--- |
| Bolt | `bolt://`, `neo4j://`, `memgraph://` | 7687 | Bolt v4.2, v4.4, v5.0, v5.4 (proposed in the handshake) |
| PostgreSQL wire | `postgresql://`, `postgres://`, `age://` | 5432 | Frontend/Backend v3.0 |
| Redis RESP | `redis://`, `falkordb://`, `valkey://` | 6379 | RESP2 / RESP3 |

TLS variants (`bolt+s://`, `neo4j+ssc://`, `postgresql+s://`, `rediss://`, …) are parsed, but native TLS negotiation is not implemented yet: `connect_any` rejects any non-plaintext TLS mode with a `NetError::TlsError` rather than silently downgrading. Use plaintext schemes for now.

`duckdb://` is recognized as an embedded URI, but this crate is a TCP driver layer and rejects it; embedded DuckDB/DuckPGQ execution lives in Voyager's Python package (`DuckDbBridge`), not here.

## Related projects

`voyager-drivers` executes queries compiled by [`voyager-ogm`](https://crates.io/crates/voyager-ogm). [`voyager-cli`](https://crates.io/crates/voyager-cli) installs the `voyager` diagnostic binary. Python users get this driver layer inside the [`voyager-ogm`](https://pypi.org/project/voyager-ogm/) package on PyPI.

## MSRV

Rust **1.87.0** (the workspace `rust-version`).

## License

MIT OR Apache-2.0. See [LICENSE-MIT](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE-MIT) and [LICENSE-APACHE](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE-APACHE).

## Links

- Repository: <https://github.com/tanishq-kumar/voyager>
- API docs: <https://docs.rs/voyager-drivers>
- Changelog: <https://github.com/tanishq-kumar/voyager/blob/main/CHANGELOG.md>
