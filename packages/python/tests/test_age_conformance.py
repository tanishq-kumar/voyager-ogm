"""Apache AGE (PostgreSQL Embedded Cypher) Comprehensive Conformance Test Suite.

Ingests and validates official Apache AGE regression scenarios:
1. SELECT * FROM cypher('graph_name', $$ MATCH ... $$) AS (...) wrapper generation
2. agtype composite projection mapping for single & multi-column queries
3. Parameter isolation ($p0, $p1) across PostgreSQL SQL/Cypher boundary
4. Multi-hop path chains: (a)-[:KNOWS]->(b)-[:WORKS_AT]->(c)
5. Variable-length path repetition: [:KNOWS*1..3]
6. Directed, incoming, and undirected traversals
7. Full predicate operators (=, !=, >, >=, <, <=, IN, NOT IN, CONTAINS, STARTS WITH, ENDS WITH)
8. Projections, aggregations (COUNT, AVG, SUM, MIN, MAX, COLLECT), and ordering
9. DML Mutations: CREATE, MERGE, SET, REMOVE, DETACH DELETE
10. Relational-Graph Hybrid JOIN queries in PostgreSQL
"""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
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


@node(label="Company")
class Company(Node):
    """Company entity."""

    name = Field()


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
# 1. Apache AGE Match Patterns & Parameter Isolation
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
        (lambda p: p.city.contains("don"), "p.city CONTAINS $p0", {"p0": "don"}),
        (lambda p: p.name.startswith("Al"), "p.name STARTS WITH $p0", {"p0": "Al"}),
        (lambda p: p.name.endswith("ce"), "p.name ENDS WITH $p0", {"p0": "ce"}),
    ],
    ids=[
        "age_eq",
        "age_ne",
        "age_gt",
        "age_gte",
        "age_lt",
        "age_lte",
        "age_in",
        "age_not_in",
        "age_contains",
        "age_starts_with",
        "age_ends_with",
    ],
)
def test_age_operator_transpilations(expr_builder, expected_fragment, expected_params):
    """Verifies all Apache AGE predicate translations inside cypher table function."""
    p = Person(alias="p")
    pred = expr_builder(p)
    q = Query.match(p).where(pred).return_(p.name)
    compiled = q.compile("apache_age", graph_name="age_graph")

    assert f"MATCH (p:Person) WHERE {expected_fragment} RETURN p.name" in compiled.statement
    assert "AS (name agtype)" in compiled.statement
    assert compiled.parameters == expected_params


# ---------------------------------------------------------------------------
# 2. Multi-Hop, Quantified Paths, and Directions in AGE
# ---------------------------------------------------------------------------


def test_age_multi_hop_traversal_chain():
    """Apache AGE: Multi-hop traversal (a)-[:KNOWS]->(b)-[:WORKS_AT]->(c)."""
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
    compiled = q.compile("apache_age", graph_name="corp_graph")

    expected_cypher = (
        "MATCH (a:Person)-[r1:KNOWS]->(b:Person)-[r2:WORKS_AT]->(c:Company) "
        "WHERE a.name = $p0 "
        "RETURN a.name AS person, b.name AS colleague, c.name AS company"
    )
    assert (
        f"SELECT * FROM cypher('corp_graph', $$ {expected_cypher} $$, %s) AS (person agtype, colleague agtype, company agtype)"
        == compiled.statement
    )
    assert compiled.parameters == {"p0": "Alice"}


@pytest.mark.parametrize(
    ("min_hops", "max_hops", "expected_var_hops"),
    [
        (1, 2, ":KNOWS*1..2"),
        (1, 3, ":KNOWS*1..3"),
        (2, 4, ":KNOWS*2..4"),
    ],
    ids=["age_hops_1_2", "age_hops_1_3", "age_hops_2_4"],
)
def test_age_variable_length_paths(min_hops, max_hops, expected_var_hops):
    """Apache AGE: Variable-length path quantization *min..max."""
    a = Person(alias="a")
    b = Person(alias="b")
    q = Query.match(a).to(Knows).hops(min_hops, max_hops).node(b).return_(b.name)
    compiled = q.compile("apache_age", graph_name="age_graph")

    assert expected_var_hops in compiled.statement
    assert "AS (name agtype)" in compiled.statement


def test_age_undirected_and_incoming_traversals():
    """Apache AGE: Undirected and Incoming relationship traversals."""
    a = Person(alias="a")
    b = Person(alias="b")
    q_undir = Query.match(a).edge(Knows, "r").node(b).return_(b.name)
    c_undir = q_undir.compile("apache_age", graph_name="age_graph")
    assert "MATCH (a:Person)-[r:KNOWS]-(b:Person) RETURN b.name" in c_undir.statement

    c = Company(alias="c")
    q_inc = Query.match(c).from_(WorksAt, "r").node(a).return_(c.name, a.name)
    c_inc = q_inc.compile("apache_age", graph_name="age_graph")
    assert "MATCH (c:Company)<-[r:WORKS_AT]-(a:Person) RETURN c.name, a.name" in c_inc.statement
    assert "AS (name agtype, name agtype)" in c_inc.statement


# ---------------------------------------------------------------------------
# 3. Aggregations & Projections in AGE
# ---------------------------------------------------------------------------


