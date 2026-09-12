"""Large-payload (10MB, 25MB, 50MB) benchmark for official Neo4j Python driver."""

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
class LargePayloadMetric:
    """Telemetry metrics container for large payload benchmark."""

    payload_size_mb: int
    iterations: int
    p50_ms: float
    p90_ms: float
    p99_ms: float
    max_ms: float
    throughput_mb_s: float

    def print_row(self) -> None:
        """Print a formatted Markdown table row."""
        print(
            f"| {self.payload_size_mb:>15} MB | {self.iterations:>6} | {self.p50_ms:>10.2f} ms | "
            f"{self.p90_ms:>10.2f} ms | {self.p99_ms:>10.2f} ms | {self.max_ms:>10.2f} ms | {self.throughput_mb_s:>12.2f} MB/s |"
        )


async def run_jumbo_benchmarks() -> None:
    """Execute jumbo payload stress test with the official Python driver."""
    driver = AsyncGraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH)
    await driver.verify_connectivity()

    print(
        "\n========================================================================================================"
    )
    print("                 OFFICIAL NEO4J PYTHON DRIVER JUMBO PAYLOAD BENCHMARK")
    print(
        "========================================================================================================"
    )
    print(
        f"| {'Payload Size':>18} | {'Runs':>6} | {'p50 Latency':>13} | {'p90 Latency':>13} | {'p99 Latency':>13} | {'Max Latency':>13} | {'Throughput (MB/s)':>17} |"
    )
    print(
        "|--------------------+--------+---------------+---------------+---------------+---------------+-------------------|"
    )

    sizes = [5, 10, 20, 35, 50]
    results = []

    for size_mb in sizes:
        iterations = 5 if size_mb >= 35 else 10
        payload = "X" * (size_mb * 1024 * 1024)
        query = "RETURN $data AS payload, size($data) AS byte_len"

        latencies = []
        for _ in range(iterations):
            t0 = time.perf_counter()
            async with driver.session() as session:
                res = await session.run(query, data=payload)
                records = await res.data()
                assert len(records) == 1
            latencies.append((time.perf_counter() - t0) * 1000.0)

        latencies.sort()
        avg_ms = sum(latencies) / len(latencies)
        throughput = ((size_mb * 2) / (avg_ms / 1000.0)) if avg_ms > 0 else 0.0

        metric = LargePayloadMetric(
            payload_size_mb=size_mb,
            iterations=iterations,
            p50_ms=percentile(latencies, 50.0),
            p90_ms=percentile(latencies, 90.0),
            p99_ms=percentile(latencies, 99.0),
            max_ms=latencies[-1],
            throughput_mb_s=throughput,
        )
        metric.print_row()
        results.append(metric)

    await driver.close()
    print(
        "========================================================================================================\n"
    )

    with open("benchmarks/python_jumbo_results.json", "w", encoding="utf-8") as f:
        json.dump([asdict(m) for m in results], f, indent=2)


if __name__ == "__main__":
    asyncio.run(run_jumbo_benchmarks())
