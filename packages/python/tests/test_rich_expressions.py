"""Comprehensive tests for Voyager OGM Phase 4A Rich Expression Engine.

Tests AST expressions, operator overloads, string/scalar/temporal functions,
CASE WHEN conditionals, comprehensions, subqueries, and multi-pattern matching.
"""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    case,
    fn,
    ident,
    list_comprehension,
    pattern_comprehension,
    reset_alias_counters,
)
from voyager_ogm.expressions import (
    BinaryExpr,
    CaseExpr,
    ListCompExpr,
    PatternCompExpr,
    UnaryExpr,
)


class Person(Node):
    __label__ = "Person"
    name: str = Field(index=True)
    age: int = Field(default=0)
    salary: float = Field(default=0.0)
    city: str = Field(default="NYC")
    skills: list[str] = Field(default_factory=list)
    created_at: str = Field(default="2026-01-01")


class Movie(Node):
    __label__ = "Movie"
    title: str = Field(index=True)
    released: int = Field(default=2000)
    rating: float = Field(default=5.0)


class ActedIn(Relationship):
    __edge_type__ = "ACTED_IN"
    role: str = Field(default="Actor")


class Directed(Relationship):
    __edge_type__ = "DIRECTED"


@pytest.fixture(autouse=True)
def _clean_state() -> None:
    reset_alias_counters()


def test_arithmetic_operator_expressions() -> None:
    p = Person("p")
    # Math expressions
    expr_add = p.age + 5
    expr_sub = p.age - 2
    expr_mul = p.salary * 1.1
    expr_div = p.salary / 12
    expr_mod = p.age % 10
    expr_neg = -p.age

    assert isinstance(expr_add, BinaryExpr)
    assert expr_add.op == "+"
    assert isinstance(expr_sub, BinaryExpr)
    assert expr_sub.op == "-"
    assert isinstance(expr_mul, BinaryExpr)
    assert expr_mul.op == "*"
    assert isinstance(expr_div, BinaryExpr)
    assert expr_div.op == "/"
    assert isinstance(expr_mod, BinaryExpr)
    assert expr_mod.op == "%"
    assert isinstance(expr_neg, UnaryExpr)
    assert expr_neg.op == "-"

    # Compound arithmetic in RETURN clause
    q = Query.match(p).return_(p.name, (p.salary * 1.15 + 500).as_("adjusted_salary"))
    compiled = q.compile("cypher")
    assert "MATCH (p:Person)" in compiled.statement
    assert "RETURN p.name, (p.salary * $p0) + $p1 AS adjusted_salary" in compiled.statement
    assert compiled.parameters["p0"] == 1.15
    assert compiled.parameters["p1"] == 500


def test_boolean_and_bitwise_operators() -> None:
    p = Person("p")
    cond_and = (p.age >= 18) & (p.city == "London")
    cond_or = (p.city == "NYC") | (p.city == "SF")
    cond_not = ~(p.age < 21)
    cond_xor = (p.age > 30) ^ (p.salary > 100000)

    assert isinstance(cond_and, BinaryExpr)
    assert cond_and.op == "AND"
    assert isinstance(cond_or, BinaryExpr)
    assert cond_or.op == "OR"
    assert isinstance(cond_not, UnaryExpr)
    assert cond_not.op == "NOT"
    assert isinstance(cond_xor, BinaryExpr)
    assert cond_xor.op == "XOR"

    q = Query.match(p).where(cond_and).return_(p.name)
    compiled = q.compile("cypher")
    assert "WHERE (p.age >= $p0) AND (p.city = $p1)" in compiled.statement
    assert compiled.parameters["p0"] == 18
    assert compiled.parameters["p1"] == "London"


def test_null_checks_and_unary_expressions() -> None:
    p = Person("p")
    null_check = p.city.is_null()
    not_null_check = p.skills.is_not_null()

    assert isinstance(null_check, UnaryExpr)
    assert null_check.op == "IS NULL"
    assert isinstance(not_null_check, UnaryExpr)
    assert not_null_check.op == "IS NOT NULL"

    q = Query.match(p).where(p.city.is_not_null()).return_(p.name)
    compiled = q.compile("cypher")
    assert "WHERE p.city IS NOT NULL" in compiled.statement


