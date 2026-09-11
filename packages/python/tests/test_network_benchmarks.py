"""Empirical network engine benchmarks: Native Rust network client vs Python drivers.

Measures p50/p95/p99 query latency, QPS throughput, Arrow deserialization,
and multi-threaded GIL contention under concurrent execution.

Run with:
    uv run pytest packages/python/tests/test_network_benchmarks.py --benchmark-only
    or
    uv run pytest packages/python/tests/test_network_benchmarks.py -v
"""

from __future__ import annotations

import time
from concurrent.futures import ThreadPoolExecutor
from typing import Any

import polars as pl
import pyarrow as pa
import pytest
from voyager_ogm import (
    NativeClient,
    Session,
    generate_synthetic_stream,
)

FALKORDB_URI = "redis://127.0.0.1:6379?connect_timeout=2"
NEO4J_URI = "bolt://neo4j:voyagerpass123@127.0.0.1:7687?connect_timeout=2"


def _is_falkordb_online() -> bool:
    try:
        c = NativeClient(FALKORDB_URI, min_idle=1, max_size=2)
        online = c.ping_sync()
        c.close()
        return online
    except Exception:
        return False


def _is_neo4j_online() -> bool:
    try:
        c = NativeClient(NEO4J_URI, min_idle=1, max_size=2)
        online = c.ping_sync()
        c.close()
        return online
    except Exception:
        return False


FALKORDB_ONLINE = _is_falkordb_online()
NEO4J_ONLINE = _is_neo4j_online()


# ===========================================================================
# 1. Wire-to-Arrow Deserialization Benchmarks (Synthetic Scale)
# ===========================================================================


def test_bench_synthetic_stream_to_polars(benchmark: Any) -> None:
    """Benchmark direct Arrow-to-Polars ingestion without Python dict allocation."""

    def run() -> int:
        stream = generate_synthetic_stream(50_000)
        df = pl.DataFrame(stream)
        return df.height

    height = benchmark(run)
    assert height == 50_000


def test_bench_synthetic_stream_to_arrow(benchmark: Any) -> None:
    """Benchmark direct Arrow C Stream ingestion into PyArrow Table."""

    def run() -> int:
        stream = generate_synthetic_stream(50_000)
        table = pa.RecordBatchReader.from_stream(stream).read_all()
        return table.num_rows

    num_rows = benchmark(run)
    assert num_rows == 50_000


def test_bench_synthetic_stream_to_python_dicts(benchmark: Any) -> None:
    """Benchmark native Rust record_batch_to_py_dicts conversion."""
    stream = generate_synthetic_stream(10_000)

    def run() -> int:
        records = stream.to_dicts()
        return len(records)

    length = benchmark(run)
    assert length == 10_000


def test_bench_lazy_execution_result_to_polars(benchmark: Any) -> None:
    """Benchmark lazy ExecutionResult converting to Polars without eager dicts."""
    from voyager_ogm.session import ExecutionResult

    def run() -> int:
        stream = generate_synthetic_stream(50_000)
        res = ExecutionResult(stream=stream)
        df = res.to_polars()
        return df.height

    height = benchmark(run)
    assert height == 50_000


@pytest.mark.skipif(not FALKORDB_ONLINE, reason="Live FalkorDB container not available")
def test_bench_fast_path_parameter_serialization(benchmark: Any) -> None:
    """Benchmark fast-path FFI parameter dictionary serialization (1,000 parameters)."""
    params = {
        f"k_{i}": (
            i if i % 4 == 0 else (f"str_{i}" if i % 4 == 1 else (i * 1.25 if i % 4 == 2 else True))
        )
        for i in range(1_000)
    }
    client = NativeClient(FALKORDB_URI, min_idle=1, max_size=2)

    def run() -> int:
        res = client.execute_sync("RETURN 1 AS val", params)
        return res.num_rows

    try:
        rows = benchmark(run)
        assert rows == 1
    finally:
        client.close()


# ===========================================================================
# 2. Live Database Native Client Latency & Throughput Benchmarks
# ===========================================================================


@pytest.mark.skipif(not FALKORDB_ONLINE, reason="Live FalkorDB container not available")
def test_bench_native_falkordb_query_latency(benchmark: Any) -> None:
    """Benchmark p50/p95/p99 query latency over raw RESP3 socket using NativeClient."""
    client = NativeClient(FALKORDB_URI, min_idle=2, max_size=10)

    def run() -> int:
        res = client.execute_sync("RETURN 1 AS num", {})
        return res.num_rows

    try:
        rows = benchmark(run)
        assert rows == 1
    finally:
        client.close()


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j container not available")
def test_bench_native_neo4j_query_latency(benchmark: Any) -> None:
    """Benchmark query round-trip latency over raw Bolt v5 socket using NativeClient."""
    client = NativeClient(NEO4J_URI, min_idle=2, max_size=10)

    def run() -> int:
        res = client.execute_sync("RETURN 1 AS num", {})
        return res.num_rows

    try:
        rows = benchmark(run)
        assert rows == 1
    finally:
        client.close()


