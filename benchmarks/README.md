# Voyager OGM — Benchmark Suite

This directory holds the benchmark scripts, the chart data, and the published captures. The figures below are the only performance numbers published by the project. They were captured before the v0.4.0 changes (native parameter binding, the MERGE-key policy, partial bulk MERGE, and the real-transaction/UnitOfWork rework), so they are historical reference points, not current performance.

## Published figures

### Hydration representation cost (10,000 rows)

| Representation | Rows/sec | Method |
| :--- | ---: | :--- |
| Polars `pl.DataFrame(dicts)` | 1,013,607 | 10,000 rows pre-fetched with the official `neo4j` driver; in-memory dict → frame conversion; mean of 5 rounds |
| Pydantic v2 `BaseModel` construction | 280,899 | Same rows; model construction; mean of 5 rounds |
| `neomodel.StructuredNode.inflate()` | 23,016 | Same rows; model inflation; mean of 5 rounds |

Source: `packages/python/benches/bench_head_to_head.py`. Captured 2026-09-13 (`benchmarks/benchmark_summary.json`). This measures representation cost, not end-to-end OGM hydration: the database fetch is outside the timer.

### Point-read latency (20 clients x 1,000 reads)

| Driver | p50 | p99 | Method |
| :--- | ---: | ---: | :--- |
| Voyager native Bolt client | 12.5 ms | 49.5 ms | Same live Neo4j, same statement, 20 concurrent clients x 1,000 point reads; single run |
| `neo4rs` 0.8.0 (community Rust driver) | 12.1 ms | 123.8 ms | Same scenario |

Source: `crates/voyager-net/benches/bench_driver_vs_voyager_net.rs`. Captured on or before 2026-09-12 (when these numbers were first committed). This is a Rust-client comparison, not a comparison against an official Neo4j driver. The script writes its JSON results to a gitignored path, so p95 is deliberately not published.

### Not published

No memory/RSS chart or memory-per-node figure is published. The repository contains no script that measures process RSS, so those numbers were removed. Likewise, the official-driver throughput row and the p95 values had no committed source and were removed.

## Environment

Versions come from `containers/compose.yaml` and the workspace manifests:

- Rust workspace: edition 2024, MSRV **1.87.0**; PyO3 **0.29** (`abi3-py311`).
- Python: **>= 3.11**.
- Neo4j: `neo4j:5.26-community` by default (Enterprise is opt-in and needs a licence).
- Memgraph: `memgraph-mage:3.14.0`.
- Apache AGE: `apache/age:release_PG18_1.8.0` (PostgreSQL on port 5455).
- FalkorDB: `falkordb:6.0.4`.
- PostgreSQL: `postgres:19beta3-alpine`.

The published captures have no committed hardware metadata: their CPU, RAM and OS are unknown and are not claimed here. The scripts use the compose credentials (`neo4j` / `voyagerpass123`) and ports listed above.

## Reproduce

```bash
just up                      # start the compose stack; required by every live benchmark
just bench-python            # offline Python benches (live modules skip without containers)
just bench-ogm               # live Neo4j head-to-head; seeds BenchmarkPerson itself
just bench                   # cargo bench --workspace; runs live Neo4j benches too, so needs `just up`
just bench-save              # append pytest-benchmark rows to latest_summary.csv
just bench-charts            # regenerate the SVGs from benchmark_summary.json
```

`just bench` is **not** offline: `bench_live_bolt` and `bench_jumbo_payloads` connect to Neo4j at `127.0.0.1:7687`.

### Seeding order for the native driver benches

`bench_live_bolt` seeds the data the other driver benches assume:

1. Start the stack: `just up`.
2. Seed and run the live scenarios: `cargo bench -p voyager-net --bench bench_live_bolt` (creates 10,000 `BenchPerson` nodes and 20,000 `KNOWS` edges).
3. Run the comparison: `cargo bench -p voyager-net --features bench-driver --bench bench_driver_vs_voyager_net` (asserts 10,000 rows).
4. For the 1M comparison: `uv run python scripts/seed_1m_dataset.py`, then `cargo bench -p voyager-net --features bench-driver --bench bench_1m_driver_vs_voyager`.

`just bench-ogm` seeds a different label (`BenchmarkPerson`) and does not prepare the Rust benches.

## Per-benchmark map

