"""Tests for DML Mutations (CREATE, MERGE, SET, DELETE, REMOVE) and Model Dirty Tracking."""

import pytest
from voyager_ogm import Query, node, relationship
from voyager_ogm.models import reset_alias_counters


@node(label="Person")
class Person:
    name: str
    age: int
    status: str = "PENDING"


@relationship(type_name="FOLLOWS")
class Follows:
    since: int = 2026


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


def test_python_create_single_node():
    p = Person()
    query = Query.create(p)
    compiled = query.compile("cypher")

    assert compiled.statement == "CREATE (_person_0:Person)"
    assert compiled.parameters == {}


def test_python_create_relationship_path():
    a = Person()
    b = Person()
    f = Follows()

    query = Query.create(a).to(f).node(b)
    compiled = query.compile("cypher")

    assert (
        compiled.statement == "CREATE (_person_0:Person)-[_follows_0:FOLLOWS]->(_person_1:Person)"
    )


def test_python_merge_with_on_create_and_on_match():
    p = Person()
    query = (
        Query.merge(p)
        .on_create_set(p.name == "Alice", p.age == 30)
        .on_match_set(p.status == "ACTIVE")
    )
    compiled = query.compile("cypher")

    expected_stmt = (
        "MERGE (_person_0:Person) "
        "ON CREATE SET _person_0.name = $p0, _person_0.age = $p1 "
        "ON MATCH SET _person_0.status = $p2"
    )
    assert compiled.statement == expected_stmt
    assert compiled.parameters["p0"] == "Alice"
    assert compiled.parameters["p1"] == 30
    assert compiled.parameters["p2"] == "ACTIVE"


def test_python_match_then_set_properties():
    p = Person()
    query = (
        Query.match(p).where(p.age > 18).set(p.status == "VERIFIED", p.age == 21).return_(p.name)
    )
    compiled = query.compile("cypher")

    expected_stmt = (
        "MATCH (_person_0:Person) "
        "WHERE _person_0.age > $p0 "
        "SET _person_0.status = $p1, _person_0.age = $p2 "
        "RETURN _person_0.name"
    )
    assert compiled.statement == expected_stmt
    assert compiled.parameters["p0"] == 18
    assert compiled.parameters["p1"] == "VERIFIED"
    assert compiled.parameters["p2"] == 21


def test_python_detach_delete():
    p = Person()
    query = Query.match(p).where(p.status == "BANNED").detach_delete(p)
    compiled = query.compile("cypher")

    assert (
        compiled.statement
        == "MATCH (_person_0:Person) WHERE _person_0.status = $p0 DETACH DELETE _person_0"
    )
    assert compiled.parameters["p0"] == "BANNED"


def test_python_remove_property():
    p = Person()
    query = Query.match(p).where(p.name == "TempUser").remove(p.status)
    compiled = query.compile("cypher")

    assert (
        compiled.statement
        == "MATCH (_person_0:Person) WHERE _person_0.name = $p0 REMOVE _person_0.status"
    )


def test_python_model_dirty_tracking():
    p = Person(name="Alice", age=25)
    assert p.dirty_fields == {"name": "Alice", "age": 25}

    p.clear_dirty()
    assert p.dirty_fields == {}

    p.age = 26
    p.status = "VERIFIED"
    assert p.dirty_fields == {"age": 26, "status": "VERIFIED"}

    # Using dirty node in Query.set
    query = Query.match(p).where(p.name == "Alice").set(p)
    compiled = query.compile("cypher")

    assert "SET _person_0.age = $p1, _person_0.status = $p2" in compiled.statement
    assert compiled.parameters["p1"] == 26
    assert compiled.parameters["p2"] == "VERIFIED"
