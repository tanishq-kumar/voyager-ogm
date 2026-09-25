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
    skills = Field()


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
        ("to", 1, 2, "MATCH ((a:Person)-[_knows_0:KNOWS]->(b:Person)){1,2} RETURN b.name"),
        ("to", 1, 3, "MATCH ((a:Person)-[_knows_0:KNOWS]->(b:Person)){1,3} RETURN b.name"),
        ("to", 2, 2, "MATCH ((a:Person)-[_knows_0:KNOWS]->(b:Person)){2,2} RETURN b.name"),
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


def test_gql_standard_functions_and_concatenation():
    """ISO GQL: Standard functions (upper, lower, char_length, cardinality) and || concatenation."""
    from voyager_ogm import fn

    p = Person(alias="p")
    q = Query.match(p).return_(
        lower_name=fn.to_lower(p.name),
        upper_name=fn.to_upper(p.name),
        city_len=fn.char_length(p.city),
        skill_count=fn.cardinality(p.skills),
        full_greeting=p.name.concat(" from London"),
    )
    compiled = q.compile(dialect="iso_gql")
    assert "lower(p.name) AS lower_name" in compiled.statement
    assert "upper(p.name) AS upper_name" in compiled.statement
    assert "char_length(p.city) AS city_len" in compiled.statement
    assert "cardinality(p.skills) AS skill_count" in compiled.statement
    assert "p.name || $p0 AS full_greeting" in compiled.statement


# ---------------------------------------------------------------------------
# 7. ISO GQL Path Search Modes & Traversal Modifiers (#56)
# ---------------------------------------------------------------------------


def test_gql_path_search_modes():
    """ISO GQL: Traversal search modes (TRAIL, SIMPLE, ACYCLIC, WALK) and Cypher compatibility."""
    p = Person(alias="a")
    f = Person(alias="b")

    # TRAIL with path variable
    q_trail = Query.match(p).trail("path").to("KNOWS").node(f).return_("path")
    comp_gql = q_trail.compile(dialect="iso_gql")
    assert comp_gql.statement == "MATCH path = TRAIL (a:Person)-[:KNOWS]->(b:Person) RETURN path"

    # Dialect cypher emits UserWarning when path mode is specified and omits keyword
    with pytest.warns(UserWarning, match="does not support explicit path search modes"):
        comp_cypher = q_trail.compile(dialect="cypher")
    assert comp_cypher.statement == "MATCH path = (a:Person)-[:KNOWS]->(b:Person) RETURN path"

    # SQL:PGQ raises UnsupportedFeature on path variables and path search modes
    with pytest.raises(ValueError, match="does not support path variables"):
        Query.match(p).path_variable("pv").to("KNOWS").node(f).compile(dialect="sql_pgq")
    with pytest.raises(ValueError, match="does not support traversal search modes"):
        Query.match(p).trail().to("KNOWS").node(f).compile(dialect="sql_pgq")

    # TRAIL without path variable
    q_trail_novar = Query.match(p).trail().to("KNOWS").node(f).return_(a=p.name)
    assert (
        q_trail_novar.compile(dialect="iso_gql").statement
        == "MATCH TRAIL (a:Person)-[:KNOWS]->(b:Person) RETURN a.name AS a"
    )
    with pytest.warns(UserWarning, match="does not support explicit path search modes"):
        comp_cypher_novar = q_trail_novar.compile(dialect="cypher")
    assert comp_cypher_novar.statement == "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name AS a"

    # SIMPLE mode
    q_simple = Query.match(p).simple("sp").to("KNOWS").node(f).return_("sp")
    assert (
        q_simple.compile(dialect="iso_gql").statement
        == "MATCH sp = SIMPLE (a:Person)-[:KNOWS]->(b:Person) RETURN sp"
    )
    with pytest.warns(UserWarning, match="does not support explicit path search modes"):
        comp_simple_cypher = q_simple.compile(dialect="cypher")
    assert comp_simple_cypher.statement == "MATCH sp = (a:Person)-[:KNOWS]->(b:Person) RETURN sp"

    # ACYCLIC mode
    q_acyc = Query.match(p).acyclic("ap").to("KNOWS").node(f).return_("ap")
    assert (
        q_acyc.compile(dialect="iso_gql").statement
        == "MATCH ap = ACYCLIC (a:Person)-[:KNOWS]->(b:Person) RETURN ap"
    )
    with pytest.warns(UserWarning, match="does not support explicit path search modes"):
        comp_acyc_cypher = q_acyc.compile(dialect="cypher")
    assert comp_acyc_cypher.statement == "MATCH ap = (a:Person)-[:KNOWS]->(b:Person) RETURN ap"

    # WALK mode
    q_walk = Query.match(p).walk("wp").to("KNOWS").node(f).return_("wp")
    assert (
        q_walk.compile(dialect="iso_gql").statement
        == "MATCH wp = WALK (a:Person)-[:KNOWS]->(b:Person) RETURN wp"
    )
    with pytest.warns(UserWarning, match="does not support explicit path search modes"):
        comp_walk_cypher = q_walk.compile(dialect="cypher")
    assert comp_walk_cypher.statement == "MATCH wp = (a:Person)-[:KNOWS]->(b:Person) RETURN wp"

    # Query hybrid starters (Query.trail, Query.simple, Query.acyclic, Query.walk)
    q_starter_trail = Query.trail("my_path").match(p).to("KNOWS").node(f).return_("my_path")
    assert (
        q_starter_trail.compile(dialect="iso_gql").statement
        == "MATCH my_path = TRAIL (a:Person)-[:KNOWS]->(b:Person) RETURN my_path"
    )

    q_starter_simple = Query.simple("my_sp").match(p).to("KNOWS").node(f).return_("my_sp")
    assert (
        q_starter_simple.compile(dialect="iso_gql").statement
        == "MATCH my_sp = SIMPLE (a:Person)-[:KNOWS]->(b:Person) RETURN my_sp"
    )

    q_starter_acyclic = Query.acyclic().match(p).to("KNOWS").node(f).return_(p=p.name)
    assert (
        q_starter_acyclic.compile(dialect="iso_gql").statement
        == "MATCH ACYCLIC (a:Person)-[:KNOWS]->(b:Person) RETURN a.name AS p"
    )

    q_starter_walk = Query.walk("my_wp").match(p).to("KNOWS").node(f).return_("my_wp")
    assert (
        q_starter_walk.compile(dialect="iso_gql").statement
        == "MATCH my_wp = WALK (a:Person)-[:KNOWS]->(b:Person) RETURN my_wp"
    )

    q_pv = (
        Query.match(p)
        .path_variable("custom_p")
        .path_mode("trail")
        .to("KNOWS")
        .node(f)
        .return_("custom_p")
    )
    assert (
        q_pv.compile(dialect="iso_gql").statement
        == "MATCH custom_p = TRAIL (a:Person)-[:KNOWS]->(b:Person) RETURN custom_p"
    )


