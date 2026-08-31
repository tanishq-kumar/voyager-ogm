"""Task 2.1: 1,000,000 Node Hydration Microbenchmark (Voyager Arrow/Polars vs Python OOP).

Run with:
    uv run pytest packages/python/tests/test_hydration_benchmark.py --benchmark-only
"""

from __future__ import annotations

from typing import Any

from voyager_ogm import (
    Field,
    Node,
    generate_synthetic_stream,
    node,
    to_arrow,
    to_polars,
)


@node(label="Person")
class Person(Node):
    name: str = Field()
    age: int = Field()
    score: float = Field()
    active: bool = Field()


# ============================================================
# Task 2.1 Hydration Benchmarks
# ============================================================
def test_bench_100k_nodes_to_polars(benchmark: Any) -> None:
    """Benchmark zero-copy ingestion of 100,000 graph nodes into Polars DataFrame."""
    stream = generate_synthetic_stream(100_000)

    def run():
        df = to_polars(stream)
        return df.height

    height = benchmark(run)
    assert height == 100_000


def test_bench_1m_nodes_to_polars(benchmark: Any) -> None:
    """Benchmark zero-copy ingestion of 1,000,000 graph nodes into Polars DataFrame."""
    stream = generate_synthetic_stream(1_000_000)

    def run():
        df = to_polars(stream)
        return df.height

    height = benchmark(run)
    assert height == 1_000_000


def test_bench_1m_nodes_to_arrow(benchmark: Any) -> None:
    """Benchmark zero-copy ingestion of 1,000,000 graph nodes into PyArrow Table."""
    stream = generate_synthetic_stream(1_000_000)

    def run():
        tbl = to_arrow(stream)
        return tbl.num_rows

    num_rows = benchmark(run)
    assert num_rows == 1_000_000


def test_bench_10k_nodes_pure_python_objects(benchmark: Any) -> None:
    """Simulate Neomodel / GQLAlchemy object hydration cliff (10,000 objects)."""

    def run():
        objects = []
        for i in range(10_000):
            p = Person(alias=f"p_{i}")
            p._values = {
                "id": i,
                "name": f"Person_{i}",
                "age": 20 + (i % 60),
                "score": 75.5,
                "active": i % 2 == 0,
            }
            objects.append(p)
        return len(objects)

    count = benchmark(run)
    assert count == 10_000
