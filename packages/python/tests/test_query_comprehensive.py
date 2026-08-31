"""Comprehensive tests covering all Query builder methods and model descriptors."""

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


@pytest.fixture(autouse=True)
def reset_aliases():
    reset_alias_counters()


@node(label="Person")
class Person(Node):
    name = Field()
    age = Field()
    score = Field()


@node(label="Company")
class Company(Node):
    name = Field()


@relationship(type_name="WORKS_AT")
class WorksAt(Relationship):
    since = Field()


@relationship(type_name="MANAGES")
class Manages(Relationship):
    department = Field()


def test_query_incoming_edge_traversal():
    p = Person("p")
    c = Company("c")

    query = (
        Query.match(p)
        .from_(WorksAt, var="w")
        .node(c)
        .where(p.age <= 65, p.score < 100.0)
        .return_(p.name, c.name)
    )

    compiled = query.compile("cypher")
    assert "MATCH (p:Person)<-[w:WORKS_AT]-(c:Company)" in compiled.statement
    assert compiled.parameters == {"p0": 65, "p1": 100.0}


def test_query_optional_match_and_distinct():
    p = Person("p")
    c = Company("c")

    query = (
        Query.match(p)
        .add_optional_match(c)
        .to(Manages, var="m")
        .node(p)
        .where(p.score >= 50.0)
        .return_(p.name, c.name, distinct=True)
        .order_by(p.name, ascending=False)
        .skip(5)
        .limit(10)
    )

    compiled = query.compile("cypher")
    assert "MATCH (p:Person)" in compiled.statement
    assert "OPTIONAL MATCH (c:Company)-[m:MANAGES]->(p:Person)" in compiled.statement
    assert "RETURN DISTINCT p.name, c.name" in compiled.statement
    assert "ORDER BY p.name DESC" in compiled.statement
    assert "SKIP 5 LIMIT 10" in compiled.statement


def test_field_binding_and_operators():
    p = Person("p")
    assert (p.age >= 18).op == "gte"
    assert (p.age <= 65).op == "lte"
    assert (p.score < 50.0).op == "lt"
    assert (p.name == "Alice").op == "eq"

    f = Field(name="custom")
    bound = f.bind("alias1")
    assert bound.target_alias == "alias1"
    assert bound.field_name == "custom"


def test_query_order_by_desc_shortcut():
    p = Person("p")
    query = Query.match(p).return_(p.name).order_by_desc(p.age)
    compiled = query.compile("cypher")
    assert "ORDER BY p.age DESC" in compiled.statement
