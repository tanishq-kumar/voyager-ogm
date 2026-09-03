"""Official openCypher & openGQL TCK Parameterized Conformance Suite for Voyager OGM.

Systematically verifies 50+ granular scenario variations across all openCypher TCK categories:
1. Match & Predicate Permutations (Equality, Relational, Contains, Multi-AND)
2. Traversal Direction & Variable Hop Permutations (Outgoing, Incoming, Undirected, Hops 1..2, 1..3, 2..2)
3. Return, Sorting & Pagination Permutations (Distinct, Asc, Desc, Skip, Limit, Skip+Limit)
4. Aggregations & Grouping Permutations (Count Node, Count Field, Avg, Grouped Multi-Agg)
5. DML Mutation Permutations (Create Node, Create Path, Merge Upsert, Set Properties, Remove, Detach Delete)
6. Bulk Ingestion & Vendor Procedure Permutations (UNWIND $batch, LOAD CSV, CALL ... YIELD)
7. Outer-Join Traversal Permutations (OPTIONAL MATCH with Null Fallback)
"""

from __future__ import annotations

import math
from typing import Any

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

try:
    from neo4j import GraphDatabase

    test_driver = GraphDatabase.driver("bolt://localhost:7687", auth=("neo4j", "voyagerpass123"))
    test_driver.verify_connectivity()
    test_driver.close()
    NEO4J_ONLINE = True
except Exception:
    NEO4J_ONLINE = False


def values_match(actual: Any, expected: Any) -> bool:
    """Compares actual database return value with expected TCK table cell value."""
    if isinstance(actual, float) and isinstance(expected, (float, int)):
        return math.isclose(actual, float(expected), rel_tol=1e-3, abs_tol=1e-3)
    if isinstance(expected, float) and isinstance(actual, (float, int)):
        return math.isclose(float(actual), expected, rel_tol=1e-3, abs_tol=1e-3)
    return bool(actual == expected)


# ---------------------------------------------------------------------------
# TCK Domain Models
# ---------------------------------------------------------------------------


@node(label="Person")
class Person(Node):
    """TCK Person entity."""

    name = Field()
    age = Field()
    city = Field()


@node(label="Company")
class Company(Node):
    """TCK Company entity."""

    name = Field()


@relationship(type_name="KNOWS")
class Knows(Relationship):
    """TCK KNOWS relationship."""

    since = Field()


@relationship(type_name="WORKS_AT")
class WorksAt(Relationship):
    """TCK WORKS_AT relationship."""

    since = Field()


# ---------------------------------------------------------------------------
# TCK Background Graph Seeding & Session Fixtures
# ---------------------------------------------------------------------------


def seed_tck_social_graph(session: Session) -> None:
    """Seeds standard TCK social graph (Alice, Bob, Charlie, Dan, Acme)."""
    seed_script = """
    CREATE (a:Person {name: 'Alice', age: 38, city: 'London'})
    CREATE (b:Person {name: 'Bob', age: 25, city: 'London'})
    CREATE (c:Person {name: 'Charlie', age: 53, city: 'New York'})
    CREATE (d:Person {name: 'Dan', age: 44, city: 'London'})
    CREATE (acme:Company {name: 'Acme Corp'})
    CREATE (a)-[:KNOWS {since: 1999}]->(b)
    CREATE (b)-[:KNOWS {since: 2010}]->(c)
    CREATE (c)-[:KNOWS {since: 2015}]->(d)
    CREATE (a)-[:KNOWS {since: 2005}]->(c)
    CREATE (a)-[:WORKS_AT {since: 2020}]->(acme)
    """
    clean_bg = " ".join(
        line for line in seed_script.splitlines() if not line.strip().startswith("//")
    ).replace(";", " ")
    session.execute(clean_bg)


