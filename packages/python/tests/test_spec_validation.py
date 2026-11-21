"""Query Spec v1 closed-world validation (issue #187).

Every structural failure in ``compile_query_from_spec`` raises ``VoyagerError``
with ``code == "INVALID_SPEC"`` and the JSON Pointer to the offending element in
``err.path`` (also present in the message). Each test below pins the exact path
for one rejection class.
"""

from __future__ import annotations

import json
from typing import Any

import pytest
from voyager_ogm import VoyagerError
from voyager_ogm._voyager_rs import compile_query_from_spec

PERSON_MATCH: list[dict[str, Any]] = [
    {
        "spec_version": 1,
        "paths": [[("node", "p", ["Person"])]],
    }
]

BASE: dict[str, Any] = {
    "spec_version": 1,
    "matches": PERSON_MATCH,
    "projections": [("field", "p", "name", "n")],
}


def assert_invalid_spec(spec: Any, path: str) -> VoyagerError:
    """Compiles ``spec`` and asserts one INVALID_SPEC naming ``path``."""
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "cypher")
    error = exc_info.value
    assert error.code == "INVALID_SPEC"
    assert isinstance(error, ValueError)
    assert error.path == path
    assert path in str(error) or (path == "" and "document root" in str(error))
    return error


def test_issue_187_unknown_top_level_key_is_rejected() -> None:
    """The issue repro with ``bogus`` no longer compiles silently."""
    assert_invalid_spec({**BASE, "bogus": 1}, "/bogus")


def test_issue_187_empty_spec_is_rejected() -> None:
    """A spec with only ``spec_version`` produces no statement and is rejected."""
    error = assert_invalid_spec({"spec_version": 1}, "")
    assert "empty query spec" in str(error)


@pytest.mark.parametrize(
    ("spec", "path"),
    [
        # Version handling.
        ({"matches": PERSON_MATCH}, "/spec_version"),
        ({"spec_version": "1", "matches": PERSON_MATCH}, "/spec_version"),
        ({"spec_version": 2, "matches": PERSON_MATCH}, "/spec_version"),
        (
            {"spec_version": 1, "matches": [{"spec_version": 2, "paths": []}]},
            "/matches/0/spec_version",
        ),
        (
            {"spec_version": 1, "with_clauses": [{"spec_version": 2, "projections": []}]},
            "/with_clauses/0/spec_version",
        ),
        # Empty containers do not count as clauses.
        ({"spec_version": 1, "matches": []}, ""),
        ({"spec_version": 1, "projections": []}, ""),
        (
            {"spec_version": 1, "with_clauses": [{"projections": []}]},
            "/with_clauses/0/projections",
        ),
        # Unknown keys at every object level.
        ({"spec_version": 1, "matches": [{"paths": [], "bogus": 1}]}, "/matches/0/bogus"),
        (
            {"spec_version": 1, "with_clauses": [{"projections": [], "bogus": 1}]},
            "/with_clauses/0/bogus",
        ),
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "projections": [
                    ("expr", {"spec_version": 1, "expr": ("lit", 1), "bogus": 1}, None)
                ],
            },
            "/projections/0/1/bogus",
        ),
        # Wrong container types (previously silently skipped).
        ({"spec_version": 1, "matches": 42}, "/matches"),
        ({"spec_version": 1, "mutations": {}}, "/mutations"),
        ({"spec_version": 1, "unwinds": "batch"}, "/unwinds"),
        ({"spec_version": 1, "projections": "name"}, "/projections"),
        ({"spec_version": 1, "order_by": 42}, "/order_by"),
        ({"spec_version": 1, "with_clauses": 42}, "/with_clauses"),
        ({"spec_version": 1, "linear_clauses": 42}, "/linear_clauses"),
        ({"spec_version": 1, "load_csv": 42}, "/load_csv"),
        (
            {"spec_version": 1, "matches": [{"paths": 42}]},
            "/matches/0/paths",
        ),
        # Wrong shapes inside arrays.
        (
            {"spec_version": 1, "matches": [42]},
            "/matches/0",
        ),
        (
            {
                "spec_version": 1,
                "matches": [{"paths": [[("node", "p", ["Person"]), ("edge", "out")]]}],
            },
            "/matches/0/paths/0/1",
        ),
        # Unknown enum values.
        (
            {"spec_version": 1, "matches": [{"path_mode": "zigzag", "paths": []}]},
            "/matches/0/path_mode",
        ),
        ({"spec_version": 1, **BASE, "execution_mode": "fast"}, "/execution_mode"),
        (
            {"spec_version": 1, "linear_clauses": [("yield", "x")]},
            "/linear_clauses/0/0",
        ),
        (
            {"spec_version": 1, "mutations": [("upsert",)]},
            "/mutations/0/0",
        ),
        (
            {"spec_version": 1, "matches": [{"paths": [[("vertex", "p", ["Person"])]]}]},
            "/matches/0/paths/0/0/0",
        ),
        (
            {"spec_version": 1, "matches": PERSON_MATCH, "projections": [("bogus", "p", "name")]},
            "/projections/0/0",
        ),
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "projections": [("agg", "p", "age", "median", None)],
            },
            "/projections/0/3",
        ),
        # Misspelt expression tag in expression position.
        (
            {
                "spec_version": 1,
                "matches": [
                    {
                        "spec_version": 1,
                        "paths": [[("node", "p", ["Person"])]],
                        "where": [["propp", "p", "age"]],
                    }
                ],
                "projections": [("field", "p", "name", None)],
            },
            "/matches/0/where/0",
        ),
        # Negative pagination.
        ({**BASE, "skip": -1}, "/skip"),
        ({**BASE, "offset": -1}, "/offset"),
        ({**BASE, "limit": -1}, "/limit"),
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "with_clauses": [{"projections": [("field", "p", "name", "n")], "skip": -1}],
            },
            "/with_clauses/0/skip",
        ),
        # A match clause must carry the schema-required `paths` key, and a with
        # clause the schema-required `projections` key.
        ({"spec_version": 1, "matches": [{}]}, "/matches/0/paths"),
        ({"spec_version": 1, "with_clauses": [{}]}, "/with_clauses/0/projections"),
    ],
)
def test_structural_failures_use_invalid_spec_with_exact_path(spec: Any, path: str) -> None:
    """Every rejection class fails with INVALID_SPEC and the expected pointer."""
    assert_invalid_spec(spec, path)


