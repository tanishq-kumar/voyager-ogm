"""Tests for Wave 1 OGM Write Path (#174, #175, #176).

Covers:
- #174: Query.create(instance) / Query.merge(instance) carrying property values inline
  (cypher, iso_gql, age), relationship instance properties inline, and create(x).set(x) compatibility.
- #175: Query.merge(...).where(...) folding equality predicates into inline pattern properties,
  and raising ValueError on non-equality, functions, or leftover WHERE predicates.
- #176: Class-level field resolution to auto-alias when exactly one pattern exists,
  and raising ValueError when ambiguous (multiple patterns) or absent.
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


@node("Person")
class Person(Node):
    id: int = Field(primary_key=True)
    name: str = Field()
    age: int = Field()


@node("Company")
class Company(Node):
    name: str = Field(unique=True)
    founded: int = Field()


@relationship(type_name="TX")
class Tx(Relationship):
    amount: float = Field()


@relationship(type_name="KNOWS")
class Knows(Relationship):
    pass


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


# =========================================================================
# A. #174: Query.create(instance) / Query.merge(instance)
# =========================================================================


def test_create_instance_cypher():
    p = Person(id=1, name="Ada", age=36)
    q = Query.create(p)
    compiled = q.compile("cypher")
    assert compiled.statement == "CREATE (_person_0:Person {id: $p0, name: $p1, age: $p2})"
    assert compiled.parameters == {"p0": 1, "p1": "Ada", "p2": 36}


def test_create_instance_iso_gql():
    p = Person(id=1, name="Ada", age=36)
    q = Query.create(p)
    compiled = q.compile("iso_gql")
    assert compiled.statement == "INSERT (_person_0:Person {id: $p0, name: $p1, age: $p2})"
    assert compiled.parameters == {"p0": 1, "p1": "Ada", "p2": 36}


def test_create_instance_age():
    p = Person(id=1, name="Ada", age=36)
    q = Query.create(p)
    compiled = q.compile("age")
    assert "CREATE (_person_0:Person {id: $p0, name: $p1, age: $p2})" in compiled.statement
    assert "cypher('age_graph'" in compiled.statement
    assert compiled.parameters == {"p0": 1, "p1": "Ada", "p2": 36}


def test_create_path_with_relationship_instance():
    src = Person(id=1, name="Ada", age=36)
    dst = Person(id=2, name="Bob", age=25)
    tx = Tx(amount=750.0)
    q = Query.create(src).to(tx).node(dst)
    compiled = q.compile("cypher")
    assert compiled.statement == (
        "CREATE (_person_0:Person {id: $p0, name: $p1, age: $p2})"
        "-[_tx_0:TX {amount: $p3}]->"
        "(_person_1:Person {id: $p4, name: $p5, age: $p6})"
    )
    assert compiled.parameters == {
        "p0": 1,
        "p1": "Ada",
        "p2": 36,
        "p3": 750.0,
        "p4": 2,
        "p5": "Bob",
        "p6": 25,
    }


def test_merge_instance_pk_cypher():
    p = Person(id=1, name="Ada", age=36)
    q = Query.merge(p)
    compiled = q.compile("cypher")
    assert compiled.statement == (
        "MERGE (_person_0:Person {id: $p0}) "
        "ON CREATE SET _person_0.name = $p1, _person_0.age = $p2 "
        "ON MATCH SET _person_0.name = $p1, _person_0.age = $p2"
    )
    assert compiled.parameters == {"p0": 1, "p1": "Ada", "p2": 36}


def test_merge_instance_unique_cypher():
    c = Company(name="Acme", founded=1980)
    q = Query.merge(c)
    compiled = q.compile("cypher")
    assert compiled.statement == (
        "MERGE (_company_0:Company {name: $p0}) "
        "ON CREATE SET _company_0.founded = $p1 "
        "ON MATCH SET _company_0.founded = $p1"
    )
    assert compiled.parameters == {"p0": "Acme", "p1": 1980}


def test_create_workaround_set_still_works():
    p = Person(id=1, name="Ada", age=36)
    q = Query.create(p).set(p)
    compiled = q.compile("cypher")
    assert "CREATE (_person_0:Person {id: $p0, name: $p1, age: $p2})" in compiled.statement
    assert "SET _person_0.id = $p3, _person_0.name = $p4, _person_0.age = $p5" in compiled.statement


def test_match_instance_does_not_inline_properties():
    p = Person(id=1, name="Ada", age=36)
    q = Query.match(p)
    compiled = q.compile("cypher")
    assert compiled.statement == "MATCH (_person_0:Person)"
    assert compiled.parameters == {}


# =========================================================================
# B. #175: Query.merge(...).where(...)
# =========================================================================


def test_merge_where_eq_folds_into_pattern():
    q = Query.merge(Person).where(Person.name == "Ada")
    compiled = q.compile("cypher")
    assert compiled.statement == "MERGE (_person_0:Person {name: $p0})"
    assert compiled.parameters == {"p0": "Ada"}


def test_merge_where_multiple_eq_folds():
    q = Query.merge(Person).where(Person.name == "Ada", Person.age == 36)
    compiled = q.compile("cypher")
    assert compiled.statement == "MERGE (_person_0:Person {name: $p0, age: $p1})"
    assert compiled.parameters == {"p0": "Ada", "p1": 36}


def test_merge_where_non_equality_raises_value_error():
    with pytest.raises(ValueError, match="MERGE clause does not support WHERE predicates"):
        Query.merge(Person).where(Person.age > 30)


def test_merge_where_not_raises_value_error():
    with pytest.raises(ValueError, match="MERGE clause does not support WHERE predicates"):
        Query.merge(Person).where_not(Person.name == "Ada")


# =========================================================================
# C. #176: Class-level field resolution
# =========================================================================


def test_match_return_class_field():
    q = Query.match(Person).return_(Person.name)
    compiled = q.compile("cypher")
    assert compiled.statement == "MATCH (_person_0:Person) RETURN _person_0.name"


def test_match_where_class_field():
    q = Query.match(Person).where(Person.name == "x").return_(Person.name)
    compiled = q.compile("cypher")
    assert (
        compiled.statement
        == "MATCH (_person_0:Person) WHERE _person_0.name = $p0 RETURN _person_0.name"
    )
    assert compiled.parameters == {"p0": "x"}


def test_match_order_by_class_field():
    q = Query.match(Person).return_(Person.name).order_by(Person.name)
    compiled = q.compile("cypher")
    assert (
        compiled.statement
        == "MATCH (_person_0:Person) RETURN _person_0.name ORDER BY _person_0.name ASC"
    )


def test_ambiguous_class_field_raises_value_error():
    with pytest.raises(ValueError, match="ambiguous.*multiple patterns"):
        Query.match(Person).to(Knows).node(Person).return_(Person.name)


def test_unmatched_class_field_raises_value_error():
    with pytest.raises(ValueError, match="cannot be resolved.*no pattern"):
        Query.match(Company).return_(Person.name)


def test_aliased_instance_remains_unchanged():
    p = Person(alias="p")
    q = Query.match(p).return_(p.name)
    compiled = q.compile("cypher")
    assert compiled.statement == "MATCH (p:Person) RETURN p.name"


def test_class_level_field_stays_hashable() -> None:
    """Overloading Field.__eq__ must not make Field unhashable (dict keys, sets)."""
    fields = {Person.name, Person.age}
    assert Person.name in fields
    assert {Person.name: 1}[Person.name] == 1
