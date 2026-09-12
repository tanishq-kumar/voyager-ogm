"""Bulk ingestion and plan generation benchmarks for Voyager OGM."""

from __future__ import annotations

import polars as pl
from voyager_ogm import Field, Node, Session, node


@node("Person")
class Person(Node):
    id: Field[int] = Field()
    name: Field[str] = Field()
    age: Field[int] = Field()


def test_bench_100k_polars_bulk_plan_generation(benchmark):
    """Benchmark high-speed chunking and plan generation for 100,000 Polars rows."""
    df = pl.DataFrame(
        {
            "id": list(range(100_000)),
            "name": [f"User_{i}" for i in range(100_000)],
            "age": [20 + (i % 40) for i in range(100_000)],
        }
    )
    session = Session(dialect="cypher")

    def run_plan():
        plan = session.bulk_create(Person, df, batch_size=50_000)
        assert plan.num_batches == 2
        return plan

    plan = benchmark(run_plan)
    assert plan.total_records == 100_000
