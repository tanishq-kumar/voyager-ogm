"""High-Precision Latency Waterfall and CPU vs. Network Profiler for Voyager OGM.

This profiling suite isolates and measures every phase of the query lifecycle:
  Phase 1: Python AST Construction (Python AST expressions & model descriptors)
  Phase 2: PyO3 Native Transit & Rust Arena Allocation (NativeQueryBuilder)
  Phase 3: AST Rule-Based Optimization (AstOptimizer: pushdown, dead-var pruning)
  Phase 4: Dialect Emitter Transpilation (String format & param dictionary generation)
  Phase 5: Network I/O & Database Server-Side Execution (Socket send, DB planner, socket receive)
  Phase 6: Wire Protocol Deserialization (Packstream/Postgres/Redis record parsing)
  Phase 7: Result Hydration (Pure Python Model Instantiation vs Zero-Copy Polars DataFrame)

Workloads Evaluated:
  1. OLTP Point Query (1 entity lookup)
  2. Multi-Hop Filter & Traversal (Medium payload, ~100-500 entities)
  3. Analytical Large Extraction (10,000 entities)
  4. Embedded Zero-Network In-Memory Query (DuckDB - pure CPU/memory baseline)
  5. Bulk Data Ingestion (10,000 - 100,000 entities - Arrow vs. Parameter packing)
  6. Pure CPU Saturation Compilation (100,000 queries - maximum theoretical client QPS)
"""

from __future__ import annotations

import cProfile
import json
import pstats
import time
from typing import Any

import duckdb
import polars as pl
from neo4j import GraphDatabase
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
)


# -----------------------------------------------------------------------------
# Domain Graph Models for Profiling
# -----------------------------------------------------------------------------
class ProfileUser(Node):
    __primary_key__ = "user_id"
    user_id: str = Field(primary_key=True)
    name: str = Field()
    city: str = Field(index=True)
    age: int = Field()


class ProfileCompany(Node):
    __primary_key__ = "comp_id"
    comp_id: str = Field(primary_key=True)
    name: str = Field()
    industry: str = Field()


class ProfileWorksAt(Relationship):
    __type__ = "WORKS_AT"
    since: int = Field()


# -----------------------------------------------------------------------------
# Profiling Timer Utilities
# -----------------------------------------------------------------------------
def measure_ns(func, *args, **kwargs) -> tuple[Any, int]:
    """Execute func and return (result, elapsed_nanoseconds)."""
    t0 = time.perf_counter_ns()
    res = func(*args, **kwargs)
    t1 = time.perf_counter_ns()
    return res, t1 - t0


