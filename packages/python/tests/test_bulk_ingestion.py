"""Unit and integration tests for bulk ingestion engine and Session APIs."""

from __future__ import annotations

import polars as pl
import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
    chunk_dataframe,
    chunk_records,
    node,
    relationship,
    reset_alias_counters,
)


@node("Person")
class Person(Node):
    id: Field[int] = Field()
    name: Field[str] = Field()
    age: Field[int] = Field()


@relationship("FOLLOWS")
class Follows(Relationship):
    since: Field[int] = Field()


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


def test_fluent_unwind_query():
    """Test manual UNWIND query assembly using Query builder."""
    compiled = (
        Query.unwind("batch", alias="row")
        .add_create(Person)
        .set(Person.name == 26)
        .compile("cypher")
    )

    assert compiled.statement.startswith("UNWIND $batch AS row CREATE (_person_0:Person)")


def test_chunk_records_helper():
    """Test pure python record list chunking."""
    records = [{"id": i, "name": f"User_{i}"} for i in range(105)]
    batches = list(chunk_records(records, batch_size=50))

    assert len(batches) == 3
    assert len(batches[0]) == 50
    assert len(batches[1]) == 50
    assert len(batches[2]) == 5


def test_chunk_polars_dataframe():
    """Test zero-copy chunking directly from a Polars DataFrame."""
    df = pl.DataFrame(
        {
            "id": list(range(120)),
            "name": [f"Person_{i}" for i in range(120)],
            "age": [20 + (i % 50) for i in range(120)],
        }
    )

    batches = list(chunk_dataframe(df, batch_size=50))
    assert len(batches) == 3
    assert len(batches[0]) == 50
    assert len(batches[1]) == 50
    assert len(batches[2]) == 20
    assert batches[0][0]["name"] == "Person_0"
    assert batches[2][-1]["name"] == "Person_119"


def test_session_bulk_create_from_list():
    """Test session.bulk_create generating bulk execution plan from list of dicts."""
    session = Session(dialect="cypher")
    data = [{"id": i, "name": f"User_{i}", "age": 25} for i in range(10_000)]

    plan = session.bulk_create(Person, data, batch_size=2_500)

    assert plan.total_records == 10_000
    assert plan.num_batches == 4
    assert plan.statement == (
        "UNWIND $batch AS row CREATE (_person_0:Person) "
        "SET _person_0.id = row.id, _person_0.name = row.name, _person_0.age = row.age"
    )

    batches = list(plan)
    assert len(batches) == 4
    assert batches[0].batch_index == 0
    assert len(batches[0].parameters["batch"]) == 2_500


def test_session_bulk_create_from_polars():
    """Test session.bulk_create from a large 100,000-row Polars DataFrame."""
    df = pl.DataFrame(
        {
            "id": list(range(100_000)),
            "name": [f"User_{i}" for i in range(100_000)],
            "age": [20 + (i % 40) for i in range(100_000)],
        }
    )

    session = Session(dialect="cypher")
    plan = session.bulk_create(Person, df, batch_size=50_000)

    assert plan.total_records == 100_000
    assert plan.num_batches == 2
    assert plan.statement == (
        "UNWIND $batch AS row CREATE (_person_0:Person) "
        "SET _person_0.id = row.id, _person_0.name = row.name, _person_0.age = row.age"
    )


def test_session_bulk_upsert_merge():
    """Test session.bulk_upsert generating idempotent MERGE statement."""
    session = Session(dialect="cypher")
    data = [{"id": 1, "name": "Alice", "age": 30}, {"id": 2, "name": "Bob", "age": 35}]

    plan = session.bulk_upsert(Person, data, key_field="id", batch_size=50_000)

    assert plan.statement == (
        "UNWIND $batch AS row MERGE (_person_0:Person {_person_0.id = row.id}) "
        "ON CREATE SET _person_0.name = row.name, _person_0.age = row.age "
        "ON MATCH SET _person_0.name = row.name, _person_0.age = row.age"
    )
    assert plan.total_records == 2


def test_session_bulk_create_relationships():
    """Test session.bulk_create_relationships generating relationship matching & creation."""
    session = Session(dialect="cypher")
    edges_df = pl.DataFrame(
        {
            "from_id": [1, 2, 3],
            "to_id": [2, 3, 4],
            "since": [2020, 2021, 2022],
        }
    )

    plan = session.bulk_create_relationships(
        Follows,
        edges_df,
        from_label="Person",
        from_key="id",
        to_label="Person",
        to_key="id",
        batch_size=50_000,
    )

    assert plan.statement == (
        "UNWIND $batch AS row "
        "MATCH (_from_person_0:Person {_from_person_0.id = row.from_id}), "
        "(_to_person_0:Person {_to_person_0.id = row.to_id}) "
        "CREATE (_from_person_0)-[_follows_0:FOLLOWS]->(_to_person_0) "
        "SET _follows_0.since = row.since"
    )
    assert plan.total_records == 3


def test_session_bulk_create_iso_gql_dialect():
    """Test bulk ingestion plan with ISO GQL dialect."""
    session = Session(dialect="iso_gql")
    data = [{"id": 1, "name": "Alice", "age": 28}]

    plan = session.bulk_create(Person, data, batch_size=1_000)

    assert plan.statement == (
        "UNWIND $batch AS row INSERT (_person_0:Person) "
        "SET _person_0.id = row.id, _person_0.name = row.name, _person_0.age = row.age"
    )


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
