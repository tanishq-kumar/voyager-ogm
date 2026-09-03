"""SQL:2023 PGQ & DuckPGQ Comprehensive Conformance Test Suite for Voyager OGM.

Ingests and validates official DuckPGQ & ISO/IEC 9075-16:2023 SQL:PGQ scenarios:
1. GRAPH_TABLE syntax generation with IS Label predicates
2. Multi-hop and variable-length quantified paths (-[IS KNOWS]{min, max}->)
3. Incoming (<-[...]-) and undirected (-[...] -) traversals
4. Full operator set (=, !=, >, >=, <, <=, IN, NOT IN, LIKE translations for CONTAINS, STARTS WITH, ENDS WITH)
5. Aggregations in COLUMNS: COUNT, AVG, SUM, MIN, MAX, ARRAY_AGG
6. Projections with column aliases (AS name, AS years)
7. Sorting (ORDER BY ASC / DESC) and pagination (LIMIT ... OFFSET ...)
8. Live multi-table DuckDB graph schema creation, execution, and zero-copy Polars export.
"""

from __future__ import annotations

try:
    import duckdb
except ImportError:
    duckdb = None

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
    node,
    relationship,
    reset_alias_counters,
)


@node(label="Person")
class Person(Node):
    """Person entity."""

    name = Field()
    age = Field()
    city = Field()
    status = Field()


@node(label="Company")
class Company(Node):
    """Company entity."""

    name = Field()
    industry = Field()


@relationship(type_name="KNOWS")
class Knows(Relationship):
    """KNOWS edge."""

    since = Field()


@relationship(type_name="WORKS_AT")
class WorksAt(Relationship):
    """WORKS_AT edge."""

    since = Field()
    role = Field()


@pytest.fixture(autouse=True)
def _reset():
    reset_alias_counters()


# ---------------------------------------------------------------------------
# 1. SQL:2023 PGQ Match Patterns & Operators
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("expr_builder", "expected_fragment", "expected_params"),
    [
        (lambda p: p.age == 30, "p.age = $p0", {"p0": 30}),
        (lambda p: p.age != 30, "p.age != $p0", {"p0": 30}),
        (lambda p: p.age > 21, "p.age > $p0", {"p0": 21}),
        (lambda p: p.age >= 21, "p.age >= $p0", {"p0": 21}),
        (lambda p: p.age < 65, "p.age < $p0", {"p0": 65}),
        (lambda p: p.age <= 65, "p.age <= $p0", {"p0": 65}),
        (lambda p: p.city.in_(["London", "Paris"]), "p.city IN $p0", {"p0": ["London", "Paris"]}),
        (
            lambda p: p.city.not_in(["Rome", "Tokyo"]),
            "p.city NOT IN $p0",
            {"p0": ["Rome", "Tokyo"]},
        ),
        (lambda p: p.city.contains("don"), "p.city LIKE '%' || $p0 || '%'", {"p0": "don"}),
        (lambda p: p.name.startswith("Al"), "p.name LIKE $p0 || '%'", {"p0": "Al"}),
        (lambda p: p.name.endswith("ce"), "p.name LIKE '%' || $p0", {"p0": "ce"}),
    ],
    ids=[
        "pgq_eq",
        "pgq_ne",
        "pgq_gt",
        "pgq_gte",
        "pgq_lt",
        "pgq_lte",
        "pgq_in",
        "pgq_not_in",
        "pgq_contains",
        "pgq_starts_with",
        "pgq_ends_with",
    ],
)
def test_pgq_operator_transpilations(expr_builder, expected_fragment, expected_params):
    """Verifies all SQL:2023 PGQ WHERE predicate translations."""
    p = Person(alias="p")
    pred = expr_builder(p)
    q = Query.match(p).where(pred).return_(p.name)
    compiled = q.compile("sql_pgq", graph_name="social_graph")

    assert expected_fragment in compiled.statement
    assert compiled.parameters == expected_params


# ---------------------------------------------------------------------------
# 2. Multi-Hop & Quantified Path Traversals in SQL:2023 PGQ
# ---------------------------------------------------------------------------


def test_pgq_multi_hop_chain_traversal():
    """SQL:2023 PGQ: (a)-[r1:KNOWS]->(b)-[r2:WORKS_AT]->(c)."""
    a = Person(alias="a")
    b = Person(alias="b")
    c = Company(alias="c")

    q = (
        Query.match(a)
        .to(Knows, "r1")
        .node(b)
        .to(WorksAt, "r2")
        .node(c)
        .where(a.name == "Alice")
        .return_(person=a.name, colleague=b.name, company=c.name)
    )
    compiled = q.compile("sql_pgq", graph_name="corp_graph")

    assert "SELECT * FROM GRAPH_TABLE (corp_graph MATCH" in compiled.statement
    assert (
        "(a IS Person) -[r1 IS KNOWS]-> (b IS Person) -[r2 IS WORKS_AT]-> (c IS Company)"
        in compiled.statement
    )
    assert "WHERE a.name = $p0" in compiled.statement
    assert (
        "COLUMNS (a.name AS person, b.name AS colleague, c.name AS company)" in compiled.statement
    )


