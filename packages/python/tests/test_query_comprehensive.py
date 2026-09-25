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


def test_query_multi_clause_match_optional_match_to_spec():
    from voyager_ogm._voyager_rs import compile_query_from_spec

    p = Person("p")
    c = Company("c")

    q = Query.match(p).where(p.age > 20).optional_match(c).where(c.name == "Acme")
    spec = q.to_spec()
    assert "matches" in spec
    assert len(spec["matches"]) == 2

    # Verify first clause is not optional and scoped with its own predicate
    assert spec["matches"][0]["optional"] is False
    assert spec["matches"][0]["paths"] == [[("node", "p", ["Person"])]]
    assert len(spec["matches"][0]["where"]) == 1

    # Verify second clause is optional and scoped with its own predicate
    assert spec["matches"][1]["optional"] is True
    assert spec["matches"][1]["paths"] == [[("node", "c", ["Company"])]]
    assert len(spec["matches"][1]["where"]) == 1

    # Compile via compile_query_from_spec and verify OPTIONAL does not bleed
    compiled_spec = compile_query_from_spec(spec, "cypher")
    statement = compiled_spec["statement"]
    assert "MATCH (p:Person)" in statement
    assert "OPTIONAL MATCH (p:Person)" not in statement
    assert "OPTIONAL MATCH (c:Company)" in statement
    assert "WHERE p.age > $p0" in statement
    assert "WHERE c.name = $p1" in statement

    # Verify existential subquery with MATCH then OPTIONAL MATCH does not bleed OPTIONAL
    sub = Query.match(p).optional_match(c)
    outer = Query.match(p).where(Query.exists(sub)).return_(p.name)
    compiled_outer = outer.compile("cypher")
    assert "EXISTS { MATCH (p:Person) OPTIONAL MATCH (c:Company) }" in compiled_outer.statement


def test_query_to_spec_mutations_roundtrip():
    from voyager_ogm._voyager_rs import compile_query_from_spec

    p = Person("p")
    c = Company("c")

    # MATCH then CREATE
    q_create = Query.match(p).create(c)
    spec_create = q_create.to_spec()
    assert "matches" in spec_create and len(spec_create["matches"]) == 1
    assert "mutations" in spec_create
    assert spec_create["mutations"] == [("create", [[("node", "c", ["Company"])]])]
    res_create = compile_query_from_spec(spec_create, "cypher")
    assert res_create["statement"] == "MATCH (p:Person) CREATE (c:Company)"

    # Standalone CREATE
    q_pure_create = Query.create(p)
    spec_pure_create = q_pure_create.to_spec()
    assert spec_pure_create["matches"] == []
    assert spec_pure_create["mutations"] == [("create", [[("node", "p", ["Person"])]])]
    res_pure_create = compile_query_from_spec(spec_pure_create, "cypher")
    assert res_pure_create["statement"] == "CREATE (p:Person)"

    # MATCH then MERGE with ON CREATE SET and ON MATCH SET
    q_merge = (
        Query.match(p).merge(c).on_create_set(c.name == "Acme").on_match_set(c.name == "AcmeCorp")
    )
    spec_merge = q_merge.to_spec()
    assert "mutations" in spec_merge
    assert len(spec_merge["mutations"]) == 1
    assert spec_merge["mutations"][0][0] == "merge"
    assert spec_merge["mutations"][0][1] == [("node", "c", ["Company"])]
    assert spec_merge["mutations"][0][2] == [("c", "name", ("lit", "Acme"))]
    assert spec_merge["mutations"][0][3] == [("c", "name", ("lit", "AcmeCorp"))]
    res_merge = compile_query_from_spec(spec_merge, "cypher")
    assert "MATCH (p:Person) MERGE (c:Company)" in res_merge["statement"]
    assert "ON CREATE SET c.name = $p0" in res_merge["statement"]
    assert "ON MATCH SET c.name = $p1" in res_merge["statement"]

    # SET and DELETE
    q_set_del = Query.match(p).set(p.score == 99).delete(p)
    spec_set_del = q_set_del.to_spec()
    assert ("set", "p", "score", ("lit", 99)) in spec_set_del["mutations"]
    assert ("delete", False, ["p"]) in spec_set_del["mutations"]
    res_set_del = compile_query_from_spec(spec_set_del, "cypher")
    assert res_set_del["statement"] == "MATCH (p:Person) SET p.score = $p0 DELETE p"

    # DETACH DELETE
    q_detach = Query.match(p).detach_delete(p)
    spec_detach = q_detach.to_spec()
    assert ("delete", True, ["p"]) in spec_detach["mutations"]
    res_detach = compile_query_from_spec(spec_detach, "cypher")
    assert res_detach["statement"] == "MATCH (p:Person) DETACH DELETE p"

    # REMOVE
    q_remove = Query.match(p).remove(p.score)
    spec_remove = q_remove.to_spec()
    assert ("remove", "p", "score") in spec_remove["mutations"]
    res_remove = compile_query_from_spec(spec_remove, "cypher")
    assert res_remove["statement"] == "MATCH (p:Person) REMOVE p.score"


