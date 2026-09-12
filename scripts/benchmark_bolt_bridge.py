"""Honest Benchmark Comparison: Official Neo4j Python Driver Bridge vs Voyager.

Runs the exact same 5 production-grade benchmark scenarios against the live Neo4j container:
1. High-Concurrency Point Reads (Indexed 1-Hop Lookup, Concurrency 20)
2. Graph 2-Hop Traversal & Aggregation (Concurrency 10)
3. High-Volume Bulk Result Streaming (10,000 Records Hydrated to Python dicts / Polars)
4. Bulk Ingestion (5,000 Nodes UNWIND $batch)
5. Connection Pool Contention (100 Concurrent Tasks on Pool Size 5)
"""

from __future__ import annotations

import asyncio
import json
import time
from dataclasses import asdict, dataclass

from neo4j import AsyncGraphDatabase

NEO4J_URI = "bolt://127.0.0.1:7687"
NEO4J_AUTH = ("neo4j", "voyagerpass123")


def percentile(data: list[float], p: float) -> float:
    """Compute the p-th percentile from a list of float measurements."""
    if not data:
        return 0.0
    sorted_data = sorted(data)
    idx = int(round((len(sorted_data) - 1) * p / 100.0))
    return sorted_data[idx]


@dataclass
class BenchmarkMetrics:
    """Telemetry metrics container for a single benchmark workload scenario."""

    name: str
    iterations: int
    concurrency: int
    duration_ms: float
    qps: float
    p50_ms: float
    p90_ms: float
    p95_ms: float
    p99_ms: float
    max_ms: float

    @classmethod
    def compute(
        cls,
        name: str,
        latencies: list[float],
        total_duration: float,
        concurrency: int,
    ) -> BenchmarkMetrics:
        """Calculate statistical percentiles and throughput from recorded latencies."""
        sorted_lat = sorted(latencies)
        iterations = len(sorted_lat)
        duration_ms = total_duration * 1000.0
        qps = (iterations / total_duration) if total_duration > 0 else 0.0

        return cls(
            name=name,
            iterations=iterations,
            concurrency=concurrency,
            duration_ms=duration_ms,
            qps=qps,
            p50_ms=percentile(sorted_lat, 50.0),
            p90_ms=percentile(sorted_lat, 90.0),
            p95_ms=percentile(sorted_lat, 95.0),
            p99_ms=percentile(sorted_lat, 99.0),
            max_ms=sorted_lat[-1] if sorted_lat else 0.0,
        )

    def print_row(self) -> None:
        """Print a formatted Markdown table row with scenario metrics."""
        print(
            f"| {self.name:<45} | {self.iterations:>6} | {self.concurrency:>4} | {self.qps:>9.2f} | "
            f"{self.p50_ms:>7.2f} ms | {self.p90_ms:>7.2f} ms | {self.p99_ms:>7.2f} ms | {self.max_ms:>7.2f} ms |"
        )