def test_non_object_spec_is_rejected() -> None:
    """A non-object top-level spec is INVALID_SPEC at the document root."""
    assert_invalid_spec(["spec_version", 1], "")


def test_unknown_nested_expression_key_is_rejected() -> None:
    """The object-wrapped expression has the same closed key set."""
    error = assert_invalid_spec(
        {
            "spec_version": 1,
            "matches": PERSON_MATCH,
            "projections": [("expr", {"spec_version": 1, "expr": ("lit", 1), "extra": True}, None)],
        },
        "/projections/0/1/extra",
    )
    assert "expression object" in str(error)


def test_error_path_is_a_json_pointer() -> None:
    """A nested array/object failure exposes a concrete JSON Pointer."""
    error = assert_invalid_spec(
        {
            "spec_version": 1,
            "matches": [
                {
                    "paths": [
                        [("node", "a", ["A"]), ("edge", "out", ["R"], None, None, None)],
                        [("node", "b", ["B"]), ("edge", "sideways", ["R"], None, None, None)],
                    ]
                }
            ],
        },
        "/matches/0/paths/1/1/1",
    )
    assert "sideways" in str(error)


@pytest.mark.parametrize(
    ("expression", "first_element"),
    [
        ([1, 2, 3], "1"),
        (["red", "green"], "'red'"),
        (["Red", "green"], "'Red'"),
        ([], None),
        (["propp", "x"], "'propp'"),
    ],
    ids=["non-string-first", "unknown-tag", "unknown-uppercase-tag", "empty", "misspelt-tag"],
)
def test_untagged_expression_array_in_where_is_rejected(
    expression: Any, first_element: str | None
) -> None:
    """A WHERE IN right-hand side may not be an untagged array."""
    error = assert_invalid_spec(
        {
            "spec_version": 1,
            "matches": [
                {
                    "spec_version": 1,
                    "paths": [[("node", "p", ["Person"])]],
                    "where": [["bin", "in", ["prop", "p", "name"], expression]],
                }
            ],
            "projections": [("field", "p", "name", None)],
        },
        "/matches/0/where/0/3",
    )
    assert (
        'Use ["lit", [...]] for a literal list or ["list", [...]] for a list of expressions'
        in str(error)
    )
    if first_element is not None:
        assert first_element in str(error)