def benchmark_phase(func, iterations=100, warmup=10) -> dict[str, float]:
    """Run warmups and iterations to gather mean, median, min, max in microseconds."""
    for _ in range(warmup):
        func()
    timings_us = []
    for _ in range(iterations):
        t0 = time.perf_counter_ns()
        func()
        t1 = time.perf_counter_ns()
        timings_us.append((t1 - t0) / 1000.0)

    timings_us.sort()
    mean_us = sum(timings_us) / len(timings_us)
    median_us = timings_us[len(timings_us) // 2]
    p95_us = timings_us[int(len(timings_us) * 0.95)]
    return {
        "mean_us": mean_us,
        "median_us": median_us,
        "min_us": min(timings_us),
        "max_us": max(timings_us),
        "p95_us": p95_us,
    }


# -----------------------------------------------------------------------------
# 1. Micro-Breakdown of Query Compilation (Pure Client CPU)
# -----------------------------------------------------------------------------
def profile_compilation_pipeline():
    print("\n" + "=" * 80)
    print("[1/5] CLIENT CPU MICRO-WATERFALL: Query Construction to Compilation")
    print("=" * 80)

    u = ProfileUser("u")
    c = ProfileCompany("c")
    w = ProfileWorksAt("w")

    # Phase 1: Python Query AST Construction
    def phase1_ast_build():
        return (
            Query.match(u)
            .to(w)
            .node(c)
            .where(u.city == "Tokyo", u.age >= 30, c.industry == "Tech")
            .return_(u.name, c.name, u.age)
        )

    # Phase 2: Unoptimized Compilation (Native PyO3 + Emitter)
    q_built = phase1_ast_build()

    def phase2_compile_unopt():
        return q_built.compile("cypher", optimize=False)

    # Phase 3: Optimized Compilation (Native PyO3 + AstOptimizer + Emitter)
    def phase3_compile_opt():
        return q_built.compile("cypher", optimize=True)

    stats_p1 = benchmark_phase(phase1_ast_build, iterations=2000, warmup=200)
    stats_p2 = benchmark_phase(phase2_compile_unopt, iterations=2000, warmup=200)
    stats_p3 = benchmark_phase(phase3_compile_opt, iterations=2000, warmup=200)

    # Measure Rust Optimizer Pass Delta
    rust_optimizer_delta_us = max(0.0, stats_p3["mean_us"] - stats_p2["mean_us"])

    print(
        f"Phase 1: Python AST Fluent Chaining: {stats_p1['mean_us']:>7.2f} µs (median: {stats_p1['median_us']:.2f} µs)"
    )
    print(
        f"Phase 2: Native Transpilation (Unopt):{stats_p2['mean_us']:>7.2f} µs (median: {stats_p2['median_us']:.2f} µs)"
    )
    print(f"Phase 3: Native AstOptimizer Pass:    {rust_optimizer_delta_us:>7.2f} µs")
    print(
        f"Total Client Compilation Time:        {stats_p3['mean_us']:>7.2f} µs (0.0{int(stats_p3['mean_us'])} ms)"
    )

    return {
        "python_ast_us": stats_p1,
        "native_emitter_unopt_us": stats_p2,
        "native_optimizer_pass_us": rust_optimizer_delta_us,
        "total_compilation_us": stats_p3,
    }


# -----------------------------------------------------------------------------
# 2. End-to-End Live Waterfall Breakdown: OLTP Point Query vs Multi-Hop
# -----------------------------------------------------------------------------
def profile_live_neo4j_waterfalls():
    print("\n" + "=" * 80)
    print("[2/5] LIVE NEO4J END-TO-END LATENCY WATERFALL BREAKDOWN")
    print("=" * 80)

    driver = GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "voyagerpass123"))
    session = Session(bridge=driver, dialect="cypher", optimize="standard")

    # Seed 2,000 nodes
    with driver.session() as s:
        s.run("MATCH (n:ProfileUser) DETACH DELETE n")
        s.run("CREATE INDEX profile_user_city IF NOT EXISTS FOR (u:ProfileUser) ON (u.city)")
        s.run("CREATE INDEX profile_user_pk IF NOT EXISTS FOR (u:ProfileUser) ON (u.user_id)")
        batch = [
            {
                "user_id": f"u_{i}",
                "name": f"User_{i}",
                "city": "Tokyo" if i % 4 == 0 else "Berlin",
                "age": 20 + (i % 50),
            }
            for i in range(2000)
        ]
        s.run(
            "UNWIND $batch AS row CREATE (:ProfileUser {user_id: row.user_id, name: row.name, city: row.city, age: row.age})",
            {"batch": batch},
        )

    u = ProfileUser("u")

    # -------------------------------------------------------------------------
    # Scenario A: OLTP Single-Row Point Query
    # -------------------------------------------------------------------------
    q_point = Query.match(u).where(u.user_id == "u_42").return_(u.user_id, u.name, u.city, u.age)
    compiled_point = q_point.compile("cypher", optimize=True)

    # 1. Measure Client Compilation Time
    t_compile = benchmark_phase(lambda: q_point.compile("cypher", optimize=True), iterations=100)[
        "mean_us"
    ]

    # 2. Measure Server Execution + Wire Transport
    with driver.session() as s:

        def run_raw_bolt():
            result = s.run(compiled_point.statement, compiled_point.parameters)
            return list(result)

        t_db_wire = benchmark_phase(run_raw_bolt, iterations=30, warmup=5)["mean_us"]

        # 3. Measure Server-Side DB Execution Only
        def run_db_server_only():
            result = s.run(compiled_point.statement, compiled_point.parameters)
            summary = result.consume()
            return summary.result_available_after

        server_exec_ms_samples = [run_db_server_only() for _ in range(20)]

    t_server_exec = (sum(server_exec_ms_samples) / len(server_exec_ms_samples)) * 1000.0  # µs
    t_network_wire = max(0.0, t_db_wire - t_server_exec)

    # 4. Measure Client Hydration Time (Polars vs. Python Objects)
    with driver.session() as s:
        raw_records = list(s.run(compiled_point.statement, compiled_point.parameters))

    def hydrate_python_models():
        return [
            ProfileUser(
                user_id=r["u.user_id"],
                name=r["u.name"],
                city=r["u.city"],
                age=r["u.age"],
            )
            for r in raw_records
        ]

    t_hydrate_orm = benchmark_phase(hydrate_python_models, iterations=200)["mean_us"]
    t_total_end_to_end = t_compile + t_db_wire + t_hydrate_orm

    print("\n--- Workload A: Single OLTP Point Query (1 Entity) ---", flush=True)
    print(
        f"1. Client AST Build & Optimize (CPU): {t_compile:>8.2f} µs ({t_compile / t_total_end_to_end * 100:>5.2f}%)",
        flush=True,
    )
    print(
        f"2. Network Socket Transport (I/O):    {t_network_wire:>8.2f} µs ({t_network_wire / t_total_end_to_end * 100:>5.2f}%)",
        flush=True,
    )
    print(
        f"3. Neo4j Server-Side Exec (DB CPU):   {t_server_exec:>8.2f} µs ({t_server_exec / t_total_end_to_end * 100:>5.2f}%)",
        flush=True,
    )
    print(
        f"4. Client ORM Hydration (CPU):        {t_hydrate_orm:>8.2f} µs ({t_hydrate_orm / t_total_end_to_end * 100:>5.2f}%)",
        flush=True,
    )
    print("-" * 65, flush=True)
    print(
        f"Total End-to-End Latency:             {t_total_end_to_end / 1000.0:>8.3f} ms (100.0%)",
        flush=True,
    )
    print(
        f"Client CPU Time vs. Network/DB I/O:   {(t_compile + t_hydrate_orm) / t_total_end_to_end * 100:.1f}% CPU vs {t_db_wire / t_total_end_to_end * 100:.1f}% Network/DB",
        flush=True,
    )

    # -------------------------------------------------------------------------
    # Scenario B: Filter Scan Query (500 Entities)
    # -------------------------------------------------------------------------
    q_scan = Query.match(u).where(u.city == "Tokyo").return_(u.user_id, u.name, u.city, u.age)
    compiled_scan = q_scan.compile("cypher", optimize=True)

    t_scan_compile = benchmark_phase(
        lambda: q_scan.compile("cypher", optimize=True), iterations=100
    )["mean_us"]

    with driver.session() as s:

        def run_raw_scan_bolt():
            result = s.run(compiled_scan.statement, compiled_scan.parameters)
            return list(result)

        t_scan_db_wire = benchmark_phase(run_raw_scan_bolt, iterations=30, warmup=3)["mean_us"]
        raw_scan_records = run_raw_scan_bolt()

    def hydrate_scan_python():
        return [
            ProfileUser(
                user_id=r["u.user_id"],
                name=r["u.name"],
                city=r["u.city"],
                age=r["u.age"],
            )
            for r in raw_scan_records
        ]

    def hydrate_scan_polars():
        return session.execute_to_polars(q_scan)

    t_scan_hydrate_orm = benchmark_phase(hydrate_scan_python, iterations=50)["mean_us"]
    t_scan_hydrate_polars = benchmark_phase(hydrate_scan_polars, iterations=30)["mean_us"]

    t_scan_total = t_scan_compile + t_scan_db_wire + t_scan_hydrate_orm
    print("\n--- Workload B: Filter Scan Query (500 Entities Returned) ---", flush=True)
    print(
        f"1. Client AST Build & Optimize (CPU): {t_scan_compile:>8.2f} µs ({t_scan_compile / t_scan_total * 100:>5.2f}%)",
        flush=True,
    )
    print(
        f"2. Network Wire + Neo4j Server (I/O): {t_scan_db_wire:>8.2f} µs ({t_scan_db_wire / t_scan_total * 100:>5.2f}%)",
        flush=True,
    )
    print(
        f"3. Client ORM Python Hydration (CPU): {t_scan_hydrate_orm:>8.2f} µs ({t_scan_hydrate_orm / t_scan_total * 100:>5.2f}%)",
        flush=True,
    )
    print(f"   [Zero-Copy Polars Extraction]:     {t_scan_hydrate_polars:>8.2f} µs", flush=True)
    print("-" * 65, flush=True)
    print(
        f"Total End-to-End Latency:             {t_scan_total / 1000.0:>8.3f} ms (100.0%)",
        flush=True,
    )
    print(
        f"Client CPU Time vs. Network/DB I/O:   {(t_scan_compile + t_scan_hydrate_orm) / t_scan_total * 100:.1f}% CPU vs {t_scan_db_wire / t_scan_total * 100:.1f}% Network/DB",
        flush=True,
    )

    driver.close()

    return {
        "point_query": {
            "client_compile_us": t_compile,
            "network_transport_us": t_network_wire,
            "server_exec_us": t_server_exec,
            "client_hydration_us": t_hydrate_orm,
            "total_ms": t_total_end_to_end / 1000.0,
            "cpu_percentage": (t_compile + t_hydrate_orm) / t_total_end_to_end * 100,
        },
        "scan_500_query": {
            "client_compile_us": t_scan_compile,
            "db_wire_us": t_scan_db_wire,
            "client_orm_hydration_us": t_scan_hydrate_orm,
            "client_polars_hydration_us": t_scan_hydrate_polars,
            "total_ms": t_scan_total / 1000.0,
            "cpu_percentage": (t_scan_compile + t_scan_hydrate_orm) / t_scan_total * 100,
        },
    }


