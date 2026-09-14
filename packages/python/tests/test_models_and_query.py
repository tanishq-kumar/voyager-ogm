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


def test_typo_safe_node_attribute_access():
    p = Person("p")
    # Declared fields work normally and are cached
    assert p.name is not None
    assert p.age is not None
    assert p.name is p.name  # Descriptor caching in _bound_fields

    # Misspelled attribute with close match raises AttributeError with suggestion
    with pytest.raises(
        AttributeError, match="'Person' has no property 'nmae'\\. Did you mean 'name'\\?"
    ):
        _ = p.nmae

    # Misspelled attribute with another close match
    with pytest.raises(
        AttributeError, match="'Person' has no property 'aeg'\\. Did you mean 'age'\\?"
    ):
        _ = p.aeg

    # Attribute with no close match raises AttributeError without suggestion
    with pytest.raises(AttributeError, match="'Person' has no property 'completely_unknown'"):
        _ = p.completely_unknown


def test_typo_safe_relationship_attribute_access():
    rel = ActedIn("r")
    assert rel.role is not None

    with pytest.raises(
        AttributeError, match="'ActedIn' has no property 'roel'\\. Did you mean 'role'\\?"
    ):
        _ = rel.roel


def test_dynamic_node_allows_arbitrary_attributes():
    # Schema-less dynamic node instance has empty _schema_fields
    raw_node = Node("n")
    assert raw_node._schema_fields == {}
    # Dynamic properties are synthesized into BoundField
    field = raw_node.custom_prop
    assert field.field_name == "custom_prop"
    assert field.target_alias == "n"


def test_schema_fields_inheritance_across_subclasses():
    class Employee(Person):
        department = Field()

    emp = Employee("e")
    # Inherited fields from Person
    assert emp.name is not None
    assert emp.age is not None
    # Own field
    assert emp.department is not None

    # Typo on inherited field
    with pytest.raises(
        AttributeError, match="'Employee' has no property 'nmae'\\. Did you mean 'name'\\?"
    ):
        _ = emp.nmae

    # Typo on own field
    with pytest.raises(
        AttributeError,
        match="'Employee' has no property 'departmnet'\\. Did you mean 'department'\\?",
    ):
        _ = emp.departmnet


def test_bound_field_hash_and_dict_set_usage():
    p = Person("p")
    u = User("u")

    # BoundField can be added to sets and dict keys
    fields_set = {p.name, p.age}
    assert p.name in fields_set
    assert p.age in fields_set
    assert u.username not in fields_set

    field_map = {p.name: "NameField", p.age: "AgeField"}
    assert field_map[p.name] == "NameField"
    assert field_map[p.age] == "AgeField"

    # Distinct BoundField instances with matching (target_alias, field_name) match in sets
    from voyager_ogm.models import BoundField

    f1 = BoundField("p", "name")
    f2 = BoundField("p", "name")
    f3 = BoundField("p", "age")
    f_diff_alias = BoundField("other", "name")

    s = {f1}
    assert f2 in s
    assert f3 not in s
    assert f_diff_alias not in s


def test_predicate_and_field_boolean_truthiness_guard():
    p = Person("p")
    u = User("u")

    # 1. Evaluating PredicateExpr in boolean context must raise TypeError
    pred = p.age == 25
    with pytest.raises(
        TypeError, match="Evaluating a PredicateExpr .* in a boolean context .* is not supported"
    ):
        if pred:
            pass

    with pytest.raises(
        TypeError, match="Evaluating a PredicateExpr .* in a boolean context .* is not supported"
    ):
        bool(p.name == "Alice")

    # 2. Evaluating BoundField directly in boolean context must raise TypeError
    with pytest.raises(
        TypeError, match="Evaluating a BoundField .* in a boolean context .* is not supported"
    ):
        if p.name:
            pass

    with pytest.raises(
        TypeError, match="Evaluating a BoundField .* in a boolean context .* is not supported"
    ):
        bool(p.age)

    # 3. Comparing two different fields returns PredicateExpr for queries
    cross_field_pred = p.name == u.username
    assert type(cross_field_pred).__name__ == "PredicateExpr"
    query = Query.match(p).node(u).where(p.name == u.username)
    compiled = query.compile("cypher")
    assert "WHERE p.name = u.username" in compiled.statement