def test_query_subquery_rejects_mutations():
    p = Person("p")
    c = Company("c")

    # EXISTS rejects CREATE
    with pytest.raises(
        ValueError, match="Subqueries do not support mutating clauses \\(CREATE/MERGE/SET/DELETE\\)"
    ):
        Query.exists(Query.match(p).create(c))

    # COUNT rejects CREATE
    with pytest.raises(
        ValueError, match="Subqueries do not support mutating clauses \\(CREATE/MERGE/SET/DELETE\\)"
    ):
        Query.count(Query.match(p).create(c))

    # Subquery with MERGE
    with pytest.raises(
        ValueError, match="Subqueries do not support mutating clauses \\(CREATE/MERGE/SET/DELETE\\)"
    ):
        Query.exists(Query.match(p).merge(c))

    # Subquery with SET
    with pytest.raises(
        ValueError, match="Subqueries do not support mutating clauses \\(CREATE/MERGE/SET/DELETE\\)"
    ):
        Query.exists(Query.match(p).set(p.score == 50))

    # Subquery with DELETE
    with pytest.raises(
        ValueError, match="Subqueries do not support mutating clauses \\(CREATE/MERGE/SET/DELETE\\)"
    ):
        Query.exists(Query.match(p).delete(p))

    # Subquery with DETACH DELETE
    with pytest.raises(
        ValueError, match="Subqueries do not support mutating clauses \\(CREATE/MERGE/SET/DELETE\\)"
    ):
        Query.exists(Query.match(p).detach_delete(p))

    # Subquery with REMOVE
    with pytest.raises(
        ValueError, match="Subqueries do not support mutating clauses \\(CREATE/MERGE/SET/DELETE\\)"
    ):
        Query.exists(Query.match(p).remove(p.score))


def test_query_hybrid_chaining_path_boundaries():
    p = Person("p")
    c = Company("c")

    # q.match(p).match(c) should produce separate paths matching add_match
    q_match = Query.match(p).match(c)
    assert len(q_match._current_paths) == 2
    assert q_match._current_paths[0] == [("node", "p", ["Person"])]
    assert q_match._current_paths[1] == [("node", "c", ["Company"])]
    assert q_match.compile("cypher").statement == "MATCH (p:Person) MATCH (c:Company)"

    q_add_match = Query.match(p).add_match(c)
    assert q_add_match._current_paths == q_match._current_paths
    assert q_add_match.compile("cypher").statement == q_match.compile("cypher").statement

    # q.match(p).optional_match(c) should produce separate paths matching add_optional_match
    q_opt = Query.match(p).optional_match(c)
    assert len(q_opt._current_paths) == 2
    assert q_opt._current_paths[0] == [("node", "p", ["Person"])]
    assert q_opt._current_paths[1] == [("node", "c", ["Company"])]
    assert q_opt.compile("cypher").statement == "MATCH (p:Person) OPTIONAL MATCH (c:Company)"

    q_add_opt = Query.match(p).add_optional_match(c)
    assert q_add_opt._current_paths == q_opt._current_paths
    assert q_add_opt.compile("cypher").statement == q_opt.compile("cypher").statement

    # q.match(p).create(c) should produce separate paths matching add_create
    q_create = Query.match(p).create(c)
    assert len(q_create._current_paths) == 2
    assert q_create._current_paths[0] == [("node", "p", ["Person"])]
    assert q_create._current_paths[1] == [("node", "c", ["Company"])]
    assert q_create.compile("cypher").statement == "MATCH (p:Person) CREATE (c:Company)"

    q_add_create = Query.match(p).add_create(c)
    assert q_add_create._current_paths == q_create._current_paths
    assert q_add_create.compile("cypher").statement == q_create.compile("cypher").statement

    # q.match(p).merge(c) should produce separate paths matching add_merge
    q_merge = Query.match(p).merge(c)
    assert len(q_merge._current_paths) == 2
    assert q_merge._current_paths[0] == [("node", "p", ["Person"])]
    assert q_merge._current_paths[1] == [("node", "c", ["Company"])]
    assert q_merge.compile("cypher").statement == "MATCH (p:Person) MERGE (c:Company)"

    q_add_merge = Query.match(p).add_merge(c)
    assert q_add_merge._current_paths == q_merge._current_paths
    assert q_add_merge.compile("cypher").statement == q_merge.compile("cypher").statement