def test_string_and_scalar_functions() -> None:
    p = Person("p")
    q = (
        Query.match(p)
        .where(fn.to_lower(p.name) == "alice")
        .return_(
            fn.to_upper(p.name).as_("upper_name"),
            fn.trim(p.city).as_("trimmed_city"),
            fn.size(p.skills).as_("skills_count"),
            fn.coalesce(p.city, "Unknown").as_("safe_city"),
        )
    )
    compiled = q.compile("cypher")
    assert "WHERE toLower(p.name) = $p0" in compiled.statement
    assert "toUpper(p.name) AS upper_name" in compiled.statement
    assert "trim(p.city) AS trimmed_city" in compiled.statement
    assert "size(p.skills) AS skills_count" in compiled.statement
    assert "coalesce(p.city, $p1) AS safe_city" in compiled.statement
    assert compiled.parameters["p0"] == "alice"
    assert compiled.parameters["p1"] == "Unknown"


def test_temporal_functions() -> None:
    p = Person("p")
    q = Query.match(p).return_(
        fn.datetime().as_("now"),
        fn.date_trunc("day", p.created_at).as_("trunc_created"),
        fn.duration("P14D").as_("window"),
    )
    compiled = q.compile("cypher")
    assert "datetime() AS now" in compiled.statement
    assert "date.truncate($p0, p.created_at) AS trunc_created" in compiled.statement
    assert "duration($p1) AS window" in compiled.statement
    assert compiled.parameters["p0"] == "day"
    assert compiled.parameters["p1"] == "P14D"


def test_case_when_conditional_expression() -> None:
    p = Person("p")
    tier_expr = case().when(p.age < 18, "Youth").when(p.age < 65, "Adult").else_("Senior")
    assert isinstance(tier_expr, CaseExpr)

    q = Query.match(p).return_(p.name, tier_expr.as_("age_tier"))
    compiled = q.compile("cypher")
    assert (
        "CASE WHEN p.age < $p0 THEN $p1 WHEN p.age < $p2 THEN $p3 ELSE $p4 END AS age_tier"
        in compiled.statement
    )
    assert compiled.parameters["p0"] == 18
    assert compiled.parameters["p1"] == "Youth"
    assert compiled.parameters["p2"] == 65
    assert compiled.parameters["p3"] == "Adult"
    assert compiled.parameters["p4"] == "Senior"


def test_list_comprehension() -> None:
    p = Person("p")
    x = ident("x")
    comp = list_comprehension(
        var="x",
        list_expr=p.skills,
        where_filter=(x != "Legacy"),
        map_expr=fn.to_upper(x),
    )
    assert isinstance(comp, ListCompExpr)

    q = Query.match(p).return_(p.name, comp.as_("clean_skills"))
    compiled = q.compile("cypher")
    assert "[x IN p.skills WHERE x != $p0 | toUpper(x)] AS clean_skills" in compiled.statement
    assert compiled.parameters["p0"] == "Legacy"


def test_pattern_comprehension() -> None:
    p = Person("p")
    m = Movie("m")
    comp = pattern_comprehension(
        path=Query.match(p).to(ActedIn).node(m),
        proj=m.title,
        where_filter=(m.released >= 2010),
    )
    assert isinstance(comp, PatternCompExpr)

    q = Query.match(p).return_(p.name, comp.as_("recent_movies"))
    compiled = q.compile("cypher")
    assert "WHERE m.released >= $p0 | m.title] AS recent_movies" in compiled.statement
    assert compiled.parameters["p0"] == 2010


def test_existential_and_count_subqueries() -> None:
    p = Person("p")
    m = Movie("m")

    # Existential subquery
    q_exists = (
        Query.match(p)
        .where(fn.exists(Query.match(p).to(ActedIn).node(m).where(m.rating >= 8.5)))
        .return_(p.name)
    )
    compiled_exists = q_exists.compile("cypher")
    assert "MATCH (p:Person)" in compiled_exists.statement
    assert "WHERE EXISTS { MATCH (p:Person)" in compiled_exists.statement
    assert "WHERE m.rating >= $p0 }" in compiled_exists.statement
    assert compiled_exists.parameters["p0"] == 8.5

    # Scalar count subquery
    q_count = Query.match(p).where(fn.count(Query.match(p).to(ActedIn).node(m)) > 5).return_(p.name)
    compiled_count = q_count.compile("cypher")
    assert "COUNT { MATCH (p:Person)" in compiled_count.statement
    assert "> $p0" in compiled_count.statement
    assert compiled_count.parameters["p0"] == 5