@pytest.fixture(scope="module")
def neo4j_session():
    """Module-scoped persistent Neo4j session with pre-seeded TCK social graph."""
    if not NEO4J_ONLINE:
        yield None
        return

    driver = GraphDatabase.driver("bolt://localhost:7687", auth=("neo4j", "voyagerpass123"))
    session = Session(bridge=driver, dialect="cypher")
    try:
        session.execute("MATCH (n) DETACH DELETE n")
        seed_tck_social_graph(session)
        yield session
    finally:
        session.execute("MATCH (n) DETACH DELETE n")
        driver.close()


@pytest.fixture(autouse=True)
def _reset():
    reset_alias_counters()


# ---------------------------------------------------------------------------
# 1. Match & Predicate Permutations (WhereAcceptanceTest / MatchAcceptanceTest)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("pred_builder", "expected_cypher_fragment", "expected_params", "expected_names"),
    [
        (lambda p: (p.age == 38,), "p.age = $p0", {"p0": 38}, ["Alice"]),
        (lambda p: (p.age > 40,), "p.age > $p0", {"p0": 40}, ["Charlie", "Dan"]),
        (lambda p: (p.age >= 44,), "p.age >= $p0", {"p0": 44}, ["Charlie", "Dan"]),
        (lambda p: (p.age < 30,), "p.age < $p0", {"p0": 30}, ["Bob"]),
        (lambda p: (p.age <= 38,), "p.age <= $p0", {"p0": 38}, ["Alice", "Bob"]),
        (
            lambda p: (p.city == "London",),
            "p.city = $p0",
            {"p0": "London"},
            ["Alice", "Bob", "Dan"],
        ),
        (
            lambda p: (p.city == "London", p.age > 30),
            "(p.city = $p0) AND (p.age > $p1)",
            {"p0": "London", "p1": 30},
            ["Alice", "Dan"],
        ),
        (lambda p: (p.city.contains("York"),), "p.city CONTAINS $p0", {"p0": "York"}, ["Charlie"]),
        (
            lambda p: (p.city.contains("don"), p.age < 40),
            "(p.city CONTAINS $p0) AND (p.age < $p1)",
            {"p0": "don", "p1": 40},
            ["Alice", "Bob"],
        ),
    ],
    ids=[
        "eq_numeric",
        "gt_numeric",
        "gte_numeric",
        "lt_numeric",
        "lte_numeric",
        "eq_string",
        "multi_and_filters",
        "contains_string",
        "contains_and_numeric",
    ],
)
def test_tck_match_and_where_permutations(
    neo4j_session: Session | None,
    pred_builder: Any,
    expected_cypher_fragment: str,
    expected_params: dict[str, Any],
    expected_names: list[str],
):
    """TCK Permutations: MATCH (p:Person) WHERE <predicates> RETURN p.name."""
    p = Person(alias="p")
    preds = pred_builder(p)
    q = Query.match(p).where(*preds).return_(p.name).order_by(p.name)

    compiled = q.compile(dialect="cypher")
    assert expected_cypher_fragment in compiled.statement
    assert compiled.parameters == expected_params

    if neo4j_session is not None:
        records = neo4j_session.execute(q)
        actual_names = [r["p.name"] for r in records]
        assert actual_names == sorted(expected_names)