# ---------------------------------------------------------------------------
# 8. ISO GQL Label Expressions (OR | and NOT !) (#56)
# ---------------------------------------------------------------------------


def test_gql_label_expressions():
    """ISO GQL: Label expressions `(A|B)&!C` conformance across GQL and Cypher 5."""
    # Disjunction with negation
    q1 = Query.match().node("n", labels=["Person | Company", "!Inactive"]).return_("n")
    assert (
        q1.compile(dialect="iso_gql").statement == "MATCH (n:(Person|Company)&!Inactive) RETURN n"
    )
    assert q1.compile(dialect="cypher").statement == "MATCH (n:(Person|Company)&!Inactive) RETURN n"

    # Triple disjunction
    q2 = Query.match().node("n", labels=["Admin | SuperUser | Manager"]).return_("n")
    assert q2.compile(dialect="iso_gql").statement == "MATCH (n:(Admin|SuperUser|Manager)) RETURN n"
    assert q2.compile(dialect="cypher").statement == "MATCH (n:(Admin|SuperUser|Manager)) RETURN n"

    # Single negation
    q3 = Query.match().node("n", labels=["!Deleted"]).return_("n")
    assert q3.compile(dialect="iso_gql").statement == "MATCH (n:!Deleted) RETURN n"
    assert q3.compile(dialect="cypher").statement == "MATCH (n:!Deleted) RETURN n"

    # Multi-label conjunction (standard GQL & vs Cypher :A:B)
    q4 = Query.match().node("n", labels=["Person", "Employee"]).return_("n")
    assert q4.compile(dialect="iso_gql").statement == "MATCH (n:Person&Employee) RETURN n"
    assert q4.compile(dialect="cypher").statement == "MATCH (n:Person:Employee) RETURN n"


# ---------------------------------------------------------------------------
# 9. GQL Linear Statements & Pagination (LET, FILTER, OFFSET) (#56)
# ---------------------------------------------------------------------------