def test_branching_graph_topologies_multi_pattern() -> None:
    p = Person("p")
    m = Movie("m")
    d = Person("d")

    # MATCH (p:Person)-[:ACTED_IN]->(m:Movie), (d:Person)-[:DIRECTED]->(m)
    q = (
        Query.match(p)
        .to(ActedIn)
        .node(m)
        .pattern()
        .node(d)
        .to(Directed)
        .node(m)
        .where(p.name != d.name)
        .return_(p.name, d.name, m.title)
    )
    compiled = q.compile("cypher")
    assert "MATCH (p:Person)-[" in compiled.statement
    assert "]->(m:Movie), (d:Person)-[" in compiled.statement
    assert "]->(m:Movie)" in compiled.statement
    assert "WHERE p.name != d.name" in compiled.statement
    assert "RETURN p.name, d.name, m.title" in compiled.statement


def test_multi_dialect_rich_expressions() -> None:
    p = Person("p")
    q = (
        Query.match(p)
        .where((p.age >= 21) & (p.salary > 50000))
        .return_(p.name, (p.salary / 12).as_("monthly_salary"))
    )

    # Cypher
    cypher_res = q.compile("cypher")
    assert "MATCH (p:Person)" in cypher_res.statement
    assert "RETURN p.name, p.salary / $p2 AS monthly_salary" in cypher_res.statement
    assert cypher_res.parameters["p2"] == 12

    # ISO GQL
    gql_res = q.compile("iso_gql")
    assert "MATCH (p:Person)" in gql_res.statement
    assert "RETURN p.name, p.salary / $p2 AS monthly_salary" in gql_res.statement

    # SQL:2023 PGQ
    pgq_res = q.compile("sql_pgq", graph_name="corp_graph")
    assert "GRAPH_TABLE (corp_graph" in pgq_res.statement
    assert "COLUMNS (p.name, p.salary / $p2 AS monthly_salary)" in pgq_res.statement


def test_scalar_mathematical_functions() -> None:
    p = Person("p")
    q = Query.match(p).return_(
        fn.abs_(p.salary).as_("abs_val"),
        fn.ceil(p.salary).as_("ceil_val"),
        fn.floor(p.salary).as_("floor_val"),
        fn.round_(p.salary, 2).as_("round_val"),
        fn.sign(p.salary).as_("sign_val"),
        fn.sqrt(p.age).as_("sqrt_val"),
        fn.power(p.age, 2).as_("power_val"),
        fn.exp(p.age).as_("exp_val"),
        fn.log(p.salary).as_("log_val"),
        fn.log10(p.salary).as_("log10_val"),
        fn.sin(p.age).as_("sin_val"),
        fn.cos(p.age).as_("cos_val"),
        fn.tan(p.age).as_("tan_val"),
        fn.degrees(p.age).as_("deg_val"),
        fn.radians(p.age).as_("rad_val"),
        fn.pi().as_("pi_val"),
        fn.rand().as_("rand_val"),
    )
    compiled = q.compile("cypher")
    assert "abs(p.salary) AS abs_val" in compiled.statement
    assert "ceil(p.salary) AS ceil_val" in compiled.statement
    assert "floor(p.salary) AS floor_val" in compiled.statement
    assert "round(p.salary, $p0) AS round_val" in compiled.statement
    assert "sign(p.salary) AS sign_val" in compiled.statement
    assert "sqrt(p.age) AS sqrt_val" in compiled.statement
    assert "power(p.age, $p1) AS power_val" in compiled.statement
    assert "pi() AS pi_val" in compiled.statement
    assert "rand() AS rand_val" in compiled.statement