@pytest.mark.parametrize(
    ("expression", "first_element"),
    [
        ([1, 2, 3], "1"),
        (["red", "green"], "'red'"),
        (["Red", "green"], "'Red'"),
        ([], None),
        (["propp", "x"], "'propp'"),
    ],
    ids=["non-string-first", "unknown-tag", "unknown-uppercase-tag", "empty", "misspelt-tag"],
)
def test_untagged_expression_array_in_projection_is_rejected(
    expression: Any, first_element: str | None
) -> None:
    """A projection expression may not be an untagged array."""
    error = assert_invalid_spec(
        {"spec_version": 1, "matches": PERSON_MATCH, "projections": [("expr", expression, None)]},
        "/projections/0/1",
    )
    assert (
        'Use ["lit", [...]] for a literal list or ["list", [...]] for a list of expressions'
        in str(error)
    )
    if first_element is not None:
        assert first_element in str(error)


def test_explicit_literal_and_expression_lists_compile() -> None:
    """Explicit ["lit", [...]] and ["list", [...]] forms compile as before."""
    base = {"spec_version": 1, "matches": PERSON_MATCH}
    numeric = compile_query_from_spec(
        {**base, "projections": [("expr", ["lit", [1, 2, 3]], "xs")]}, "cypher"
    )
    strings = compile_query_from_spec(
        {**base, "projections": [("expr", ["lit", ["red", "green"]], "xs")]}, "cypher"
    )
    expressions = compile_query_from_spec(
        {
            **base,
            "projections": [("expr", ["list", [["lit", 1], ["lit", 2], ["lit", 3]]], "xs")],
        },
        "cypher",
    )

    assert numeric["statement"] == "MATCH (p:Person) RETURN $p0 AS xs"
    assert numeric["parameters"] == {"p0": [1, 2, 3]}
    assert strings["statement"] == "MATCH (p:Person) RETURN $p0 AS xs"
    assert strings["parameters"] == {"p0": ["red", "green"]}
    assert expressions["statement"] == "MATCH (p:Person) RETURN [$p0, $p1, $p2] AS xs"
    assert expressions["parameters"] == {"p0": 1, "p1": 2, "p2": 3}


# --- `where`: one expression or a list of expressions (issue #187 follow-up) ---

MATCH_WHERE_SINGLE_JSON = """
{
  "spec_version": 1,
  "matches": [
    {
      "paths": [[["node", "p", ["Person"]]]],
      "where": ["bin", "=", ["prop", "p", "age"], ["lit", 21]]
    }
  ],
  "projections": [["field", "p", "name", null]]
}
"""

MATCH_WHERE_LIST_JSON = """
{
  "spec_version": 1,
  "matches": [
    {
      "paths": [[["node", "p", ["Person"]]]],
      "where": [["bin", "=", ["prop", "p", "age"], ["lit", 21]]]
    }
  ],
  "projections": [["field", "p", "name", null]]
}
"""

WITH_WHERE_SINGLE_JSON = """
{
  "spec_version": 1,
  "matches": [{"paths": [[["node", "p", ["Person"]]]]}],
  "with_clauses": [
    {
      "projections": [["field", "p", "name", "n"]],
      "where": ["bin", "=", ["ident", "n"], ["lit", "Alice"]]
    }
  ],
  "projections": [["expr", ["ident", "n"], null]]
}
"""

WITH_WHERE_LIST_JSON = """
{
  "spec_version": 1,
  "matches": [{"paths": [[["node", "p", ["Person"]]]]}],
  "with_clauses": [
    {
      "projections": [["field", "p", "name", "n"]],
      "where": [["bin", "=", ["ident", "n"], ["lit", "Alice"]]]
    }
  ],
  "projections": [["expr", ["ident", "n"], null]]
}
"""