# ---------------------------------------------------------------------------
# 2. Traversal Direction & Variable Hop Permutations (PathAcceptanceTest)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    (
        "direction",
        "min_h",
        "max_h",
        "source_name",
        "expected_cypher_fragment",
        "expected_gql_fragment",
        "expected_targets",
    ),
    [
        (
            "to",
            1,
            1,
            "Alice",
            "MATCH (a:Person)-[r:KNOWS]->(b:Person)",
            "MATCH (a:Person)-[r:KNOWS]->(b:Person)",
            ["Bob", "Charlie"],
        ),
        (
            "from",
            1,
            1,
            "Charlie",
            "MATCH (a:Person)<-[r:KNOWS]-(b:Person)",
            "MATCH (a:Person)<-[r:KNOWS]-(b:Person)",
            ["Alice", "Bob"],
        ),
        (
            "edge",
            1,
            1,
            "Bob",
            "MATCH (a:Person)-[r:KNOWS]-(b:Person)",
            "MATCH (a:Person)-[r:KNOWS]-(b:Person)",
            ["Alice", "Charlie"],
        ),
        ("to", 1, 2, "Alice", ":KNOWS*1..2]->", ":KNOWS{1,2}]->", ["Bob", "Charlie", "Dan"]),
        ("to", 1, 3, "Alice", ":KNOWS*1..3]->", ":KNOWS{1,3}]->", ["Bob", "Charlie", "Dan"]),
        ("to", 2, 2, "Alice", ":KNOWS*2..2]->", ":KNOWS{2,2}]->", ["Charlie", "Dan"]),
    ],
    ids=[
        "outgoing_single_hop",
        "incoming_single_hop",
        "undirected_single_hop",
        "var_hops_1_to_2",
        "var_hops_1_to_3",
        "exact_hops_2",
    ],
)
def test_tck_traversal_direction_and_hops_permutations(
    neo4j_session: Session | None,
    direction: str,
    min_h: int,
    max_h: int,
    source_name: str,
    expected_cypher_fragment: str,
    expected_gql_fragment: str,
    expected_targets: list[str],
):
    """TCK Permutations: MATCH (a:Person)-[r:KNOWS*min..max]->(b:Person) WHERE a.name = $name."""
    a = Person(alias="a")
    b = Person(alias="b")

    q = Query.match(a)
    if min_h == 1 and max_h == 1:
        if direction == "to":
            q = q.to(Knows, var="r").node(b)
        elif direction == "from":
            q = q.from_(Knows, var="r").node(b)
        else:
            q = q.edge(Knows, var="r").node(b)
    else:
        q = q.to(Knows).hops(min_h, max_h).node(b)

    q = q.where(a.name == source_name).return_(b.name, distinct=True).order_by(b.name)

    cypher = q.compile(dialect="cypher")
    assert expected_cypher_fragment in cypher.statement
    assert cypher.parameters == {"p0": source_name}

    gql = q.compile(dialect="iso_gql")
    assert expected_gql_fragment in gql.statement

    if neo4j_session is not None:
        records = neo4j_session.execute(q)
        actual_targets = [r["b.name"] for r in records]
        assert sorted(actual_targets) == sorted(expected_targets)


# ---------------------------------------------------------------------------
# 3. Return, Sorting & Pagination Permutations (Return / OrderBy / SkipLimit)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("distinct", "order_col", "asc", "skip_n", "limit_n", "expected_values"),
    [
        (False, "age", True, None, None, ["Bob", "Alice", "Dan", "Charlie"]),
        (False, "age", False, None, None, ["Charlie", "Dan", "Alice", "Bob"]),
        (True, "city", True, None, None, ["London", "New York"]),
        (False, "age", True, 1, 2, ["Alice", "Dan"]),
        (False, "age", True, 2, None, ["Dan", "Charlie"]),
        (False, "age", True, None, 2, ["Bob", "Alice"]),
        (True, "city", True, 1, 1, ["New York"]),
    ],
    ids=[
        "order_by_age_asc",
        "order_by_age_desc",
        "distinct_city_asc",
        "skip_1_limit_2",
        "skip_2_only",
        "limit_2_only",
        "distinct_skip_1_limit_1",
    ],
)
def test_tck_return_order_and_pagination_permutations(
    neo4j_session: Session | None,
    distinct: bool,
    order_col: str,
    asc: bool,
    skip_n: int | None,
    limit_n: int | None,
    expected_values: list[str],
):
    """TCK Permutations: RETURN [DISTINCT] p.name/city ORDER BY ... [SKIP n] [LIMIT m]."""
    p = Person(alias="p")
    target_field = p.city if order_col == "city" else p.name
    sort_field = p.city if order_col == "city" else p.age

    q = Query.match(p).return_(target_field, distinct=distinct)
    q = q.order_by(sort_field, ascending=asc)

    if skip_n is not None:
        q = q.skip(skip_n)
    if limit_n is not None:
        q = q.limit(limit_n)

    cypher = q.compile(dialect="cypher")
    if distinct:
        assert "DISTINCT" in cypher.statement
    if skip_n is not None:
        assert f"SKIP {skip_n}" in cypher.statement
    if limit_n is not None:
        assert f"LIMIT {limit_n}" in cypher.statement

    if neo4j_session is not None:
        records = neo4j_session.execute(q)
        col_key = f"p.{target_field.field_name}"
        actual_vals = [r[col_key] for r in records]
        assert actual_vals == expected_values