def test_extended_string_functions() -> None:
    p = Person("p")
    q = Query.match(p).return_(
        fn.substring(p.name, 0, 3).as_("sub_name"),
        fn.left(p.name, 2).as_("left_name"),
        fn.right(p.name, 2).as_("right_name"),
        fn.ltrim(p.city).as_("ltrimmed"),
        fn.rtrim(p.city).as_("rtrimmed"),
        fn.replace(p.name, "A", "B").as_("replaced"),
        fn.reverse(p.name).as_("reversed"),
        fn.length(p.name).as_("len_name"),
    )
    compiled = q.compile("cypher")
    assert "substring(p.name, $p0, $p1) AS sub_name" in compiled.statement
    assert "left(p.name, $p2) AS left_name" in compiled.statement
    assert "right(p.name, $p3) AS right_name" in compiled.statement
    assert "ltrim(p.city) AS ltrimmed" in compiled.statement
    assert "rtrim(p.city) AS rtrimmed" in compiled.statement
    assert "replace(p.name, $p4, $p5) AS replaced" in compiled.statement
    assert "reverse(p.name) AS reversed" in compiled.statement
    assert "length(p.name) AS len_name" in compiled.statement


def test_list_and_reflection_functions() -> None:
    p = Person("p")
    q = Query.match(p).return_(
        fn.last(p.skills).as_("last_skill"),
        fn.range_(1, 10, 2).as_("num_range"),
        fn.keys(p).as_("node_keys"),
        fn.properties(p).as_("node_props"),
        fn.to_integer(p.age).as_("int_age"),
        fn.to_float(p.age).as_("float_age"),
        fn.to_string(p.age).as_("str_age"),
        fn.to_boolean(p.age).as_("bool_age"),
        fn.labels(p).as_("node_labels"),
        fn.element_id(p).as_("node_eid"),
    )
    compiled = q.compile("cypher")
    assert "last(p.skills) AS last_skill" in compiled.statement
    assert "range($p0, $p1, $p2) AS num_range" in compiled.statement
    assert "keys(p) AS node_keys" in compiled.statement
    assert "properties(p) AS node_props" in compiled.statement
    assert "toInteger(p.age) AS int_age" in compiled.statement
    assert "toFloat(p.age) AS float_age" in compiled.statement
    assert "toString(p.age) AS str_age" in compiled.statement
    assert "toBoolean(p.age) AS bool_age" in compiled.statement
    assert "labels(p) AS node_labels" in compiled.statement
    assert "elementId(p) AS node_eid" in compiled.statement


def test_graph_and_path_functions() -> None:
    p = Person("p")
    m = Movie("m")
    rel = ActedIn(alias="r")
    p_path = ident("p_path")

    q = (
        Query.match(p)
        .to(rel)
        .node(m)
        .return_(
            fn.start_node(rel).as_("s_node"),
            fn.end_node(rel).as_("e_node"),
            fn.type_(rel).as_("r_type"),
            fn.nodes(p_path).as_("p_nodes"),
            fn.relationships(p_path).as_("p_rels"),
            fn.shortest_path(p_path).as_("sp"),
            fn.all_shortest_paths(p_path).as_("asp"),
        )
    )
    compiled = q.compile("cypher")
    assert "startNode(r) AS s_node" in compiled.statement
    assert "endNode(r) AS e_node" in compiled.statement
    assert "type(r) AS r_type" in compiled.statement
    assert "nodes(p_path) AS p_nodes" in compiled.statement
    assert "relationships(p_path) AS p_rels" in compiled.statement
    assert "shortestPath(p_path) AS sp" in compiled.statement
    assert "allShortestPaths(p_path) AS asp" in compiled.statement


def test_dynamic_custom_function_dispatch() -> None:
    p = Person("p")
    # 1. Explicit fn.call("custom_func", *args)
    q1 = Query.match(p).return_(
        fn.call("apoc.text.clean", p.name).as_("clean_name"),
        fn.call("vector.similarity.cosine", p.name, "target_vec").as_("score"),
    )
    compiled1 = q1.compile("cypher")
    assert "apoc.text.clean(p.name) AS clean_name" in compiled1.statement
    assert "vector.similarity.cosine(p.name, $p0) AS score" in compiled1.statement
    assert compiled1.parameters["p0"] == "target_vec"

    # 2. Dynamic attribute access fn.any_custom_function(*args) (SQLAlchemy func.* model)
    q2 = Query.match(p).return_(
        fn.my_custom_geo_distance(p.city, "Berlin").as_("dist"),
        fn.custom_levenshtein(p.name, "Alice").as_("similarity"),
    )
    compiled2 = q2.compile("cypher")
    assert "my_custom_geo_distance(p.city, $p0) AS dist" in compiled2.statement
    assert "custom_levenshtein(p.name, $p1) AS similarity" in compiled2.statement