def compile_json(spec_json: str, dialect: str = "cypher") -> dict[str, Any]:
    """Compile a literal JSON spec, so every array is a real JSON list."""
    return compile_query_from_spec(json.loads(spec_json), dialect)


@pytest.mark.parametrize(
    ("single_json", "list_json"),
    [
        pytest.param(MATCH_WHERE_SINGLE_JSON, MATCH_WHERE_LIST_JSON, id="match"),
        pytest.param(WITH_WHERE_SINGLE_JSON, WITH_WHERE_LIST_JSON, id="with"),
    ],
)
def test_where_single_expression_json_array_compiles_like_predicate_list(
    single_json: str, list_json: str
) -> None:
    """A single expression `where` is the same query as a one-item list form."""
    single = compile_json(single_json)
    predicates = compile_json(list_json)
    assert single["statement"] == predicates["statement"]
    assert single["parameters"] == predicates["parameters"]
    assert "WHERE" in single["statement"]


def test_where_list_of_two_predicates_json_array() -> None:
    """Two predicates compile to the same AND as two list items."""
    compiled = compile_json(
        """
        {
          "spec_version": 1,
          "matches": [
            {
              "paths": [[["node", "p", ["Person"]]]],
              "where": [
                ["bin", ">", ["prop", "p", "age"], ["lit", 21]],
                ["bin", "<", ["prop", "p", "age"], ["lit", 65]]
              ]
            }
          ],
          "projections": [["field", "p", "name", null]]
        }
        """
    )
    assert compiled["statement"] == (
        "MATCH (p:Person) WHERE (p.age > $p0) AND (p.age < $p1) RETURN p.name"
    )
    assert compiled["parameters"] == {"p0": 21, "p1": 65}


def test_empty_where_array_is_an_empty_predicate_list() -> None:
    """An empty `where` array adds no predicate, matching the previous rule."""
    compiled = compile_json(
        """
        {
          "spec_version": 1,
          "matches": [
            {
              "paths": [[["node", "p", ["Person"]]]],
              "where": []
            }
          ],
          "projections": [["field", "p", "name", null]]
        }
        """
    )
    assert compiled["statement"] == "MATCH (p:Person) RETURN p.name"


# --- Raw JSON value positions (not expression positions) ---------------------

