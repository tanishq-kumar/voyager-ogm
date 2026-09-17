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
    fn,
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
        (1, 2, "-[r IS KNOWS]->{1,2}"),
        (1, 3, "-[r IS KNOWS]->{1,3}"),
        (2, 5, "-[r IS KNOWS]->{2,5}"),
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


def test_pgq_scalar_function_mappings():
    """SQL:2023 PGQ: Function mappings to standard ANSI SQL functions."""
    p = Person(alias="p")
    q = Query.match(p).return_(
        lower_name=fn.to_lower(p.name),
        upper_city=fn.to_upper(p.city),
        split_city=fn.split(p.city, "-"),
    )
    compiled = q.compile("sql_pgq", graph_name="g")
    assert "LOWER(p.name) AS lower_name" in compiled.statement
    assert "UPPER(p.city) AS upper_city" in compiled.statement
    assert "STRING_SPLIT(p.city, $p0) AS split_city" in compiled.statement


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
    """SQL:2023 PGQ: COUNT, AVG, SUM, MIN, MAX, ARRAY_AGG in grouped queries."""
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

    assert "SELECT city, COUNT(name) AS total_count" in compiled.statement
    assert "AVG(age) AS avg_age" in compiled.statement
    assert "SUM(age) AS total_age" in compiled.statement
    assert "MIN(age) AS min_age" in compiled.statement
    assert "MAX(age) AS max_age" in compiled.statement
    assert "ARRAY_AGG(name) AS all_names" in compiled.statement
    assert "FROM GRAPH_TABLE (g MATCH (p IS Person)" in compiled.statement
    assert "COLUMNS (p.city AS city, p.name AS name, p.age AS age))" in compiled.statement
    assert "GROUP BY city" in compiled.statement
    assert "ORDER BY city ASC LIMIT 10 OFFSET 5" in compiled.statement


# ---------------------------------------------------------------------------
# 4. Live In-Memory DuckDB Multi-Table Execution & Polars Streaming
# ---------------------------------------------------------------------------