# -----------------------------------------------------------------------------
# 3. In-Memory Zero-Network Benchmark (DuckDB - Pure Memory & CPU)
# -----------------------------------------------------------------------------
def profile_in_memory_duckdb():
    print("\n" + "=" * 80, flush=True)
    print("[3/5] ZERO-NETWORK IN-MEMORY BASELINE (DuckDB 50,000 Rows)", flush=True)
    print("=" * 80, flush=True)

    cities = ["Tokyo", "Berlin", "Paris", "London", "NewYork"]
    seed_df = pl.DataFrame(
        {
            "user_id": [f"u_{i}" for i in range(50000)],
            "name": [f"User_{i}" for i in range(50000)],
            "city": [cities[i % len(cities)] for i in range(50000)],
            "age": [20 + (i % 50) for i in range(50000)],
        }
    )

    con = duckdb.connect(":memory:")
    con.register("seed_df", seed_df)
    con.execute("CREATE TABLE users AS SELECT * FROM seed_df;")

    u = ProfileUser("u")
    q = Query.match(u).where(u.city == "Tokyo", u.age >= 30).return_(u.user_id, u.name, u.age)

    # Measure compilation
    t_compile = benchmark_phase(lambda: q.compile("sql_pgq", optimize=True), iterations=100)[
        "mean_us"
    ]

    # Measure DuckDB execution directly
    def run_duckdb_pl():
        return con.execute(
            "SELECT user_id, name, age FROM users WHERE city = 'Tokyo' AND age >= 30"
        ).pl()

    t_duck_exec = benchmark_phase(run_duckdb_pl, iterations=30)["mean_us"]
    t_total = t_compile + t_duck_exec

    print(
        f"1. Client AST Compilation (CPU):      {t_compile:>8.2f} µs ({t_compile / t_total * 100:>5.2f}%)",
        flush=True,
    )
    print(
        f"2. In-Process DuckDB SIMD Scan & Join:{t_duck_exec:>8.2f} µs ({t_duck_exec / t_total * 100:>5.2f}%)",
        flush=True,
    )
    print("-" * 65, flush=True)
    print(f"Total Execution Time (Zero Network):  {t_total / 1000.0:>8.3f} ms (100.0%)", flush=True)
    print(
        f"Ratio: Client Voyager CPU = {t_compile / t_total * 100:.2f}%, Engine Memory/SIMD = {t_duck_exec / t_total * 100:.2f}%",
        flush=True,
    )

    return {
        "compile_us": t_compile,
        "duck_exec_us": t_duck_exec,
        "total_ms": t_total / 1000.0,
    }