RAW_VALUE_POSITIONS: dict[str, tuple[str, str, str]] = {
    "spec_version": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "MATCH (p:Person)",
    ),
    "load_csv[url,with_headers,alias]": (
        '{"spec_version": 1, "load_csv": ["https://example.com/nodes.csv", true, "row"], '
        '"matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "LOAD CSV WITH HEADERS FROM $p0 AS row",
    ),
    "unwinds[param,alias]": (
        '{"spec_version": 1, "unwinds": [["$batch", "row"]], '
        '"matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "UNWIND $batch AS row",
    ),
    "match.optional": (
        '{"spec_version": 1, "matches": [{"optional": true, '
        '"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "OPTIONAL MATCH",
    ),
    "match.path_mode": (
        '{"spec_version": 1, "matches": [{"path_mode": "simple", '
        '"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "iso_gql",
        "MATCH SIMPLE",
    ),
    "match.path_variable": (
        '{"spec_version": 1, "matches": [{"path_variable": "pth", '
        '"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "MATCH pth =",
    ),
    "path_step.node[variable,labels]": (
        '{"spec_version": 1, "matches": [{"paths": '
        '[[["node", null, ["Person", "Admin"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "(:Person:Admin)",
    ),
    "path_step.edge[direction,types,variable,min_hops,max_hops]": (
        '{"spec_version": 1, "matches": [{"paths": [['
        '["node", "p", ["Person"]], '
        '["edge", "incoming", ["KNOWS", "LIKES"], "r", 1, 3], '
        '["node", "f", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "<-[r:KNOWS|LIKES*1..3]-",
    ),
    "path_step.edge[null_hops]": (
        '{"spec_version": 1, "matches": [{"paths": [['
        '["node", "p", ["Person"]], '
        '["edge", "undirected", [], null, null, null], '
        '["node", "f", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        ")-[]-(",
    ),
    "mutation.create[paths]": (
        '{"spec_version": 1, "mutations": [["create", [[["node", "c", ["Company"]]]]]]}',
        "cypher",
        "CREATE (c:Company)",
    ),
    "mutation.merge[path,on_create,on_match]": (
        '{"spec_version": 1, "mutations": [["merge", [["node", "c", ["Company"]]], '
        '[["c", "name", ["lit", "Acme"]]], [["c", "name", ["lit", "AcmeCorp"]]]]]}',
        "cypher",
        "ON CREATE SET c.name = $p0 ON MATCH SET c.name = $p1",
    ),
    "mutation.set[variable,property]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"mutations": [["set", "p", "age", ["lit", 30]]], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "SET p.age = $p0",
    ),
    "mutation.delete[detach,targets]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"mutations": [["delete", true, ["p"]]]}',
        "cypher",
        "DETACH DELETE p",
    ),
    "mutation.remove[variable,property]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"mutations": [["remove", "p", "age"]], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "REMOVE p.age",
    ),
    "with_clause[distinct,projections,order_by,skip,limit]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"with_clauses": [{"distinct": true, "projections": [["field", "p", "name", "n"]], '
        '"order_by": [["n", "n", true]], "skip": 0, "limit": 5}], '
        '"projections": [["field", "n", "n", null]]}',
        "cypher",
        "WITH DISTINCT p.name AS n ORDER BY n.n ASC SKIP 0 LIMIT 5",
    ),
    "projection.field[variable,property,alias]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", "n"]]}',
        "cypher",
        "RETURN p.name AS n",
    ),
    "projection.agg[variable,property,func,alias]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["agg", "p", "age", "count_distinct", null]]}',
        "cypher",
        "RETURN COUNT(DISTINCT p.age)",
    ),
    "root.distinct": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]], "distinct": true}',
        "cypher",
        "RETURN DISTINCT p.name",
    ),
    "order_by[variable,property,direction]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]], '
        '"order_by": [["p", "age", false], ["p", "name", "ASC"]]}',
        "cypher",
        "ORDER BY p.age DESC, p.name ASC",
    ),
    "pagination[skip,offset,limit]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]], '
        '"skip": 0, "offset": 0, "limit": 50}',
        "cypher",
        "SKIP 0 LIMIT 50",
    ),
    "execution_mode": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]], "execution_mode": "explain"}',
        "cypher",
        "EXPLAIN MATCH (p:Person)",
    ),
    "expression.lit[payload]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", ["lit", {"name": "Alice", "age": 30, '
        '"tags": ["a", "b"], "ok": true, "none": null}], "m"]]}',
        "cypher",
        "RETURN $p0 AS m",
    ),
    "expression.case[operand,branches,else]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", ["case", null, '
        '[[["lit", true], ["lit", "yes"]]], null], "c"]]}',
        "cypher",
        "CASE WHEN $p0 THEN $p1 END",
    ),
    "expression.list_comp[where_filter,map_expr]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", ["list_comp", "x", ["prop", "p", "tags"], null, null], '
        '"x"]]}',
        "cypher",
        "[x IN p.tags]",
    ),
    "expression.pattern_comp[path_spec,where_filter]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", ["pattern_comp", [["node", "f", ["Person"]]], null, '
        '["prop", "f", "name"]], "x"]]}',
        "cypher",
        "[(f:Person) | f.name]",
    ),
    "expression.exists[subquery_spec]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", ["exists", {"spec_version": 1, "matches": [{"paths": '
        '[[["node", "f", ["Person"]]]]}]}], "x"]]}',
        "cypher",
        "EXISTS { MATCH (f:Person) }",
    ),
    "expression.count[subquery_spec]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", ["count", {"spec_version": 1, "matches": [{"paths": '
        '[[["node", "f", ["Person"]]]]}]}], "x"]]}',
        "cypher",
        "COUNT { MATCH (f:Person) }",
    ),
    "expression.object_wrapped[spec_version,expr]": (
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", {"spec_version": 1, "expr": ["lit", 7]}, null]]}',
        "cypher",
        "RETURN $p0",
    ),
    "linear_clauses[variable,expression]": (
        '{"spec_version": 1, "linear_clauses": ['
        '["let", "threshold", ["lit", 30]], '
        '["filter", ["bin", "gt", ["ident", "threshold"], ["lit", 0]]]], '
        '"matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["field", "p", "name", null]]}',
        "cypher",
        "$p0 AS threshold",
    ),
}


@pytest.mark.parametrize(
    ("spec_json", "dialect", "fragment"),
    RAW_VALUE_POSITIONS.values(),
    ids=RAW_VALUE_POSITIONS.keys(),
)
def test_raw_value_positions_accept_schema_values(
    spec_json: str, dialect: str, fragment: str
) -> None:
    """A raw JSON value position accepts exactly the schema value it documents."""
    compiled = compile_json(spec_json, dialect)
    assert fragment in compiled["statement"]


def test_literal_payload_keeps_nested_raw_json_values() -> None:
    """A ["lit", value] payload may be any raw JSON value, including objects."""
    payload = {"name": "Alice", "age": 30, "tags": ["a", "b"], "ok": True, "none": None}
    compiled = compile_json(
        '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", ["lit", {"name": "Alice", "age": 30, '
        '"tags": ["a", "b"], "ok": true, "none": null}], "m"]]}'
    )
    assert compiled["parameters"] == {"p0": payload}


# --- Error messages: JSON type names and explicit-form hints ------------------

LIST_HINT = 'Use ["lit", [...]] for a literal list or ["list", [...]] for a list of expressions'
SCALAR_HINT = '["lit", value]'


@pytest.mark.parametrize(
    ("spec_json", "path", "type_text", "hint"),
    [
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
            '"projections": [["expr", 21, null]]}',
            "/projections/0/1",
            "got number",
            SCALAR_HINT,
            id="bare-scalar",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
            '"projections": [["expr", [1, 2, 3], null]]}',
            "/projections/0/1",
            "got 1 (number)",
            LIST_HINT,
            id="bare-list-non-string-first",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
            '"projections": [["expr", [], null]]}',
            "/projections/0/1",
            "Empty expression array",
            LIST_HINT,
            id="empty-array",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
            '"projections": [["expr", ["red", "green"], null]]}',
            "/projections/0/1",
            "Unknown expression tag 'red'",
            LIST_HINT,
            id="unknown-tag",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
            '"projections": [["expr", ["propp", "p", "age"], null]]}',
            "/projections/0/1",
            "Unknown expression tag 'propp'",
            LIST_HINT,
            id="misspelt-tag",
        ),
    ],
)
def test_expression_position_errors_use_json_types_and_hints(
    spec_json: str, path: str, type_text: str, hint: str
) -> None:
    """Expression rejections name JSON types and suggest an explicit spelling."""
    error = assert_invalid_spec(json.loads(spec_json), path)
    message = str(error)
    assert "<class" not in message
    assert type_text in message
    assert hint in message


@pytest.mark.parametrize(
    ("spec_json", "path", "type_text", "hint"),
    [
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]], '
            '"where": [1, 2, 3]}], "projections": [["field", "p", "name", null]]}',
            "/matches/0/where",
            "got first element 1 (number)",
            LIST_HINT,
            id="where-array-scalar-first",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]], '
            '"where": 21}], "projections": [["field", "p", "name", null]]}',
            "/matches/0/where",
            "got number",
            SCALAR_HINT,
            id="where-bare-scalar",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]], '
            '"where": [["bin", "in", ["prop", "p", "name"], [1, 2, 3]]]}], '
            '"projections": [["field", "p", "name", null]]}',
            "/matches/0/where/0/3",
            "got 1 (number)",
            LIST_HINT,
            id="where-expression-list-non-string-first",
        ),
    ],
)
def test_where_rejections_use_json_types_and_hints(
    spec_json: str, path: str, type_text: str, hint: str
) -> None:
    """A `where` that matches neither schema branch explains both spellings."""
    error = assert_invalid_spec(json.loads(spec_json), path)
    message = str(error)
    assert "<class" not in message
    assert type_text in message
    assert hint in message