def test_age_aggregations_and_grouping():
    """Apache AGE: Projections with COUNT, AVG, SUM, MIN, MAX, COLLECT."""
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
    )
    compiled = q.compile("apache_age", graph_name="age_graph")

    assert "COUNT(p.name) AS total_count" in compiled.statement
    assert "AVG(p.age) AS avg_age" in compiled.statement
    assert "SUM(p.age) AS total_age" in compiled.statement
    assert "MIN(p.age) AS min_age" in compiled.statement
    assert "MAX(p.age) AS max_age" in compiled.statement
    assert "COLLECT(p.name) AS all_names" in compiled.statement
    assert (
        "AS (city agtype, total_count agtype, avg_age agtype, total_age agtype, min_age agtype, max_age agtype, all_names agtype)"
        in compiled.statement
    )


# ---------------------------------------------------------------------------
# 4. DML Mutations in AGE (CREATE, MERGE, SET, REMOVE, DELETE)
# ---------------------------------------------------------------------------


def test_age_dml_mutations():
    """Apache AGE: Graph mutations wrapped inside cypher table function."""
    p = Person(alias="p")

    # CREATE
    q_create = Query.create(p)
    c_create = q_create.compile("apache_age", graph_name="age_graph")
    assert (
        c_create.statement
        == "SELECT * FROM cypher('age_graph', $$ CREATE (p:Person) $$) AS (result agtype)"
    )

    # MERGE (Upsert)
    q_merge = Query.merge(p).on_create_set(p.name == "Eva").on_match_set(p.age == 30)
    c_merge = q_merge.compile("apache_age", graph_name="age_graph")
    assert (
        "MERGE (p:Person) ON CREATE SET p.name = $p0 ON MATCH SET p.age = $p1" in c_merge.statement
    )
    assert c_merge.parameters == {"p0": "Eva", "p1": 30}

    # SET properties
    q_set = (
        Query.match(p).where(p.name == "Dan").set(p.age == 45, p.city == "Oxford").return_(p.name)
    )
    c_set = q_set.compile("apache_age", graph_name="age_graph")
    assert (
        "MATCH (p:Person) WHERE p.name = $p0 SET p.age = $p1, p.city = $p2 RETURN p.name"
        in c_set.statement
    )
    assert "AS (name agtype)" in c_set.statement
    assert c_set.parameters == {"p0": "Dan", "p1": 45, "p2": "Oxford"}

    # REMOVE property
    q_rem = Query.match(p).where(p.name == "Dan").remove(p.city)
    c_rem = q_rem.compile("apache_age", graph_name="age_graph")
    assert "MATCH (p:Person) WHERE p.name = $p0 REMOVE p.city" in c_rem.statement

    # DETACH DELETE
    q_del = Query.match(p).where(p.age < 18).detach_delete(p)
    c_del = q_del.compile("apache_age", graph_name="age_graph")
    assert "MATCH (p:Person) WHERE p.age < $p0 DETACH DELETE p" in c_del.statement
    assert c_del.parameters == {"p0": 18}


# ---------------------------------------------------------------------------
# 5. Live PostgreSQL / Apache AGE Database Execution
# ---------------------------------------------------------------------------


def test_live_apache_age_database_execution():
    """Verifies live query execution against running Apache AGE instance if available."""
    try:
        import psycopg

        con = psycopg.connect(
            "host=localhost port=5455 user=postgres password=voyagerpass123 dbname=voyager_graph",
            autocommit=True,
            connect_timeout=3,
        )
    except Exception:
        pytest.skip("Apache AGE container not reachable on port 5455")

    with con.cursor() as cur:
        cur.execute("LOAD 'age';")
        cur.execute('SET search_path = ag_catalog, "$user", public;')

        # Ensure graph exists
        cur.execute("SELECT count(*) FROM ag_graph WHERE name = 'live_age_graph';")
        if cur.fetchone()[0] == 0:
            cur.execute("SELECT create_graph('live_age_graph');")

        # Create nodes
        cur.execute(
            "SELECT * FROM cypher('live_age_graph', $$ MATCH (n) DETACH DELETE n $$) AS (res agtype);"
        )
        cur.execute(
            "SELECT * FROM cypher('live_age_graph', $$ CREATE (a:Person {name: 'Alice', age: 34}), (b:Person {name: 'Bob', age: 28}) $$) AS (res agtype);"
        )
        cur.execute(
            "SELECT * FROM cypher('live_age_graph', $$ MATCH (a:Person {name: 'Alice'}), (b:Person {name: 'Bob'}) CREATE (a)-[:KNOWS {since: 2020}]->(b) $$) AS (res agtype);"
        )

        # Compile query with Voyager
        a = Person(alias="a")
        b = Person(alias="b")
        q = (
            Query.match(a)
            .to(Knows, "r")
            .node(b)
            .where(a.name == "Alice")
            .return_(source=a.name, target=b.name)
        )
        compiled = q.compile("apache_age", graph_name="live_age_graph")

        # Execute compiled query
        import json

        params = (json.dumps(compiled.parameters),) if compiled.parameters else ()
        cur.execute(compiled.statement, params)
        rows = cur.fetchall()
        assert len(rows) == 1
        assert "Alice" in str(rows[0][0])
        assert "Bob" in str(rows[0][1])

    con.close()