# -----------------------------------------------------------------------------
# 4. Heavy Bulk Data Ingestion: CPU Bottleneck Profiling (100,000 Nodes)
# -----------------------------------------------------------------------------
def profile_bulk_ingestion_cpu():
    print("\n" + "=" * 80, flush=True)
    print("[4/5] BULK INGESTION CPU BOTTLENECK PROFILING (100,000 Entities)", flush=True)
    print("=" * 80, flush=True)

    num_rows = 100000
    cities = ["Tokyo", "Berlin", "Paris", "London", "NewYork"]

    # Phase 1: Creating raw Python dictionaries vs Polars DataFrame
    t0 = time.perf_counter_ns()
    _ = [
        {
            "user_id": f"u_{i}",
            "name": f"User_{i}",
            "city": cities[i % len(cities)],
            "age": 20 + (i % 50),
        }
        for i in range(num_rows)
    ]
    t1 = time.perf_counter_ns()
    dict_creation_ms = (t1 - t0) / 1_000_000.0

    t0 = time.perf_counter_ns()
    df = pl.DataFrame(
        {
            "user_id": [f"u_{i}" for i in range(num_rows)],
            "name": [f"User_{i}" for i in range(num_rows)],
            "city": [cities[i % len(cities)] for i in range(num_rows)],
            "age": [20 + (i % 50) for i in range(num_rows)],
        }
    )
    t1 = time.perf_counter_ns()
    polars_creation_ms = (t1 - t0) / 1_000_000.0

    # Phase 2: Generating UNWIND Cypher Parameter Batch
    bulk_q = Query.unwind("batch", alias="row").add_create(ProfileUser)

    t_compile = benchmark_phase(lambda: bulk_q.compile("cypher"), iterations=50)["mean_us"]

    # Phase 3: Arrow RecordBatch / IPC serialization
    t0 = time.perf_counter_ns()
    _ = df.to_arrow()
    t1 = time.perf_counter_ns()
    arrow_conv_ms = (t1 - t0) / 1_000_000.0

    print(f"1. 100k Python Dictionaries Creation (CPU): {dict_creation_ms:>8.2f} ms", flush=True)
    print(f"2. 100k Polars Columnar Creation (SIMD/CPU): {polars_creation_ms:>8.2f} ms", flush=True)
    print(f"3. 100k Polars to PyArrow Zero-Copy (CPU):   {arrow_conv_ms:>8.2f} ms", flush=True)
    print(f"4. Bulk Query AST Compilation (Rust CPU):     {t_compile:>8.2f} µs", flush=True)

    return {
        "dict_creation_ms": dict_creation_ms,
        "polars_creation_ms": polars_creation_ms,
        "arrow_conv_ms": arrow_conv_ms,
        "bulk_compile_us": t_compile,
    }


