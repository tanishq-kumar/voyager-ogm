"""Comprehensive tests covering all Query builder methods and model descriptors."""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    Node,
    Path,
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


def test_query_with_clause_and_intermediate_pipeline():
    p = Person("p")

    query = (
        Query.match(p)
        .where(p.age > 21)
        .with_(p.name, p.age)
        .where(p.age < 50)
        .return_(p.name)
        .limit(10)
    )

    compiled = query.compile("cypher")
    assert (
        compiled.statement
        == "MATCH (p:Person) WHERE p.age > $p0 WITH p.name, p.age WHERE p.age < $p1 RETURN p.name LIMIT 10"
    )
    assert compiled.parameters == {"p0": 21, "p1": 50}

    # Distinct WITH with aliased projection
    q2 = Query.match(p).with_(p.name, distinct=True, user_age=p.age).return_("user_age")
    c2 = q2.compile("cypher")
    assert "WITH DISTINCT p.name, p.age AS user_age" in c2.statement


def test_query_hybrid_method_chaining():
    p = Person("p")
    c = Company("c")

    # UNWIND then CREATE
    q1 = Query.unwind("$batch", "row").create(p)
    c1 = q1.compile("cypher")
    assert "UNWIND $batch AS row" in c1.statement
    assert "CREATE (p:Person)" in c1.statement

    # MATCH then CREATE
    q2 = Query.match(p).create(c)
    c2 = q2.compile("cypher")
    assert "MATCH (p:Person)" in c2.statement
    assert "CREATE (c:Company)" in c2.statement

    # MATCH then MERGE
    q3 = Query.match(p).merge(c)
    c3 = q3.compile("cypher")
    assert "MATCH (p:Person)" in c3.statement
    assert "MERGE (c:Company)" in c3.statement

    # MATCH then OPTIONAL MATCH
    q4 = Query.match(p).optional_match(c)
    c4 = q4.compile("cypher")
    assert "MATCH (p:Person)" in c4.statement
    assert "OPTIONAL MATCH (c:Company)" in c4.statement

    # LOAD CSV then CREATE
    q5 = Query.load_csv("file:///data.csv", alias="row").create(p)
    c5 = q5.compile("cypher")
    assert "LOAD CSV WITH HEADERS FROM $p0 AS row" in c5.statement
    assert c5.parameters["p0"] == "file:///data.csv"
    assert "CREATE (p:Person)" in c5.statement


def test_query_input_validation_exceptions():
    p = Person("p")

    # Negative limit
    with pytest.raises(ValueError, match="limit count must be non-negative"):
        Query.match(p).limit(-1)

    # Negative skip
    with pytest.raises(ValueError, match="skip count must be non-negative"):
        Query.match(p).skip(-1)

    # Negative offset
    with pytest.raises(ValueError, match="offset count must be non-negative"):
        Query.match(p).offset(-5)

    # Empty where()
    with pytest.raises(ValueError, match="where\\(\\) requires at least one predicate condition"):
        Query.match(p).where()

    # Empty where_not()
    with pytest.raises(
        ValueError, match="where_not\\(\\) requires at least one predicate condition"
    ):
        Query.match(p).where_not()

    # Empty with_()
    with pytest.raises(ValueError, match="with_\\(\\) requires at least one projection field"):
        Query.match(p).with_()


def test_query_offset_alias():
    p = Person("p")
    query = Query.match(p).return_(p.name).offset(10).limit(5)
    compiled = query.compile("cypher")
    assert "SKIP 10 LIMIT 5" in compiled.statement


def test_query_subquery_static_methods():
    p = Person("p")
    c = Company("c")

    sub = Query.match(c).where(c.name == p.name)
    exists_expr = Query.exists(sub)
    assert exists_expr.kind == "exists"

    count_expr = Query.count(sub)
    assert count_expr.kind == "count"

    # In a where predicate
    query = Query.match(p).where(Query.exists(sub)).return_(p.name)
    compiled = query.compile("cypher")
    assert "WHERE EXISTS { MATCH (c:Company)" in compiled.statement


def test_query_match_patterns_and_path():
    p = Person("p")
    c = Company("c")

    p1 = Path.match(p).to("WORKS_AT").node(c)
    p2 = Path.match(p).to("MANAGES").node(c)

    query = Query.match_patterns(p1, p2).where(c.name == "Acme").return_(p.name)
    compiled = query.compile("cypher")
    assert "MATCH (p:Person)-[:WORKS_AT]->(c:Company), (p)-[:MANAGES]->(c)" in compiled.statement
    assert "WHERE c.name = $p0" in compiled.statement
    assert "RETURN p.name" in compiled.statement


def test_query_subquery_with_and_projections_roundtrip():
    p = Person("p")
    c = Company("c")

    sub = (
        Query.match(c)
        .with_(c.name)
        .where(c.name == "Acme")
        .return_(c.name)
        .order_by(c.name)
        .limit(1)
    )
    spec = sub.to_spec()
    assert "with_clauses" in spec
    assert len(spec["with_clauses"]) == 1
    assert spec["with_clauses"][0]["projections"] == [("field", "c", "name", None)]
    assert len(spec["with_clauses"][0]["where"]) == 1
    assert spec["projections"] == [("field", "c", "name", None)]
    assert spec["limit"] == 1

    query = Query.match(p).where(Query.exists(sub)).return_(p.name)
    compiled = query.compile("cypher")
    assert "EXISTS {" in compiled.statement
    assert "WITH c.name" in compiled.statement
    assert "WHERE c.name = $p0" in compiled.statement
    assert "RETURN c.name" in compiled.statement
    assert "LIMIT 1" in compiled.statement


def test_query_match_patterns_multi_path():
    a = Person("a")
    b = Person("b")
    c = Person("c")
    d = Person("d")

    p = Path.match(a).to("KNOWS").node(b).pattern().node(c).to("KNOWS").node(d)
    query = Query.match_patterns(p).return_(a.name, d.name)
    compiled = query.compile("cypher")
    assert (
        compiled.statement
        == "MATCH (a:Person)-[:KNOWS]->(b:Person), (c:Person)-[:KNOWS]->(d:Person) RETURN a.name, d.name"
    )


def test_query_project_field_as_without_dot():
    p = Person("p")
    query = Query.match(p).return_("name AS n")
    compiled = query.compile("cypher")
    assert "RETURN name AS n" in compiled.statement


def test_query_unknown_aggregation_raises():
    from voyager_ogm._voyager_rs import compile_query_from_spec

    spec = {
        "matches": [
            {
                "optional": False,
                "paths": [[("node", "p", ["Person"])]],
                "where": [],
            }
        ],
        "projections": [("agg", "p", "age", "unsupported_agg_func", None)],
    }
    with pytest.raises(ValueError, match="Unknown aggregation function"):
        compile_query_from_spec(spec, "cypher")


def test_query_with_where_before_order_by():
    p = Person("p")
    query = (
        Query.match(p)
        .with_(p.name, p.age)
        .where(p.age > 21)
        .order_by(p.age)
        .limit(5)
        .return_(p.name)
    )
    compiled = query.compile("cypher")
    assert (
        "WITH p.name, p.age WHERE p.age > $p0 ORDER BY p.age ASC LIMIT 5 RETURN p.name"
        in compiled.statement
    )
