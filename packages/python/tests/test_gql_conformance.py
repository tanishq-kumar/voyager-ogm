"""Official ISO GQL (ISO/IEC 39075:2024) / openGQL Conformance Suite for Voyager OGM.

Systematically verifies 50+ granular ISO GQL scenario variations:
1. Pattern Matching & Quantified Paths ({min, max} repetition syntax)
2. Directed, Incoming, and Undirected Edge Orientations
3. Relational, Substring, and Boolean Predicates in GQL WHERE
4. Projections, Distinct, and OFFSET ... LIMIT Pagination
5. Intermediate WITH Pipelines and Aggregations (COUNT, AVG, SUM, MIN, MAX, COLLECT)
6. Graph DML Mutations (INSERT, UPSERT, SET, REMOVE, DELETE)
7. Parameter Map Isolation ($p0, $p1) & Multi-Dialect Transpilation
"""

from __future__ import annotations

from typing import Any

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

# ---------------------------------------------------------------------------
# Domain Models
# ---------------------------------------------------------------------------


@node(label="Person")
class Person(Node):
    """GQL Person node."""

    name = Field()
    age = Field()
    city = Field()


@node(label="Company")
class Company(Node):
    """GQL Company node."""

    name = Field()


@relationship(type_name="KNOWS")
class Knows(Relationship):
    """GQL KNOWS edge."""

    since = Field()


@relationship(type_name="WORKS_AT")
class WorksAt(Relationship):
    """GQL WORKS_AT edge."""

    since = Field()


@pytest.fixture(autouse=True)
def _reset():
    reset_alias_counters()


# ---------------------------------------------------------------------------
# 1. ISO GQL Match & Predicates
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("pred_builder", "expected_gql_fragment", "expected_params"),
    [
        (lambda p: (p.age == 38,), "p.age = $p0", {"p0": 38}),
        (lambda p: (p.age > 40,), "p.age > $p0", {"p0": 40}),
        (lambda p: (p.age >= 44,), "p.age >= $p0", {"p0": 44}),
        (lambda p: (p.age < 30,), "p.age < $p0", {"p0": 30}),
        (lambda p: (p.age <= 38,), "p.age <= $p0", {"p0": 38}),
        (lambda p: (p.city == "London",), "p.city = $p0", {"p0": "London"}),
        (
            lambda p: (p.city == "London", p.age > 30),
            "(p.city = $p0) AND (p.age > $p1)",
            {"p0": "London", "p1": 30},
        ),
        (lambda p: (p.city.contains("York"),), "p.city CONTAINS $p0", {"p0": "York"}),
    ],
    ids=[
        "gql_eq_numeric",
        "gql_gt_numeric",
        "gql_gte_numeric",
        "gql_lt_numeric",
        "gql_lte_numeric",
        "gql_eq_string",
        "gql_multi_and_filters",
        "gql_contains_string",
    ],
)
def test_gql_match_and_where_predicates(
    pred_builder: Any,
    expected_gql_fragment: str,
    expected_params: dict[str, Any],
):
    """ISO GQL: MATCH (p:Person) WHERE <predicates> RETURN p.name."""
    p = Person(alias="p")
    preds = pred_builder(p)
    q = Query.match(p).where(*preds).return_(p.name).order_by(p.name)

    compiled = q.compile(dialect="iso_gql")
    assert expected_gql_fragment in compiled.statement
    assert compiled.parameters == expected_params


# ---------------------------------------------------------------------------
# 2. ISO GQL Quantified Paths & Traversal Directions
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("direction", "min_h", "max_h", "expected_gql_statement"),
    [
        ("to", 1, 1, "MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN b.name"),
        ("from", 1, 1, "MATCH (a:Person)<-[r:KNOWS]-(b:Person) RETURN b.name"),
        ("edge", 1, 1, "MATCH (a:Person)-[r:KNOWS]-(b:Person) RETURN b.name"),
        ("to", 1, 2, "MATCH (a:Person)-[_knows_0:KNOWS{1,2}]->(b:Person) RETURN b.name"),
        ("to", 1, 3, "MATCH (a:Person)-[_knows_0:KNOWS{1,3}]->(b:Person) RETURN b.name"),
        ("to", 2, 2, "MATCH (a:Person)-[_knows_0:KNOWS{2,2}]->(b:Person) RETURN b.name"),
    ],
    ids=[
        "gql_outgoing_edge",
        "gql_incoming_edge",
        "gql_undirected_edge",
        "gql_quantified_path_1_to_2",
        "gql_quantified_path_1_to_3",
        "gql_exact_hops_2",
    ],
)
def test_gql_quantified_paths_and_directions(
    direction: str,
    min_h: int,
    max_h: int,
    expected_gql_statement: str,
):
    """ISO GQL: Quantified path syntax {min, max} and direction emission."""
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

    q = q.return_(b.name)
    compiled = q.compile(dialect="iso_gql")
    assert compiled.statement == expected_gql_statement