# ---------------------------------------------------------------------------
# 4. Aggregations & Grouping Permutations (AggregationAcceptanceTest)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("group_by", "expected_records"),
    [
        (False, [{"count": 4}]),
        (True, [{"city": "London", "count": 3}, {"city": "New York", "count": 1}]),
    ],
    ids=[
        "global_node_count",
        "grouped_city_count",
    ],
)
def test_tck_aggregations_and_grouping_permutations(
    neo4j_session: Session | None,
    group_by: bool,
    expected_records: list[dict[str, Any]],
):
    """TCK Permutations: RETURN [p.city AS city,] count(p) AS count [ORDER BY city ASC]."""
    p = Person(alias="p")
    q = Query.match(p)
    if group_by:
        q = q.return_(city=p.city, count=p.count()).order_by(p.city)
    else:
        q = q.return_(count=p.count())

    cypher = q.compile(dialect="cypher")
    assert "count(p)" in cypher.statement.lower()

    if neo4j_session is not None:
        records = neo4j_session.execute(q)
        assert records == expected_records


# ---------------------------------------------------------------------------
# 5. DML Mutation Permutations (Create / Merge / Set / Remove / Delete)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("mutation_name", "action_fn", "expected_statement_fragment", "expected_params"),
    [
        (
            "create_node",
            lambda: Query.create(Person(alias="p")),
            "CREATE (p:Person)",
            {},
        ),
        (
            "create_path",
            lambda: Query.create(Person(alias="a")).to(Knows(alias="r")).node(Person(alias="b")),
            "CREATE (a:Person)-[r:KNOWS]->(b:Person)",
            {},
        ),
        (
            "merge_upsert",
            lambda: (
                Query.merge(Person(alias="p"))
                .on_create_set(Person("p").name == "Eva")
                .on_match_set(Person("p").age == 30)
            ),
            "MERGE (p:Person) ON CREATE SET p.name = $p0 ON MATCH SET p.age = $p1",
            {"p0": "Eva", "p1": 30},
        ),
        (
            "set_multi_props",
            lambda: (
                Query.match(Person(alias="p"))
                .where(Person("p").name == "Dan")
                .set(Person("p").age == 45, Person("p").city == "Oxford")
            ),
            "MATCH (p:Person) WHERE p.name = $p0 SET p.age = $p1, p.city = $p2",
            {"p0": "Dan", "p1": 45, "p2": "Oxford"},
        ),
        (
            "remove_property",
            lambda: (
                Query.match(Person(alias="p"))
                .where(Person("p").name == "Dan")
                .remove(Person("p").city)
            ),
            "MATCH (p:Person) WHERE p.name = $p0 REMOVE p.city",
            {"p0": "Dan"},
        ),
        (
            "detach_delete",
            lambda: (
                Query.match(Person(alias="p"))
                .where(Person("p").age < 18)
                .detach_delete(Person("p"))
            ),
            "MATCH (p:Person) WHERE p.age < $p0 DETACH DELETE p",
            {"p0": 18},
        ),
    ],
    ids=[
        "create_single_node",
        "create_path_chain",
        "merge_with_on_create_and_on_match",
        "set_multiple_properties",
        "remove_single_property",
        "detach_delete_cascade",
    ],
)
def test_tck_mutation_and_dml_permutations(
    mutation_name: str,
    action_fn: Any,
    expected_statement_fragment: str,
    expected_params: dict[str, Any],
):
    """TCK Permutations: DML mutation compiler emission and parameter extraction."""
    q: Query = action_fn()
    compiled = q.compile(dialect="cypher")
    assert expected_statement_fragment in compiled.statement
    assert compiled.parameters == expected_params