async def run_benchmarks() -> None:
    """Run the 5 benchmark workloads against the live Neo4j instance."""
    driver = AsyncGraphDatabase.driver(
        NEO4J_URI,
        auth=NEO4J_AUTH,
        max_connection_pool_size=20,
    )

    # Verify connectivity
    await driver.verify_connectivity()

    print(
        "\n================================================================================================================="
    )
    print("                           OFFICIAL NEO4J PYTHON DRIVER (BRIDGE) BENCHMARK SUITE")
    print(
        "================================================================================================================="
    )
    print(
        f"| {'Scenario':<45} | {'Runs':>6} | {'Conc':>4} | {'QPS':>9} | {'p50':>10} | {'p90':>10} | {'p99':>10} | {'Max':>10} |"
    )
    print(
        "|-----------------------------------------------+--------+------+-----------+------------+------------+------------+------------|"
    )

    results: list[BenchmarkMetrics] = []

    # -------------------------------------------------------------------------
    # Scenario 1: Point Reads (1-Hop Lookup, Concurrency 20)
    # -------------------------------------------------------------------------
    iterations_1 = 1000
    concurrency_1 = 20
    query_1 = "MATCH (p:BenchPerson {id: $id}) RETURN p.id AS id, p.name AS name, p.age AS age, p.city AS city, p.score AS score"
    sem_1 = asyncio.Semaphore(concurrency_1)

    async def worker_1(idx: int) -> float:
        async with sem_1:
            q_id = (idx * 17) % 10000
            t0 = time.perf_counter()
            async with driver.session() as session:
                res = await session.run(query_1, id=q_id)
                records = await res.data()
                assert len(records) == 1
            return (time.perf_counter() - t0) * 1000.0

    t_start = time.perf_counter()
    tasks_1 = [asyncio.create_task(worker_1(i)) for i in range(iterations_1)]
    latencies_1 = await asyncio.gather(*tasks_1)
    total_time_1 = time.perf_counter() - t_start

    metrics_1 = BenchmarkMetrics.compute(
        "1. Point Reads (1-Hop Lookup, Concurrency 20)",
        list(latencies_1),
        total_time_1,
        concurrency_1,
    )
    metrics_1.print_row()
    results.append(metrics_1)

    # -------------------------------------------------------------------------
    # Scenario 2: Graph 2-Hop Traversal & Aggregation
    # -------------------------------------------------------------------------
    iterations_2 = 200
    concurrency_2 = 10
    query_2 = """
        MATCH (p:BenchPerson {id: $id})-[:KNOWS]->(f:BenchPerson)-[:KNOWS]->(fof:BenchPerson)
        RETURN fof.city AS city, count(fof) AS cnt, avg(fof.score) AS avg_score
        ORDER BY cnt DESC
        LIMIT 10
    """
    sem_2 = asyncio.Semaphore(concurrency_2)

    async def worker_2(idx: int) -> float:
        async with sem_2:
            q_id = (idx * 31) % 10000
            t0 = time.perf_counter()
            async with driver.session() as session:
                res = await session.run(query_2, id=q_id)
                records = await res.data()
                assert len(records) > 0
            return (time.perf_counter() - t0) * 1000.0

    t_start_2 = time.perf_counter()
    tasks_2 = [asyncio.create_task(worker_2(i)) for i in range(iterations_2)]
    latencies_2 = await asyncio.gather(*tasks_2)
    total_time_2 = time.perf_counter() - t_start_2

    metrics_2 = BenchmarkMetrics.compute(
        "2. Graph 2-Hop Traversal & Aggregation", list(latencies_2), total_time_2, concurrency_2
    )
    metrics_2.print_row()
    results.append(metrics_2)

    # -------------------------------------------------------------------------
    # Scenario 3: High-Volume Bulk Result Streaming (10,000 Records)
    # -------------------------------------------------------------------------
    iterations_3 = 20
    concurrency_3 = 1
    query_3 = """
        MATCH (p:BenchPerson)
        RETURN p.id AS id, p.name AS name, p.age AS age, p.city AS city, p.score AS score, p.active AS active
        LIMIT 10000
    """

    latencies_3 = []
    t_start_3 = time.perf_counter()
    for _ in range(iterations_3):
        t0 = time.perf_counter()
        async with driver.session() as session:
            res = await session.run(query_3)
            records = await res.data()
            assert len(records) == 10000
        latencies_3.append((time.perf_counter() - t0) * 1000.0)
    total_time_3 = time.perf_counter() - t_start_3

    metrics_3 = BenchmarkMetrics.compute(
        "3. Bulk Result Streaming (10k Rows -> Python)", latencies_3, total_time_3, concurrency_3
    )
    metrics_3.print_row()
    results.append(metrics_3)

    # -------------------------------------------------------------------------
    # Scenario 4: Bulk Mutation Ingestion (UNWIND $batch 5,000 Nodes)
    # -------------------------------------------------------------------------
    iterations_4 = 10
    concurrency_4 = 1
    query_4 = """
        UNWIND $batch AS row
        CREATE (p:BenchBulkPy {
            id: row.id,
            name: row.name,
            age: row.age,
            city: row.city,
            score: row.score,
            active: row.active
        })
    """

    latencies_4 = []
    t_start_4 = time.perf_counter()
    for b in range(iterations_4):
        batch = [
            {
                "id": b * 5000 + i,
                "name": f"BulkPy_{b * 5000 + i}",
                "age": 28,
                "city": "Tokyo",
                "score": 99.9,
                "active": True,
            }
            for i in range(5000)
        ]
        t0 = time.perf_counter()
        async with driver.session() as session:
            res = await session.run(query_4, batch=batch)
            summary = await res.consume()
            assert summary.counters.nodes_created == 5000
        latencies_4.append((time.perf_counter() - t0) * 1000.0)
    total_time_4 = time.perf_counter() - t_start_4

    metrics_4 = BenchmarkMetrics.compute(
        "4. Bulk Ingestion (5,000 Nodes UNWIND $batch)", latencies_4, total_time_4, concurrency_4
    )
    metrics_4.print_row()
    results.append(metrics_4)

    await driver.close()

    # -------------------------------------------------------------------------
    # Scenario 5: Connection Pool Contention (100 Concurrent Tasks on Pool Size 5)
    # -------------------------------------------------------------------------
    small_driver = AsyncGraphDatabase.driver(
        NEO4J_URI,
        auth=NEO4J_AUTH,
        max_connection_pool_size=5,
    )
    iterations_5 = 1000
    concurrency_5 = 100
    query_5 = "RETURN $val AS res"
    sem_5 = asyncio.Semaphore(concurrency_5)

    async def worker_5(val: int) -> float:
        async with sem_5:
            t0 = time.perf_counter()
            async with small_driver.session() as session:
                res = await session.run(query_5, val=val)
                records = await res.data()
                assert len(records) == 1
            return (time.perf_counter() - t0) * 1000.0

    t_start_5 = time.perf_counter()
    tasks_5 = [asyncio.create_task(worker_5(i)) for i in range(iterations_5)]
    latencies_5 = await asyncio.gather(*tasks_5)
    total_time_5 = time.perf_counter() - t_start_5

    metrics_5 = BenchmarkMetrics.compute(
        "5. Pool Contention (100 Tasks / 5 Conns)", list(latencies_5), total_time_5, concurrency_5
    )
    metrics_5.print_row()
    results.append(metrics_5)

    # -------------------------------------------------------------------------
    # Scenario 6: Mixed Heterogeneous Column Types (5,000 Rows Variant Types)
    # -------------------------------------------------------------------------
    iterations_6 = 10
    concurrency_6 = 1
    query_6 = """
        UNWIND range(1, 5000) AS i
        RETURN i AS id,
               CASE WHEN i % 3 = 0 THEN i
                    WHEN i % 3 = 1 THEN 'str_val_' + toString(i)
                    ELSE toFloat(i) + 0.5 END AS mixed_val,
               CASE WHEN i % 2 = 0 THEN true ELSE false END AS bool_val
    """

    latencies_6 = []
    t_start_6 = time.perf_counter()
    for _ in range(iterations_6):
        t0 = time.perf_counter()
        async with small_driver.session() as session:
            res = await session.run(query_6)
            records = await res.data()
            assert len(records) == 5000
        latencies_6.append((time.perf_counter() - t0) * 1000.0)
    total_time_6 = time.perf_counter() - t_start_6

    metrics_6 = BenchmarkMetrics.compute(
        "6. Mixed Types Streaming (5k Variant Rows)", latencies_6, total_time_6, concurrency_6
    )
    metrics_6.print_row()
    results.append(metrics_6)

    # -------------------------------------------------------------------------
    # Scenario 7: Jumbo Chunk Payload (5MB Multi-Chunk Frame)
    # -------------------------------------------------------------------------
    iterations_7 = 10
    concurrency_7 = 1
    jumbo_str = "V" * (5 * 1024 * 1024)
    query_7 = "RETURN $payload AS jumbo_str, size($payload) AS len"

    latencies_7 = []
    t_start_7 = time.perf_counter()
    for _ in range(iterations_7):
        t0 = time.perf_counter()
        async with small_driver.session() as session:
            res = await session.run(query_7, payload=jumbo_str)
            records = await res.data()
            assert len(records) == 1
        latencies_7.append((time.perf_counter() - t0) * 1000.0)
    total_time_7 = time.perf_counter() - t_start_7

    metrics_7 = BenchmarkMetrics.compute(
        "7. Jumbo Frame Streaming (5MB Payload / TCP)", latencies_7, total_time_7, concurrency_7
    )
    metrics_7.print_row()
    results.append(metrics_7)

    await small_driver.close()

    print(
        "=================================================================================================================\n"
    )

    # Save results to json
    with open("benchmarks/python_bridge_results.json", "w", encoding="utf-8") as f:
        json.dump([asdict(m) for m in results], f, indent=2)
    print("[SAVED] Saved python bridge metrics to benchmarks/python_bridge_results.json")


if __name__ == "__main__":
    asyncio.run(run_benchmarks())