@pytest.mark.parametrize(
    ("spec_json", "path", "type_text"),
    [
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", 42]]]}]}',
            "/matches/0/paths/0/0/2",
            "got number",
            id="node-labels",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": 42}',
            "/matches",
            "got number",
            id="matches-container",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
            '"projections": [["field", "p", "name", null]], "skip": true}',
            "/skip",
            "got boolean",
            id="skip-boolean",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [{"paths": [[["node", "p", ["Person"]]]]}], '
            '"projections": [["field", "p", "name", null]], '
            '"order_by": [["p", "age", 1]]}',
            "/order_by/0/2",
            "got number",
            id="order-direction",
        ),
        pytest.param(
            '{"spec_version": 1, "matches": [42]}',
            "/matches/0",
            "got number",
            id="match-object",
        ),
        pytest.param(
            '["spec_version", 1]',
            "",
            "got array",
            id="non-object-spec",
        ),
    ],
)
def test_non_expression_errors_use_json_types(spec_json: str, path: str, type_text: str) -> None:
    """Non-expression rejections also say JSON type names, never Python reprs."""
    error = assert_invalid_spec(json.loads(spec_json), path)
    message = str(error)
    assert "<class" not in message
    assert type_text in message


# --- Path step shapes -------------------------------------------------------


def _match_spec(step_json: str) -> dict[str, Any]:
    return json.loads(
        '{"spec_version": 1, "matches": [{"spec_version": 1, "paths": [[["node", "a", ["Person"]], '
        + step_json
        + ', ["node", "b", ["Person"]]]]}], "projections": [["field", "a", "name", "n"]]}'
    )