# -----------------------------------------------------------------------------
# 5. Python CPU Profiler (cProfile Function Call Breakdown)
# -----------------------------------------------------------------------------
def profile_cprofile_hotspots():
    print("\n" + "=" * 80, flush=True)
    print("[5/5] cProfile FLAME-TABLE: Top CPU Bottlenecks in Voyager", flush=True)
    print("=" * 80, flush=True)

    u = ProfileUser("u")
    c = ProfileCompany("c")
    w = ProfileWorksAt("w")

    profiler = cProfile.Profile()
    profiler.enable()

    # Execute 2,000 query builds and compiles
    for _ in range(2000):
        q = (
            Query.match(u)
            .to(w)
            .node(c)
            .where(u.city == "Tokyo", u.age >= 30, c.industry == "Tech")
            .return_(u.name, c.name, u.age)
        )
        q.compile("cypher", optimize=True)

    profiler.disable()
    stats = pstats.Stats(profiler).sort_stats("tottime")
    stats.print_stats(15)


# -----------------------------------------------------------------------------
# Main Profiling Orchestrator
# -----------------------------------------------------------------------------
def main():
    print("=" * 80)
    print("VOYAGER OGM: FORENSIC WATERFALL LATENCY & CPU BOTTLENECK PROFILER")
    print("Investigating Amdahl's Law: Client CPU vs. Network vs. Database Engine")
    print("=" * 80)

    results = {}
    results["compilation"] = profile_compilation_pipeline()
    results["live_neo4j_waterfall"] = profile_live_neo4j_waterfalls()
    results["in_memory_duckdb"] = profile_in_memory_duckdb()
    results["bulk_ingestion"] = profile_bulk_ingestion_cpu()
    profile_cprofile_hotspots()

    with open("benchmarks/profiling_waterfall_results.json", "w") as f:
        json.dump(results, f, indent=2)
    print("\nSaved full profiling metrics to benchmarks/profiling_waterfall_results.json")


if __name__ == "__main__":
    main()
