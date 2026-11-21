"""Regression tests for the SQL/PGQ emitter fixes (#268) and graph-name validation (#270).

#268: OPTIONAL MATCH, RETURN DISTINCT, LOAD CSV, subquery expressions
(EXISTS/COUNT/list comprehension/pattern comprehension) and the ``=~`` operator
must be translated or rejected with ``UnsupportedFeature`` -- never silently
dropped -- and an empty projection list must raise a clear error instead of
emitting ``COLUMNS ()``.

#270: the SQL/PGQ graph name is interpolated into ``GRAPH_TABLE (... )`` and must
be a plain identifier at every entry point, including ``compile_query_from_spec``.
"""

from __future__ import annotations

from typing import Any

import pytest
from voyager_ogm import (
    Field,
    Query,
    compile_query_from_spec,
    list_comprehension,
    node,
    pattern_comprehension,
    relationship,
)


@node(label="Person")
class Person:
    name: str = Field()
    age: int = Field()
    city: str = Field()
    skills: list[str] = Field(default_factory=list)


@node(label="Company")
class Company:
    name: str = Field()


@relationship(type_name="WORKS_AT")
class WorksAt:
    since: int = Field(default=2020)


def _base_spec() -> dict[str, Any]:
    """Returns the smallest valid spec that compiles for every dialect."""
    return {
        "spec_version": 1,
        "matches": [{"paths": [[("node", "p", ["Person"], None)]]}],
        "projections": [("field", "p", "name", None)],
    }


# ---------------------------------------------------------------------------
# #268: fluent API
# ---------------------------------------------------------------------------


def test_sql_pgq_optional_match_raises() -> None:
    p = Person("p")
    c = Company("c")
    query = Query.match(p).optional_match(c).return_(p.name, c.name)

    with pytest.raises(ValueError, match="OPTIONAL MATCH"):
        query.compile("sql_pgq", graph_name="g")


def test_sql_pgq_distinct_is_emitted() -> None:
    p = Person("p")
    compiled = Query.match(p).distinct().return_(p.city).compile("sql_pgq", graph_name="g")

    assert compiled.statement == (
        "SELECT DISTINCT * FROM GRAPH_TABLE (g MATCH (p IS Person) COLUMNS (p.city))"
    )


def test_sql_pgq_load_csv_raises() -> None:
    p = Person("p")
    query = (
        Query.load_csv("file:///persons.csv", with_headers=True, alias="row")
        .match(p)
        .return_(p.name)
    )

    with pytest.raises(ValueError, match="LOAD CSV"):
        query.compile("sql_pgq", graph_name="g")


def test_sql_pgq_regex_operator_raises() -> None:
    p = Person("p")
    query = Query.match(p).where(p.name.regex("A.*")).return_(p.name)

    with pytest.raises(ValueError, match="Regular expression") as exc_info:
        query.compile("sql_pgq", graph_name="g")
    assert "NodeHandle(" not in str(exc_info.value)


def test_sql_pgq_subquery_expressions_raise_without_ast_dump() -> None:
    p = Person("p")
    c = Company("c")

    exists_query = (
        Query.match(p).where(Query.exists(Query.match(p).to(WorksAt).node(c))).return_(p.name)
    )
    with pytest.raises(ValueError, match="EXISTS") as exists_err:
        exists_query.compile("sql_pgq", graph_name="g")
    assert "QueryStatement {" not in str(exists_err.value)

    count_query = Query.match(p).return_(Query.count(Query.match(c)))
    with pytest.raises(ValueError, match="COUNT") as count_err:
        count_query.compile("sql_pgq", graph_name="g")
    assert "QueryStatement {" not in str(count_err.value)

    list_query = Query.match(p).return_(
        p.name,
        list_comprehension("x", p.skills, None, None).as_("skills"),
    )
    with pytest.raises(ValueError, match="List comprehension") as list_err:
        list_query.compile("sql_pgq", graph_name="g")
    assert "ListComprehension {" not in str(list_err.value)

    pattern_query = Query.match(p).return_(
        p.name,
        pattern_comprehension(
            path=Query.match(p).to(WorksAt).node(c),
            proj=c.name,
        ).as_("companies"),
    )
    with pytest.raises(ValueError, match="Pattern comprehension") as pattern_err:
        pattern_query.compile("sql_pgq", graph_name="g")
    assert "PatternComprehension {" not in str(pattern_err.value)


def test_sql_pgq_requires_a_projection() -> None:
    p = Person("p")
    with pytest.raises(ValueError, match="at least one projection") as exc_info:
        Query.match(p).compile("sql_pgq", graph_name="g")
    assert "COLUMNS ()" not in str(exc_info.value)


# ---------------------------------------------------------------------------
# #268: Query Spec path
# ---------------------------------------------------------------------------


def test_spec_optional_match_raises() -> None:
    spec = _base_spec()
    spec["matches"].append({"optional": True, "paths": [[("node", "c", ["Company"], None)]]})

    with pytest.raises(ValueError, match="OPTIONAL MATCH"):
        compile_query_from_spec(spec, "sql_pgq", graph_name="g")