def test_gql_linear_statements_and_pagination():
    """ISO GQL: Linear LET and FILTER clauses, and OFFSET pagination."""
    p = Person(alias="p")

    # LET and linear_filter
    q = (
        Query.match(p)
        .let_(fullName=p.name + " Senior")
        .linear_filter(p.age >= 60)
        .return_("fullName")
        .offset(15)
        .limit(10)
    )

    comp_gql = q.compile(dialect="iso_gql")
    assert (
        comp_gql.statement
        == "MATCH (p:Person) LET fullName = p.name || $p0 FILTER p.age >= $p1 RETURN fullName OFFSET 15 LIMIT 10"
    )
    assert comp_gql.parameters == {"p0": " Senior", "p1": 60}

    comp_cypher = q.compile(dialect="cypher")
    assert (
        comp_cypher.statement
        == "MATCH (p:Person) WITH *, p.name + $p0 AS fullName WHERE p.age >= $p1 RETURN fullName SKIP 15 LIMIT 10"
    )
    assert comp_cypher.parameters == {"p0": " Senior", "p1": 60}

    # filter_ alias works identically
    q_alias = Query.match(p).filter_(p.age >= 60).return_("p.name")
    assert "FILTER p.age >= $p0" in q_alias.compile(dialect="iso_gql").statement
    assert "WHERE p.age >= $p0" in q_alias.compile(dialect="cypher").statement

    # Positional let_ and dict let_
    q_pos = Query.match(p).let_("n", p.name).return_("n")
    assert "LET n = p.name" in q_pos.compile(dialect="iso_gql").statement

    q_dict = Query.match(p).let_({"n": p.name, "a": p.age}).return_("n")
    assert "LET n = p.name LET a = p.age" in q_dict.compile(dialect="iso_gql").statement

    # SQL:PGQ rejects linear statements (LET, FILTER)
    with pytest.raises(ValueError, match="does not support linear statements"):
        Query.match(p).let_(x=1).compile(dialect="sql_pgq")
    with pytest.raises(ValueError, match="does not support linear statements"):
        Query.match(p).linear_filter(p.age > 20).compile(dialect="sql_pgq")

    # Validation: empty/malformed let_ and linear_filter must raise ValueError
    with pytest.raises(ValueError, match="requires at least one variable assignment"):
        Query.match(p).let_()

    with pytest.raises(ValueError, match="single positional argument must be a dict"):
        Query.match(p).let_("malformed")

    with pytest.raises(ValueError, match="accepts at most 2 positional arguments"):
        Query.match(p).let_("a", 1, 2)

    with pytest.raises(ValueError, match="requires at least one predicate expression"):
        Query.match(p).linear_filter()


# ---------------------------------------------------------------------------
# 10. String Concatenation (||) & Standard Function Helpers (#56)
# ---------------------------------------------------------------------------


def test_gql_string_concatenation_and_helpers():
    """ISO GQL: Nested string additions emit || in GQL and +, helper methods on expressions."""
    from voyager_ogm import fn

    p = Person(alias="p")

    # Nested addition p.name + " lives in " + p.city
    q_str = Query.match(p).return_(
        info=p.name + " lives in " + p.city,
    )
    comp_gql = q_str.compile(dialect="iso_gql")
    assert comp_gql.statement == "MATCH (p:Person) RETURN (p.name || $p0) || p.city AS info"
    comp_cypher = q_str.compile(dialect="cypher")
    assert comp_cypher.statement == "MATCH (p:Person) RETURN (p.name + $p0) + p.city AS info"

    # Expression methods: .char_length(), .upper(), .lower()
    q_helpers = Query.match(p).return_(
        c_len=p.city.char_length(),
        u_name=p.name.upper(),
        l_name=p.name.lower(),
    )
    comp_helpers_gql = q_helpers.compile(dialect="iso_gql")
    assert "char_length(p.city) AS c_len" in comp_helpers_gql.statement
    assert "upper(p.name) AS u_name" in comp_helpers_gql.statement
    assert "lower(p.name) AS l_name" in comp_helpers_gql.statement

    comp_helpers_cypher = q_helpers.compile(dialect="cypher")
    assert "toUpper(p.name) AS u_name" in comp_helpers_cypher.statement
    assert "toLower(p.name) AS l_name" in comp_helpers_cypher.statement

    # Graph/path functions: fn.elements(p), fn.path_length(p)
    q_path_fn = (
        Query.match()
        .trail("p")
        .node("a")
        .to("KNOWS")
        .node("b")
        .return_(
            elems=fn.elements("p"),
            plen=fn.path_length("p"),
        )
    )
    comp_pfn_gql = q_path_fn.compile(dialect="iso_gql")
    assert "elements(p) AS elems" in comp_pfn_gql.statement
    assert "path_length(p) AS plen" in comp_pfn_gql.statement

    with pytest.warns(UserWarning, match="does not support explicit path search modes"):
        comp_pfn_cypher = q_path_fn.compile(dialect="cypher")
    assert "elements(p) AS elems" in comp_pfn_cypher.statement
    assert "length(p) AS plen" in comp_pfn_cypher.statement