def test_node_step_property_value_must_be_an_expression() -> None:
    spec = json.loads(
        '{"spec_version": 1, "mutations": [["create", [[["node", "a", ["Person"], '
        '{"name": "Alice"}]]]]]}'
    )
    error = assert_invalid_spec(spec, "/mutations/0/1/0/0/3/name")
    assert "got string" in str(error)
    assert '["lit", value]' in str(error)


def test_node_step_properties_must_be_an_object_or_null() -> None:
    spec = json.loads(
        '{"spec_version": 1, "mutations": [["create", [[["node", "a", ["Person"], "Alice"]]]]]}'
    )
    error = assert_invalid_spec(spec, "/mutations/0/1/0/0/3")
    assert "expected a JSON object" in str(error)


def test_node_step_trailing_null_is_still_accepted() -> None:
    spec = json.loads(
        '{"spec_version": 1, "matches": [{"spec_version": 1, "paths": '
        '[[["node", "p", ["Person"], null]]]}], "projections": [["field", "p", "name", "n"]]}'
    )
    assert compile_query_from_spec(spec, "cypher")["statement"] == (
        "MATCH (p:Person) RETURN p.name AS n"
    )


def test_edge_step_short_form_matches_full_form() -> None:
    short = compile_query_from_spec(_match_spec('["edge", "out", ["KNOWS"]]'), "cypher")
    full = compile_query_from_spec(
        _match_spec('["edge", "out", ["KNOWS"], null, null, null]'), "cypher"
    )
    assert short["statement"] == full["statement"]


def test_edge_step_extra_item_is_rejected() -> None:
    error = assert_invalid_spec(
        _match_spec('["edge", "out", ["KNOWS"], "r", null, null, {"since": ["lit", 2020]}, 1]'),
        "/matches/0/paths/0/1/7",
    )
    assert "accepts no further items" in str(error)


def test_edge_step_properties_must_be_an_object_or_null() -> None:
    error = assert_invalid_spec(
        _match_spec('["edge", "out", ["KNOWS"], "r", null, null, ["since", 2020]]'),
        "/matches/0/paths/0/1/6",
    )
    assert "expected a JSON object" in str(error)


def test_node_step_fifth_item_is_rejected() -> None:
    error = assert_invalid_spec(
        _match_spec('["node", "x", ["Person"], {"name": ["lit", "x"]}, 1]'),
        "/matches/0/paths/0/1/4",
    )
    assert "accepts no further items" in str(error)
