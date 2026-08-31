"""Comprehensive Correctness and Invariant Matrix for Voyager OGM Python SDK."""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    generate_synthetic_stream,
    node,
    relationship,
    reset_alias_counters,
    to_arrow,
    to_polars,
)


@pytest.fixture(autouse=True)
def reset_aliases():
    reset_alias_counters()


@node(label=["Person", "Actor", "Producer"])
class MultiLabelPerson(Node):
    name: str = Field()
    age: int = Field()
    net_worth: float = Field()
    is_active: bool = Field()


@node(label="Movie")
class Movie(Node):
    title: str = Field()
    budget: float = Field()


@relationship(type_name="PRODUCED")
class Produced(Relationship):
    share: float = Field()


def test_invalid_dialect_compilation_raises_value_error():
    p = MultiLabelPerson("p")
    query = Query.match(p).return_(p.name)

    with pytest.raises(ValueError, match="Unsupported query dialect"):
        query.compile("graphql")

    with pytest.raises(ValueError, match="Unsupported query dialect"):
        query.compile("sparql")


def test_multi_label_node_generation_and_compilation():
    p = MultiLabelPerson("p")
    query = Query.match(p).where(p.age >= 30).return_(p.name)

    cypher = query.compile("cypher")
    assert "MATCH (p:Person:Actor:Producer)" in cypher.statement
    assert cypher.parameters == {"p0": 30}

    gql = query.compile("iso_gql")
    assert "MATCH (p:Person&Actor&Producer)" in gql.statement
    assert gql.parameters == {"p0": 30}


def test_deep_20_hop_linear_path_compilation():
    query = Query.match()
    first = MultiLabelPerson()
    query.node(first)

    for _ in range(20):
        m = Movie()
        rel = Produced()
        query.to(rel).node(m)

    query.return_(first.name).limit(10)
    compiled = query.compile("cypher")

    assert compiled.statement.startswith(
        "MATCH (_person_0:Person:Actor:Producer)-[_produced_0:PRODUCED]->(_movie_0:Movie)"
    )
    assert "LIMIT 10" in compiled.statement


def test_all_comparison_operators_correctness():
    p = MultiLabelPerson("p")

    q1 = Query.match(p).where(p.age == 25).return_(p.name).compile("cypher")
    assert "WHERE p.age = $p0" in q1.statement and q1.parameters == {"p0": 25}

    q2 = Query.match(p).where(p.age > 25).return_(p.name).compile("cypher")
    assert "WHERE p.age > $p0" in q2.statement and q2.parameters == {"p0": 25}

    q3 = Query.match(p).where(p.age >= 25).return_(p.name).compile("cypher")
    assert "WHERE p.age >= $p0" in q3.statement and q3.parameters == {"p0": 25}

    q4 = Query.match(p).where(p.age < 25).return_(p.name).compile("cypher")
    assert "WHERE p.age < $p0" in q4.statement and q4.parameters == {"p0": 25}

    q5 = Query.match(p).where(p.age <= 25).return_(p.name).compile("cypher")
    assert "WHERE p.age <= $p0" in q5.statement and q5.parameters == {"p0": 25}

    q6 = Query.match(p).where(p.name.contains("Smith")).return_(p.name).compile("cypher")
    assert "WHERE p.name CONTAINS $p0" in q6.statement and q6.parameters == {"p0": "Smith"}


def test_arrow_and_polars_data_integrity_and_null_checks():
    stream = generate_synthetic_stream(50_000)
    df = to_polars(stream)

    assert df.height == 50_000
    assert df["id"].null_count() == 0
    assert df["name"].null_count() == 0
    assert df["age"].min() == 20
    assert df["age"].max() == 79

    table = to_arrow(stream)
    assert table.num_rows == 50_000
    assert table.column("id").null_count == 0


def test_sql_pgq_multi_hop_compilation_accuracy():
    p = MultiLabelPerson("p")
    m = Movie("m")
    rel = Produced("r")

    query = (
        Query.match(p)
        .to(rel)
        .hops(1, 4)
        .node(m)
        .where(p.age > 40, m.budget >= 1000000.0)
        .return_(p.name, m.title, total_budget=m.budget)
        .order_by(p.name)
        .limit(25)
    )

    compiled = query.compile("sql_pgq", graph_name="hollywood")
    expected = (
        "SELECT * FROM GRAPH_TABLE (hollywood MATCH (p IS Person IS Actor IS Producer) "
        "-[r IS PRODUCED{1,4}]-> (m IS Movie) "
        "WHERE (p.age > $p0) AND (m.budget >= $p1) "
        "COLUMNS (p.name, m.title, m.budget AS total_budget)) "
        "ORDER BY p.name ASC LIMIT 25"
    )
    assert compiled.statement == expected
    assert compiled.parameters == {"p0": 40, "p1": 1000000.0}