def test_spec_distinct_is_emitted() -> None:
    spec = _base_spec()
    spec["distinct"] = True

    result = compile_query_from_spec(spec, "sql_pgq", graph_name="g")

    assert result["statement"] == (
        "SELECT DISTINCT * FROM GRAPH_TABLE (g MATCH (p IS Person) COLUMNS (p.name))"
    )


def test_spec_load_csv_raises() -> None:
    spec = _base_spec()
    spec["load_csv"] = ("file:///persons.csv", True, "row")

    with pytest.raises(ValueError, match="LOAD CSV"):
        compile_query_from_spec(spec, "sql_pgq", graph_name="g")


def test_spec_regex_operator_raises() -> None:
    spec = _base_spec()
    spec["matches"] = [
        {
            "paths": [[("node", "p", ["Person"], None)]],
            "where": [("bin", "regex", ("prop", "p", "name"), ("lit", "A.*"))],
        }
    ]

    with pytest.raises(ValueError, match="Regular expression") as exc_info:
        compile_query_from_spec(spec, "sql_pgq", graph_name="g")
    assert "NodeHandle(" not in str(exc_info.value)


@pytest.mark.parametrize(
    ("expression", "expected"),
    [
        (
            (
                "exists",
                {
                    "spec_version": 1,
                    "matches": [{"paths": [[("node", "c", ["Company"], None)]]}],
                },
            ),
            "EXISTS",
        ),
        (
            (
                "count",
                {
                    "spec_version": 1,
                    "matches": [{"paths": [[("node", "c", ["Company"], None)]]}],
                },
            ),
            "COUNT",
        ),
        (("list_comp", "x", ("prop", "p", "skills"), None, None), "List comprehension"),
        (
            ("pattern_comp", [("node", "p", ["Person"], None)], None, ("prop", "p", "name")),
            "Pattern comprehension",
        ),
    ],
)
def test_spec_subquery_expressions_raise(expression: Any, expected: str) -> None:
    spec = _base_spec()
    if expression[0] in {"exists", "count"}:
        spec["matches"][0]["where"] = [expression]
    else:
        spec["projections"] = [("expr", expression, None)]

    with pytest.raises(ValueError, match=expected) as exc_info:
        compile_query_from_spec(spec, "sql_pgq", graph_name="g")
    message = str(exc_info.value)
    assert "NodeHandle(" not in message
    assert "QueryStatement {" not in message
    assert "ListComprehension {" not in message
    assert "PatternComprehension {" not in message


def test_spec_without_projections_raises() -> None:
    spec = _base_spec()
    del spec["projections"]

    with pytest.raises(ValueError, match="at least one projection") as exc_info:
        compile_query_from_spec(spec, "sql_pgq", graph_name="g")
    assert "COLUMNS ()" not in str(exc_info.value)


# ---------------------------------------------------------------------------
# #270: graph name validation through the fluent API and Query Spec
# ---------------------------------------------------------------------------

VALID_GRAPH_NAMES = ["g", "_g", "g123", "a" * 63]
INVALID_GRAPH_NAMES = [
    "",
    "g) UNION SELECT 1 --",
    "g'",
    'g"',
    "g;",
    "g name",
    "g-name",
    "1g",
    "g.dot",
]


@pytest.mark.parametrize("graph_name", VALID_GRAPH_NAMES)
def test_fluent_sql_pgq_accepts_valid_graph_names(graph_name: str) -> None:
    p = Person("p")
    compiled = Query.match(p).return_(p.name).compile("sql_pgq", graph_name=graph_name)
    assert f"FROM GRAPH_TABLE ({graph_name} MATCH" in compiled.statement


@pytest.mark.parametrize("graph_name", INVALID_GRAPH_NAMES)
def test_fluent_sql_pgq_rejects_invalid_graph_names(graph_name: str) -> None:
    p = Person("p")
    with pytest.raises(ValueError, match="Invalid graph name"):
        Query.match(p).return_(p.name).compile("sql_pgq", graph_name=graph_name)


@pytest.mark.parametrize("graph_name", VALID_GRAPH_NAMES)
@pytest.mark.parametrize("dialect", ["sql_pgq", "duckpgq", "pgq", "sql"])
def test_compile_query_from_spec_accepts_valid_graph_names(graph_name: str, dialect: str) -> None:
    result = compile_query_from_spec(_base_spec(), dialect, graph_name=graph_name)
    assert f"FROM GRAPH_TABLE ({graph_name} MATCH" in result["statement"]


@pytest.mark.parametrize("graph_name", INVALID_GRAPH_NAMES)
@pytest.mark.parametrize("dialect", ["sql_pgq", "duckpgq"])
def test_compile_query_from_spec_rejects_invalid_graph_names(graph_name: str, dialect: str) -> None:
    with pytest.raises(ValueError, match="Invalid graph name") as exc_info:
        compile_query_from_spec(_base_spec(), dialect, graph_name=graph_name)
    assert "SQL:PGQ" in str(exc_info.value)