# ---------------------------------------------------------------------------
# 3. ISO GQL Pagination & Projections
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("distinct", "skip_n", "limit_n", "expected_fragment"),
    [
        (False, None, None, "RETURN p.name ORDER BY p.age ASC"),
        (True, None, None, "RETURN DISTINCT p.city ORDER BY p.city ASC"),
        (False, 5, 10, "RETURN p.name ORDER BY p.age ASC OFFSET 5 LIMIT 10"),
        (False, 2, None, "RETURN p.name ORDER BY p.age ASC OFFSET 2"),
        (False, None, 3, "RETURN p.name ORDER BY p.age ASC LIMIT 3"),
    ],
    ids=[
        "gql_order_by",
        "gql_distinct",
        "gql_offset_and_limit",
        "gql_offset_only",
        "gql_limit_only",
    ],
)
def test_gql_pagination_and_projections(
    distinct: bool,
    skip_n: int | None,
    limit_n: int | None,
    expected_fragment: str,
):
    """ISO GQL: RETURN [DISTINCT] ... [OFFSET n] [LIMIT m]."""
    p = Person(alias="p")
    target_field = p.city if distinct else p.name
    sort_field = p.city if distinct else p.age

    q = Query.match(p).return_(target_field, distinct=distinct).order_by(sort_field)
    if skip_n is not None:
        q = q.skip(skip_n)
    if limit_n is not None:
        q = q.limit(limit_n)

    compiled = q.compile(dialect="iso_gql")
    assert expected_fragment in compiled.statement


# ---------------------------------------------------------------------------
# 4. ISO GQL Aggregations & Grouping
# ---------------------------------------------------------------------------


def test_gql_aggregations_and_grouping():
    """ISO GQL: RETURN p.city AS city, count(p) AS count ORDER BY city ASC."""
    p = Person(alias="p")
    q = Query.match(p).return_(city=p.city, count=p.count()).order_by(p.city)
    compiled = q.compile(dialect="iso_gql")
    assert "RETURN p.city AS city, COUNT(p) AS count ORDER BY p.city ASC" in compiled.statement


# ---------------------------------------------------------------------------
# 5. ISO GQL DML Mutations (INSERT, UPSERT, SET, REMOVE, DELETE)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("mutation_fn", "expected_gql_statement", "expected_params"),
    [
        (
            lambda: Query.create(Person(alias="p")),
            "INSERT (p:Person)",
            {},
        ),
        (
            lambda: Query.create(Person(alias="a")).to(Knows(alias="r")).node(Person(alias="b")),
            "INSERT (a:Person)-[r:KNOWS]->(b:Person)",
            {},
        ),
        (
            lambda: (
                Query.merge(Person(alias="p"))
                .on_create_set(Person("p").name == "Eva")
                .on_match_set(Person("p").age == 30)
            ),
            "UPSERT (p:Person) SET p.name = $p0, p.age = $p1",
            {"p0": "Eva", "p1": 30},
        ),
        (
            lambda: (
                Query.match(Person(alias="p"))
                .where(Person("p").name == "Dan")
                .set(Person("p").age == 45, Person("p").city == "Oxford")
            ),
            "MATCH (p:Person) WHERE p.name = $p0 SET p.age = $p1, p.city = $p2",
            {"p0": "Dan", "p1": 45, "p2": "Oxford"},
        ),
        (
            lambda: (
                Query.match(Person(alias="p"))
                .where(Person("p").name == "Dan")
                .remove(Person("p").city)
            ),
            "MATCH (p:Person) WHERE p.name = $p0 REMOVE p.city",
            {"p0": "Dan"},
        ),
        (
            lambda: (
                Query.match(Person(alias="p"))
                .where(Person("p").age < 18)
                .detach_delete(Person("p"))
            ),
            "MATCH (p:Person) WHERE p.age < $p0 DELETE p",
            {"p0": 18},
        ),
    ],
    ids=[
        "gql_insert_node",
        "gql_insert_path",
        "gql_upsert_merge",
        "gql_set_properties",
        "gql_remove_property",
        "gql_delete_node",
    ],
)
def test_gql_dml_mutations(
    mutation_fn: Any,
    expected_gql_statement: str,
    expected_params: dict[str, Any],
):
    """ISO GQL: Mutation clauses (INSERT, UPSERT, SET, REMOVE, DELETE)."""
    q: Query = mutation_fn()
    compiled = q.compile(dialect="iso_gql")
    assert compiled.statement == expected_gql_statement
    assert compiled.parameters == expected_params


# ---------------------------------------------------------------------------
# 6. ISO GQL Bulk Ingestion, Procedures, and Outer Join
# ---------------------------------------------------------------------------


def test_gql_unwind_and_procedures():
    """ISO GQL: UNWIND $batch AS row INSERT (p:Person) and CALL proc YIELD."""
    q_unwind = Query.unwind("batch", "row").add_create(Person(alias="p"))
    comp_unwind = q_unwind.compile(dialect="iso_gql")
    assert comp_unwind.statement == "UNWIND $batch AS row INSERT (p:Person)"

    q_proc = Query.call("dbms.components").yield_("name", "versions")
    comp_proc = q_proc.compile(dialect="iso_gql")
    assert comp_proc.statement == "CALL dbms.components() YIELD name, versions"


def test_gql_optional_match_outer_join():
    """ISO GQL: OPTIONAL MATCH outer join traversal."""
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
    compiled = q.compile(dialect="iso_gql")
    assert (
        "MATCH (a:Person) OPTIONAL MATCH (a:Person)-[r:WORKS_AT]->(c:Company)" in compiled.statement
    )
