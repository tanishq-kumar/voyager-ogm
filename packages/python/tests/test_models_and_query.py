"""Tests for Voyager OGM Python Models and Fluent Query Builder."""

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


# Style 1: Pure decorator without (Node)
@node
class Person:
    name = Field()
    age = Field()
    city = Field()


# Style 2: Custom label decorator without (Node)
@node("Film")
class Movie:
    title = Field()
    released = Field()


# Style 3: Pure decorator relationship without (Relationship)
@relationship
class ActedIn:
    role = Field()


# Style 4: Subclassing with kwargs
class Directed(Relationship, type_name="DIRECTED"):
    year = Field()


# Style 5: Pure OOP subclassing
class User(Node, label="Customer"):
    username = Field()


def test_decorator_auto_inherits_node_and_relationship():
    p = Person()
    m = Movie()
    rel = ActedIn()
    u = User()

    assert isinstance(p, Node)
    assert p.labels == ["Person"]
    assert p.alias == "_person_0"

    assert isinstance(m, Node)
    assert m.labels == ["Film"]
    assert m.alias == "_film_0"

    assert isinstance(rel, Relationship)
    assert rel.edge_type == "ACTEDIN"
    assert rel.alias == "_actedin_0"

    assert isinstance(u, Node)
    assert u.labels == ["Customer"]
    assert u.alias == "_customer_0"


def test_model_constructor_auto_aliasing():
    p1 = Person()
    p2 = Person()
    m = Movie()

    assert p1.alias == "_person_0"
    assert p2.alias == "_person_1"
    assert m.alias == "_film_0"


def test_model_custom_aliasing():
    p = Person(alias="actor")
    m = Movie(alias="m")

    assert p.alias == "actor"
    assert m.alias == "m"


def test_fluent_query_cypher_compilation():
    p = Person()
    m = Movie()

    query = (
        Query.match(p)
        .to(ActedIn)
        .hops(1, 2)
        .node(m)
        .where(p.age > 21, m.released == 1999)
        .return_(p.name, m.title, actor_name=p.name)
        .order_by(p.name)
        .limit(10)
    )

    compiled = query.compile("cypher")
    expected = (
        "MATCH (_person_0:Person)-[_actedin_0:ACTEDIN*1..2]->(_film_0:Film) "
        "WHERE (_person_0.age > $p0) AND (_film_0.released = $p1) "
        "RETURN _person_0.name, _film_0.title, _person_0.name AS actor_name "
        "ORDER BY _person_0.name ASC LIMIT 10"
    )
    assert compiled.statement == expected
    assert compiled.parameters == {"p0": 21, "p1": 1999}


def test_fluent_query_sql_pgq_compilation():
    p = Person("p")
    m = Movie("m")

    query = (
        Query.match(p)
        .to(ActedIn, var="r")
        .node(m)
        .where(p.age > 25)
        .return_(p.name, m.title)
        .limit(20)
    )

    compiled = query.compile("sql_pgq", graph_name="cinema_graph")
    expected = (
        "SELECT * FROM GRAPH_TABLE (cinema_graph MATCH (p IS Person) "
        "-[r IS ACTEDIN]-> (m IS Film) "
        "WHERE p.age > $p0 "
        "COLUMNS (p.name, m.title)) "
        "LIMIT 20"
    )
    assert compiled.statement == expected
    assert compiled.parameters == {"p0": 25}


def test_fluent_query_iso_gql_compilation():
    p = Person("p")
    m = Movie("m")

    query = (
        Query.match(p)
        .to(ActedIn, var="r")
        .node(m)
        .where(m.title == "The Matrix")
        .return_(p.name, m.title)
    )

    compiled = query.compile("iso_gql")
    assert (
        compiled.statement
        == "MATCH (p:Person)-[r:ACTEDIN]->(m:Film) WHERE m.title = $p0 RETURN p.name, m.title"
    )
    assert compiled.parameters == {"p0": "The Matrix"}


def test_string_contains_predicate():
    p = Person("p")

    query = Query.match(p).where(p.name.contains("Keanu")).return_(p.name)

    compiled_cypher = query.compile("cypher")
    assert compiled_cypher.statement == "MATCH (p:Person) WHERE p.name CONTAINS $p0 RETURN p.name"
    assert compiled_cypher.parameters == {"p0": "Keanu"}

    compiled_pgq = query.compile("sql_pgq", graph_name="social")
    expected_pgq = (
        "SELECT * FROM GRAPH_TABLE (social MATCH (p IS Person) "
        "WHERE p.name LIKE '%' || $p0 || '%' "
        "COLUMNS (p.name))"
    )
    assert compiled_pgq.statement == expected_pgq