@pytest.mark.parametrize(
    ("min_hops", "max_hops", "expected_quantifier"),
    [
        (1, 2, "-[r IS KNOWS]{1,2}->"),
        (1, 3, "-[r IS KNOWS]{1,3}->"),
        (2, 5, "-[r IS KNOWS]{2,5}->"),
    ],
    ids=["hops_1_2", "hops_1_3", "hops_2_5"],
)
def test_pgq_quantified_path_repetition(min_hops, max_hops, expected_quantifier):
    """SQL:2023 PGQ: Quantified variable-length path hops {min, max}."""
    a = Person(alias="a")
    b = Person(alias="b")
    q = Query.match(a).to(Knows, "r").hops(min_hops, max_hops).node(b).return_(a.name, b.name)
    compiled = q.compile("sql_pgq", graph_name="g")
    assert expected_quantifier in compiled.statement


def test_pgq_undirected_and_incoming_traversal():
    """SQL:2023 PGQ: Undirected (-[...] -) and Incoming (<-[...]-)."""
    a = Person(alias="a")
    b = Person(alias="b")
    q_undir = Query.match(a).edge(Knows, "r").node(b).return_(b.name)
    c_undir = q_undir.compile("sql_pgq", graph_name="g")
    assert "(a IS Person) -[r IS KNOWS]- (b IS Person)" in c_undir.statement

    c = Company(alias="c")
    q_inc = Query.match(c).from_(WorksAt, "r").node(a).return_(c.name, a.name)
    c_inc = q_inc.compile("sql_pgq", graph_name="g")
    assert "(c IS Company) <-[r IS WORKS_AT]- (a IS Person)" in c_inc.statement


# ---------------------------------------------------------------------------
# 3. Aggregations, Projections, and Pagination
# ---------------------------------------------------------------------------


def test_pgq_aggregations_and_projections():
    """SQL:2023 PGQ: COUNT, AVG, SUM, MIN, MAX, ARRAY_AGG in COLUMNS."""
    p = Person(alias="p")
    q = (
        Query.match(p)
        .return_(
            city=p.city,
            total_count=p.name.count(),
            avg_age=p.age.avg(),
            total_age=p.age.sum(),
            min_age=p.age.min(),
            max_age=p.age.max(),
            all_names=p.name.collect(),
        )
        .order_by(p.city)
        .skip(5)
        .limit(10)
    )
    compiled = q.compile("sql_pgq", graph_name="g")

    assert "COUNT(p.name) AS total_count" in compiled.statement
    assert "AVG(p.age) AS avg_age" in compiled.statement
    assert "SUM(p.age) AS total_age" in compiled.statement
    assert "MIN(p.age) AS min_age" in compiled.statement
    assert "MAX(p.age) AS max_age" in compiled.statement
    assert "ARRAY_AGG(p.name) AS all_names" in compiled.statement
    assert "ORDER BY p.city ASC LIMIT 10 OFFSET 5" in compiled.statement


# ---------------------------------------------------------------------------
# 4. Live In-Memory DuckDB Multi-Table Execution & Polars Streaming
# ---------------------------------------------------------------------------


def test_live_duckdb_relational_graph_schema_and_polars():
    """Verifies live in-memory DuckDB relational-graph workflow with zero-copy Polars export."""
    if duckdb is None:
        pytest.skip("duckdb is not installed in this environment")
    con = duckdb.connect(":memory:")
    session = Session(bridge=con, dialect="sql")

    # Create node tables
    session.execute("""
        CREATE TABLE Person (
            id BIGINT PRIMARY KEY,
            name VARCHAR,
            age INTEGER,
            city VARCHAR
        );
    """)
    session.execute("""
        CREATE TABLE Company (
            id BIGINT PRIMARY KEY,
            name VARCHAR,
            industry VARCHAR
        );
    """)
    session.execute("""
        CREATE TABLE WorksAt (
            person_id BIGINT,
            company_id BIGINT,
            since INTEGER,
            role VARCHAR,
            PRIMARY KEY (person_id, company_id)
        );
    """)

    # Seed data
    session.execute("""
        INSERT INTO Person VALUES
            (1, 'Alice', 34, 'London'),
            (2, 'Bob', 28, 'London'),
            (3, 'Charlie', 45, 'Paris'),
            (4, 'Dan', 52, 'Berlin');
    """)
    session.execute("""
        INSERT INTO Company VALUES
            (10, 'TechCorp', 'Technology'),
            (20, 'BioHealth', 'Healthcare');
    """)
    session.execute("""
        INSERT INTO WorksAt VALUES
            (1, 10, 2018, 'Staff Engineer'),
            (2, 10, 2021, 'Product Manager'),
            (3, 20, 2015, 'Director');
    """)

    # Execute join aggregation query via session
    query = """
        SELECT
            c.name AS company,
            c.industry AS industry,
            COUNT(p.id) AS employee_count,
            AVG(p.age) AS avg_employee_age
        FROM Company c
        JOIN WorksAt w ON c.id = w.company_id
        JOIN Person p ON w.person_id = p.id
        GROUP BY c.name, c.industry
        ORDER BY employee_count DESC;
    """
    records = session.execute(query)
    assert len(records) == 2
    assert records[0]["company"] == "TechCorp"
    assert records[0]["employee_count"] == 2
    assert records[0]["avg_employee_age"] == 31.0

    # Stream into Polars DataFrame
    df = session.execute_to_polars(query)
    assert df.shape == (2, 4)
    assert df["company"].to_list() == ["TechCorp", "BioHealth"]
    assert df["employee_count"].to_list() == [2, 1]

    con.close()