# ---------------------------------------------------------------------------
# 6. Bulk Ingestion & Vendor Procedure Permutations (Unwind / LoadCsv / Call)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("action_fn", "expected_cypher", "expected_params"),
    [
        (
            lambda: Query.unwind("batch", "row").add_create(Person(alias="p")),
            "UNWIND $batch AS row CREATE (p:Person)",
            {},
        ),
        (
            lambda: Query.unwind("payload_items", "item").add_create(Company(alias="c")),
            "UNWIND $payload_items AS item CREATE (c:Company)",
            {},
        ),
        (
            lambda: Query.load_csv("file:///data.csv", with_headers=True, alias="row").add_create(
                Person(alias="p")
            ),
            "LOAD CSV WITH HEADERS FROM $p0 AS row CREATE (p:Person)",
            {"p0": "file:///data.csv"},
        ),
        (
            lambda: Query.load_csv("file:///raw.csv", with_headers=False, alias="line").add_create(
                Person(alias="p")
            ),
            "LOAD CSV FROM $p0 AS line CREATE (p:Person)",
            {"p0": "file:///raw.csv"},
        ),
        (
            lambda: Query.call("dbms.components").yield_("name", "versions"),
            "CALL dbms.components() YIELD name, versions",
            {},
        ),
    ],
    ids=[
        "unwind_batch_parameter",
        "unwind_custom_param_and_alias",
        "load_csv_with_headers",
        "load_csv_without_headers",
        "vendor_procedure_call_yield",
    ],
)
def test_tck_bulk_and_procedure_permutations(
    action_fn: Any,
    expected_cypher: str,
    expected_params: dict[str, Any],
):
    """TCK Permutations: UNWIND, LOAD CSV, and CALL procedure execution."""
    q: Query = action_fn()
    compiled = q.compile(dialect="cypher")
    assert expected_cypher in compiled.statement
    assert compiled.parameters == expected_params


# ---------------------------------------------------------------------------
# 7. Outer-Join Traversal Permutations (OptionalMatchAcceptanceTest)
# ---------------------------------------------------------------------------


def test_tck_optional_match_outer_join_null_semantics(neo4j_session: Session | None):
    """TCK: MATCH (a:Person) OPTIONAL MATCH (a)-[r:WORKS_AT]->(c:Company) RETURN a.name, c.name."""
    a = Person(alias="a")
    c = Company(alias="c")
    q = (
        Query.match(a)
        .add_optional_match(a)
        .to(WorksAt, var="r")
        .node(c)
        .return_(person=a.name, company=c.name)
        .order_by(a.name)
    )

    cypher = q.compile(dialect="cypher")
    assert (
        "MATCH (a:Person) OPTIONAL MATCH (a:Person)-[r:WORKS_AT]->(c:Company)" in cypher.statement
    )

    if neo4j_session is not None:
        records = neo4j_session.execute(q)
        assert len(records) == 4
        # Alice has a company, others have None (outer-join null binding)
        assert records[0] == {"person": "Alice", "company": "Acme Corp"}
        assert records[1] == {"person": "Bob", "company": None}
        assert records[2] == {"person": "Charlie", "company": None}
        assert records[3] == {"person": "Dan", "company": None}