def test_query_has_mutations_native():
    p = Person("p")

    # Read queries
    q_read = Query.match(p).where(p.age > 21).return_(p.name)
    assert not q_read.has_mutations()
    assert not q_read._native.has_mutations()

    # Create
    q_create = Query.create(p)
    assert q_create.has_mutations()
    assert q_create._native.has_mutations()

    # Merge
    q_merge = Query.merge(p)
    assert q_merge.has_mutations()
    assert q_merge._native.has_mutations()

    # Set
    q_set = Query.match(p).set(p.name == "Alice")
    assert q_set.has_mutations()
    assert q_set._native.has_mutations()

    # Delete
    q_delete = Query.match(p).delete(p)
    assert q_delete.has_mutations()
    assert q_delete._native.has_mutations()

    # Detach Delete
    q_detach = Query.match(p).detach_delete(p)
    assert q_detach.has_mutations()
    assert q_detach._native.has_mutations()

    # Remove
    q_remove = Query.match(p).remove(p.age)
    assert q_remove.has_mutations()
    assert q_remove._native.has_mutations()


def test_aliased_expr_to_spec_preserves_alias():
    p = Person("p")

    aliased = p.name.as_("full_name")
    spec = aliased.to_spec()
    assert spec == ("alias", ("prop", "p", "name"), "full_name")

    # Roundtrip in Query projection
    query = Query.match(p).return_(aliased)
    compiled = query.compile("cypher")
    assert "RETURN p.name AS full_name" in compiled.statement

    # Subquery WITH projection roundtrip via to_spec
    q_with = Query.match(p).with_(aliased).return_("full_name")
    assert "WITH p.name AS full_name" in q_with.compile("cypher").statement


def test_fn_count_and_exists_decoupled_from_lazy_import():
    from voyager_ogm import fn

    p = Person("p")
    c = Company("c")

    # fn.count on field produces function call
    agg = fn.count(p.name)
    assert repr(agg) == "count(p.name)"
    assert agg.to_spec() == ("fn", "count", [("prop", "p", "name")])

    # fn.count on subquery produces scalar subquery
    sub = Query.match(c).where(c.name == "Acme")
    scalar_count = fn.count(sub)
    assert repr(scalar_count) == f"COUNT {{ {sub!r} }}"
    assert scalar_count.to_spec() == ("count", sub.to_spec())

    # fn.exists on subquery produces existential subquery
    exist_expr = fn.exists(sub)
    assert repr(exist_expr) == f"EXISTS {{ {sub!r} }}"
    assert exist_expr.to_spec() == ("exists", sub.to_spec())

    # Mutating queries raise ValueError on exists and count
    mut_q = Query.match(p).create(c)
    with pytest.raises(ValueError, match="Subqueries do not support mutating clauses"):
        fn.exists(mut_q)
    with pytest.raises(ValueError, match="Subqueries do not support mutating clauses"):
        fn.count(mut_q)


def test_query_match_patterns_native_composition():
    p = Person("p")
    c = Company("c")

    p1 = Path.match(p).to("WORKS_AT").node(c)
    p2 = Path.match(p).to("MANAGES").node(c)

    query = Query.match_patterns(p1, p2).where(c.name == "Acme").return_(p.name)
    compiled = query.compile("cypher")
    assert (
        compiled.statement
        == "MATCH (p:Person)-[:WORKS_AT]->(c:Company), (p)-[:MANAGES]->(c) WHERE c.name = $p0 RETURN p.name"
    )

    # Reject mutating queries in match_patterns
    mut_path = Path.create(p)
    with pytest.raises(ValueError, match="Cannot import mutating query into MATCH clause"):
        Query.match_patterns(mut_path)