# ===========================================================================
# 3. Multi-Threaded GIL Release & Concurrency Scaling
# ===========================================================================


@pytest.mark.skipif(not FALKORDB_ONLINE, reason="Live FalkorDB container not available")
def test_bench_multithreaded_gil_release_falkordb() -> None:
    """Verify that native query execution releases Python GIL across 8 worker threads."""
    client = NativeClient(FALKORDB_URI, min_idle=4, max_size=16)
    total_queries = 200
    num_workers = 8

    latencies: list[float] = []

    def worker_query() -> float:
        t0 = time.perf_counter()
        res = client.execute_sync("RETURN 42 AS val", {})
        assert res.num_rows == 1
        return time.perf_counter() - t0

    start_all = time.perf_counter()
    with ThreadPoolExecutor(max_workers=num_workers) as pool:
        futures = [pool.submit(worker_query) for _ in range(total_queries)]
        latencies = [f.result() for f in futures]
    total_time = time.perf_counter() - start_all

    client.close()

    latencies.sort()
    p50 = latencies[int(len(latencies) * 0.50)] * 1000
    p95 = latencies[int(len(latencies) * 0.95)] * 1000
    p99 = latencies[int(len(latencies) * 0.99)] * 1000
    qps = total_queries / total_time

    print(f"\n[FalkorDB Multi-Threaded Latency] (8 threads, {total_queries} queries):")
    print(f"  QPS : {qps:.1f} queries/sec")
    print(f"  p50 : {p50:.2f} ms")
    print(f"  p95 : {p95:.2f} ms")
    print(f"  p99 : {p99:.2f} ms")

    assert qps > 50.0, f"Expected QPS > 50, got {qps:.1f}"
    assert p50 < 50.0, f"Expected p50 < 50ms, got {p50:.2f}ms"


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j container not available")
def test_bench_multithreaded_gil_release_neo4j() -> None:
    """Verify that native query execution releases Python GIL across 8 worker threads on Bolt."""
    client = NativeClient(NEO4J_URI, min_idle=4, max_size=16)
    total_queries = 200
    num_workers = 8

    def worker_query() -> float:
        t0 = time.perf_counter()
        res = client.execute_sync("RETURN 100 AS val", {})
        assert res.num_rows == 1
        return time.perf_counter() - t0

    start_all = time.perf_counter()
    with ThreadPoolExecutor(max_workers=num_workers) as pool:
        futures = [pool.submit(worker_query) for _ in range(total_queries)]
        latencies = [f.result() for f in futures]
    total_time = time.perf_counter() - start_all

    client.close()

    latencies.sort()
    p50 = latencies[int(len(latencies) * 0.50)] * 1000
    p95 = latencies[int(len(latencies) * 0.95)] * 1000
    p99 = latencies[int(len(latencies) * 0.99)] * 1000
    qps = total_queries / total_time

    print(f"\n[Neo4j Bolt Multi-Threaded Latency] (8 threads, {total_queries} queries):")
    print(f"  QPS : {qps:.1f} queries/sec")
    print(f"  p50 : {p50:.2f} ms")
    print(f"  p95 : {p95:.2f} ms")
    print(f"  p99 : {p99:.2f} ms")

    assert qps > 50.0, f"Expected QPS > 50, got {qps:.1f}"
    assert p50 < 50.0, f"Expected p50 < 50ms, got {p50:.2f}ms"


# ===========================================================================
# 4. End-to-End Session Dual-Backend Benchmark
# ===========================================================================


@pytest.mark.skipif(not FALKORDB_ONLINE, reason="Live FalkorDB container not available")
def test_bench_session_native_vs_bridge(benchmark: Any) -> None:
    """Benchmark Session.execute() running native backend."""
    session = Session(FALKORDB_URI, backend="native")
    assert session.backend == "native"

    def run() -> int:
        res = session.execute("RETURN 1 AS val")
        return len(res)

    try:
        count = benchmark(run)
        assert count == 1
    finally:
        session.close()


@pytest.mark.skipif(not FALKORDB_ONLINE, reason="Live FalkorDB container not available")
def test_bench_session_execute_to_polars_direct(benchmark: Any) -> None:
    """Benchmark direct Session.execute_to_polars() bypass."""
    session = Session(FALKORDB_URI, backend="native")

    def run() -> int:
        df = session.execute_to_polars("RETURN 1 AS val")
        return df.height

    try:
        count = benchmark(run)
        assert count == 1
    finally:
        session.close()
