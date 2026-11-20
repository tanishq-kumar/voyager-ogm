"""Query Spec v1 closed-world validation (issue #187).

Every structural failure in ``compile_query_from_spec`` raises ``VoyagerError``
with ``code == "INVALID_SPEC"`` and the JSON Pointer to the offending element in
``err.path`` (also present in the message). Each test below pins the exact path
for one rejection class.
"""

from __future__ import annotations

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
        ({"spec_version": 1, "with_clauses": [{"projections": []}]}, ""),
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
                "matches": [{"paths": [[("node", "p", ["Person"]), ("edge", "out", ["KNOWS"])]]}],
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