def test_live_duckdb_relational_graph_schema_and_polars():
    """Verifies live in-memory DuckDB + DuckPGQ execution and zero-copy Polars export."""
    if duckdb is None:
        pytest.skip("duckdb is not installed in this environment")
    con = duckdb.connect(":memory:", config={"allow_unsigned_extensions": "true"})
    try:
        con.execute("LOAD duckpgq;")
    except Exception as exc:
        pytest.skip(f"duckpgq extension could not be loaded: {exc}")

    session = Session(bridge=con, dialect="sql_pgq")

    # Create node and edge tables
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
    session.execute("""
        CREATE TABLE Knows (
            src BIGINT,
            dst BIGINT,
            since INTEGER,
            PRIMARY KEY (src, dst)
        );
    """)

    # Seed data
    session.execute("""
        INSERT INTO Person VALUES
            (1, 'Alice', 34, 'London'),
            (2, 'Bob', 28, 'Berlin'),
            (3, 'Charlie', 45, 'London'),
            (4, 'Dan', 52, 'Paris');
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
    session.execute("""
        INSERT INTO Knows VALUES
            (1, 2, 2020),
            (2, 3, 2021),
            (3, 4, 2022);
    """)

    # Create DuckPGQ Property Graph
    session.execute("""
        CREATE PROPERTY GRAPH corp_graph
        VERTEX TABLES (
            Person LABEL Person,
            Company LABEL Company
        )
        EDGE TABLES (
            Knows SOURCE KEY (src) REFERENCES Person (id)
                  DESTINATION KEY (dst) REFERENCES Person (id)
                  LABEL KNOWS,
            WorksAt SOURCE KEY (person_id) REFERENCES Person (id)
                    DESTINATION KEY (company_id) REFERENCES Company (id)
                    LABEL WORKS_AT
        );
    """)

    p = Person(alias="p")
    p2 = Person(alias="p2")
    c = Company(alias="c")

    # Scenario 1: Single Node Match + WHERE Condition + Scalar Functions
    q_single = (
        Query.match(p)
        .where(p.age >= 30)
        .return_(lower_name=fn.to_lower(p.name), upper_city=fn.to_upper(p.city), age=p.age)
        .order_by(p.age)
    )
    single_records = session.execute(q_single.compile("sql_pgq", graph_name="corp_graph")).all()
    assert len(single_records) == 3
    assert single_records[0]["lower_name"] == "alice"
    assert single_records[0]["upper_city"] == "LONDON"

    # Scenario 2: Directed Multi-Hop Traversal Chain (Person -> Knows -> Person -> WorksAt -> Company)
    q_chain = (
        Query.match(p)
        .to(Knows, "r1")
        .node(p2)
        .to(WorksAt, "r2")
        .node(c)
        .return_(start_person=p.name, colleague=p2.name, company=c.name)
        .order_by(p.name)
    )
    chain_records = session.execute(q_chain.compile("sql_pgq", graph_name="corp_graph")).all()
    assert len(chain_records) == 2
    assert chain_records[0]["start_person"] == "Alice"
    assert chain_records[0]["colleague"] == "Bob"
    assert chain_records[0]["company"] == "TechCorp"

    # Scenario 3: Incoming Traversal (Company <- WorksAt - Person)
    q_inc = (
        Query.match(c)
        .from_(WorksAt, "r")
        .node(p)
        .return_(company=c.name, person=p.name)
        .order_by(p.name)
    )
    inc_records = session.execute(q_inc.compile("sql_pgq", graph_name="corp_graph")).all()
    assert len(inc_records) == 3
    assert inc_records[0]["person"] == "Alice"
    assert inc_records[0]["company"] == "TechCorp"

    # Scenario 4: Undirected Traversal (Person - Knows - Person)
    q_undir = Query.match(p).edge(Knows, "r").node(p2).return_(p1=p.name, p2=p2.name)
    undir_records = session.execute(q_undir.compile("sql_pgq", graph_name="corp_graph")).all()
    assert len(undir_records) == 6

    # Scenario 5: Variable-length quantifier repetition ->{1,2}
    q_hops = (
        Query.match(p)
        .to(Knows, "r")
        .hops(1, 2)
        .node(p2)
        .return_(source=p.name, reachable=p2.name)
        .order_by(p.name)
    )
    hops_records = session.execute(q_hops.compile("sql_pgq", graph_name="corp_graph")).all()
    assert len(hops_records) == 5

    # Scenario 6: Grouped Aggregations (COUNT, AVG, MIN, MAX, SUM) + Pagination (LIMIT/OFFSET)
    q_agg = (
        Query.match(p)
        .to(WorksAt, "r")
        .node(c)
        .return_(
            company=c.name,
            employee_count=p.name.count(),
            avg_employee_age=p.age.avg(),
            min_age=p.age.min(),
            max_age=p.age.max(),
            sum_age=p.age.sum(),
        )
        .order_by(c.name)
        .limit(10)
    )
    compiled = q_agg.compile("sql_pgq", graph_name="corp_graph")
    assert (
        "FROM GRAPH_TABLE (corp_graph MATCH (p IS Person) -[r IS WORKS_AT]-> (c IS Company)"
        in compiled.statement
    )

    records = session.execute(compiled).all()
    assert len(records) == 2
    assert records[0]["company"] == "BioHealth"
    assert records[0]["employee_count"] == 1
    assert records[0]["avg_employee_age"] == 45.0
    assert records[0]["min_age"] == 45
    assert records[0]["max_age"] == 45
    assert records[1]["company"] == "TechCorp"
    assert records[1]["employee_count"] == 2
    assert records[1]["avg_employee_age"] == 31.0
    assert records[1]["min_age"] == 28
    assert records[1]["max_age"] == 34

    # Scenario 7: Zero-Copy Polars Streaming
    df = session.execute_to_polars(compiled)
    assert df.shape == (2, 6)
    assert df["company"].to_list() == ["BioHealth", "TechCorp"]
    assert df["employee_count"].to_list() == [1, 2]
    assert df["avg_employee_age"].to_list() == [45.0, 31.0]
    assert df["min_age"].to_list() == [45, 28]
    assert df["max_age"].to_list() == [45, 34]

    con.close()