| Script | Measures | Does not measure | Reproduce | Containers / seeding |
| :--- | :--- | :--- | :--- | :--- |
| `benchmarks/complex_path_large_scale_benchmark.py` | Per-engine 2/3-hop opt-vs-unopt latency on six engines | Fair cross-engine speedups (row counts differ per engine); Voyager's optimizer for SQL variants (hand-written SQL) | `uv run python benchmarks/complex_path_large_scale_benchmark.py` | All six engines; seeds its own data |
| `benchmarks/live_6engine_optimizer_benchmark.py` | Per-engine opt-vs-unopt latency and throughput | Equal dataset sizes (AGE uses fewer rows); cross-engine claims | `uv run python benchmarks/live_6engine_optimizer_benchmark.py` | All six engines; seeds its own data |
| `benchmarks/live_profile_constant_folding.py` | Neo4j `PROFILE` dbHits, operator trees and interleaved latency | Other engines; client-side compilation cost | `uv run python benchmarks/live_profile_constant_folding.py` | Neo4j; seeds 10,000 nodes itself |
| `benchmarks/profile_constant_folding.py` | Constant-folding compile cost, AST reduction and DuckDB execution on 500k rows | Live database or network behaviour | `uv run python benchmarks/profile_constant_folding.py` | Offline (`duckdb`) |
| `benchmarks/profiler_waterfall_breakdown.py` | Per-phase CPU/network waterfall; live Neo4j and DuckDB workloads | Repeatable published numbers (writes a gitignored JSON) | `uv run python benchmarks/profiler_waterfall_breakdown.py` | Neo4j for live phases; DuckDB phases offline |
| `packages/python/benches/bench_compilation.py` | Voyager Python compile/emit cost vs in-file pure-Python builder stand-ins | Real GQLAlchemy or Neomodel libraries; execution | `just bench-python` | Offline |
| `packages/python/benches/bench_head_to_head.py` | Live fetch, compilation, representation cost and end-to-end latency | Voyager's `execute_to_polars` path (uses `pl.DataFrame` directly) | `just bench-ogm` | Neo4j; seeds and deletes `BenchmarkPerson` itself |
| `packages/python/benches/bench_hydration.py` | Synthetic stream → Arrow/Polars and pure-Python object construction | Live wire hydration | `just bench-python` | Offline |
| `packages/python/benches/bench_ingestion.py` | Bulk-ingestion plan generation and chunking | Execution throughput (no database calls) | `just bench-python` | Offline |
| `packages/python/benches/bench_network.py` | Native-client point-query latency, QPS, Arrow decode and GIL behaviour | Official Python drivers; Voyager vs driver comparison | `just bench-python` | Neo4j and FalkorDB; live modules skip when offline |
| `crates/voyager-net/benches/bench_live_bolt.rs` | Live native Bolt scenarios, including ingestion, and seeds the 10k dataset | Comparison against another driver | `cargo bench -p voyager-net --bench bench_live_bolt` | Neo4j; seeds 10,000 nodes + 20,000 edges |
| `crates/voyager-net/benches/bench_driver_vs_voyager_net.rs` | Native Bolt client vs `neo4rs` 0.8.0 on identical queries | Official Neo4j driver; committed results (JSON is gitignored) | `cargo bench -p voyager-net --features bench-driver --bench bench_driver_vs_voyager_net` | Neo4j; needs the 10k seed above |
| `crates/voyager-net/benches/bench_1m_driver_vs_voyager.rs` | Native Bolt client vs `neo4rs` 0.8.0 at 1M nodes / 2M edges | Seeding automation | `uv run python scripts/seed_1m_dataset.py` then `cargo bench -p voyager-net --features bench-driver --bench bench_1m_driver_vs_voyager` | Neo4j; 1M-node seed |
| `crates/voyager-net/benches/bench_jumbo_payloads.rs` | 10/25/50 MB payload transport over the native Bolt stream | Ingestion semantics or driver comparison | `cargo bench -p voyager-net --bench bench_jumbo_payloads` | Neo4j |
| `crates/voyager-net/benches/bench_security_overhead.rs` | PackStream, Redis and PostgreSQL security-hardening overhead | Live database behaviour | `cargo bench -p voyager-net --bench bench_security_overhead` | Offline |
| `crates/voyager-ogm/benches/arena_bench.rs` | Criterion AST arena allocation and fluent builder construction | Emission or database access | `cargo bench -p voyager-ogm --bench arena_bench` | Offline |
| `crates/voyager-ogm/benches/emitter_bench.rs` | Criterion Cypher, ISO GQL and SQL/PGQ emitter throughput | Apache AGE emitter; live database | `cargo bench -p voyager-ogm --bench emitter_bench` | Offline |

## Caveats

- The driver comparison pre-warms the Voyager pool but not `neo4rs`, and runs once; it is not a statistical benchmark.
- `just bench-python` collects `bench_*.py` via an explicit `python_files` override; the global pytest pattern stays `test_*.py`.
- Results JSON written by these scripts is gitignored by design. Only `benchmark_summary.json`, `latest_summary.csv` and the two SVGs are committed.
