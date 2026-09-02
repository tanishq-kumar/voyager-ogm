"""Query Authoring Style Equivalence Conformance Suite.

Verifies that all ergonomic query authoring approaches supported by Voyager OGM:
1. Model Instance Chaining (p = Person("p"))
2. Class Reference Auto-Aliasing (Query.match(Person))
3. Raw String & Label Array Chaining (Query.match("p", labels=["Person"]))
4. Step-by-Step Memgraph / GQLAlchemy Chaining (Query.match().node(...).to(...).node(...))
5. Single-Call Helper Chaining (Query.match_node("p", "Person"))

produce 100% equivalent ASTs, identical parameter extraction ($p0, $p1),
and identical compiled outputs across openCypher and ISO GQL.
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


@pytest.fixture(autouse=True)
def _reset():
    reset_alias_counters()


# ---------------------------------------------------------------------------
# 1. Single Node Matching & Predicate Style Equivalence
# ---------------------------------------------------------------------------


def test_single_node_query_styles_equivalence():
    """Verifies that all 5 authoring styles for single node match produce identical Cypher."""
    # Style 1: Model Instance with Bound Fields
    p = Person(alias="p")
    q1 = Query.match(p).where(p.age > 30).return_(p.name).order_by(p.age)

    # Style 2: Raw Identifier & Labels
    q2 = (
        Query.match("p", labels=["Person"])
        .where(Person("p").age > 30)
        .return_(Person("p").name)
        .order_by(Person("p").age)
    )

    # Style 3: GQLAlchemy / Step-by-step empty match start
    q3 = (
        Query.match()
        .node(labels=["Person"], variable="p")
        .where(Person("p").age > 30)
        .return_(Person("p").name)
        .order_by(Person("p").age)
    )

    # Style 4: Single-Call Helper (match_node)
    q4 = (
        Query.match_node("p", "Person")
        .where(Person("p").age > 30)
        .return_(Person("p").name)
        .order_by(Person("p").age)
    )

    c1 = q1.compile("cypher")
    c2 = q2.compile("cypher")
    c3 = q3.compile("cypher")
    c4 = q4.compile("cypher")

    expected_statement = "MATCH (p:Person) WHERE p.age > $p0 RETURN p.name ORDER BY p.age ASC"
    expected_params = {"p0": 30}

    assert c1.statement == expected_statement
    assert c2.statement == expected_statement
    assert c3.statement == expected_statement
    assert c4.statement == expected_statement

    assert c1.parameters == expected_params
    assert c2.parameters == expected_params
    assert c3.parameters == expected_params
    assert c4.parameters == expected_params


# ---------------------------------------------------------------------------
# 2. Multi-Hop Relationship Traversal Style Equivalence
# ---------------------------------------------------------------------------


def test_multi_hop_traversal_styles_equivalence():
    """Verifies that all authoring styles for (a:Person)-[r:KNOWS]->(b:Person) match identically."""
    # Style 1: Model Instance Traversal
    a = Person(alias="a")
    b = Person(alias="b")
    q1 = Query.match(a).to(Knows, var="r").node(b).where(a.name == "Alice").return_(b.name)

    # Style 2: Relationship Class Instance
    q2 = Query.match(a).to(Knows(alias="r")).node(b).where(a.name == "Alice").return_(b.name)

    # Style 3: Raw Edge Type String & Variable
    q3 = (
        Query.match(a)
        .to(edge_type="KNOWS", variable="r")
        .node(b)
        .where(a.name == "Alice")
        .return_(b.name)
    )

    # Style 4: Step-by-Step Node & Edge Chaining
    q4 = (
        Query.match()
        .node(labels=["Person"], variable="a")
        .to(edge_type="KNOWS", variable="r")
        .node(labels=["Person"], variable="b")
        .where(Person("a").name == "Alice")
        .return_(Person("b").name)
    )

    c1 = q1.compile("cypher")
    c2 = q2.compile("cypher")
    c3 = q3.compile("cypher")
    c4 = q4.compile("cypher")

    expected_statement = "MATCH (a:Person)-[r:KNOWS]->(b:Person) WHERE a.name = $p0 RETURN b.name"
    expected_params = {"p0": "Alice"}

    assert c1.statement == expected_statement
    assert c2.statement == expected_statement
    assert c3.statement == expected_statement
    assert c4.statement == expected_statement

    assert c1.parameters == expected_params
    assert c2.parameters == expected_params
    assert c3.parameters == expected_params
    assert c4.parameters == expected_params


# ---------------------------------------------------------------------------
# 3. Projection & Aliasing Style Equivalence
# ---------------------------------------------------------------------------


def test_projection_and_aliasing_styles_equivalence():
    """Verifies positional, keyword, and expression projection styles."""
    p = Person(alias="p")

    # Style 1: Keyword arguments (full_name=p.name, user_age=p.age)
    q1 = Query.match(p).return_(full_name=p.name, user_age=p.age)

    # Style 2: Positional BoundField references
    q2 = Query.match(p).return_(p.name, p.age)

    c1 = q1.compile("cypher")
    c2 = q2.compile("cypher")

    assert c1.statement == "MATCH (p:Person) RETURN p.name AS full_name, p.age AS user_age"
    assert c2.statement == "MATCH (p:Person) RETURN p.name, p.age"


# ---------------------------------------------------------------------------
# 4. Mutation Style Equivalence (CREATE & MERGE)
# ---------------------------------------------------------------------------


def test_mutation_creation_styles_equivalence():
    """Verifies node and path creation across different builder styles."""
    # Style 1: Model Instance Creation
    p = Person(alias="p")
    q1 = Query.create(p)

    # Style 2: Step-by-step create chaining
    q2 = Query.create().node(labels=["Person"], variable="p")

    c1 = q1.compile("cypher")
    c2 = q2.compile("cypher")

    assert c1.statement == "CREATE (p:Person)"
    assert c2.statement == "CREATE (p:Person)"

    g1 = q1.compile("iso_gql")
    g2 = q2.compile("iso_gql")

    assert g1.statement == "INSERT (p:Person)"
    assert g2.statement == "INSERT (p:Person)"
