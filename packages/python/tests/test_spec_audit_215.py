"""Query Spec v1 consumer fixes from the #215 audit (issue #187 follow-up).

Each test pins either ``VoyagerError`` with ``code == "INVALID_SPEC"`` and the
exact JSON Pointer, or the corrected compiled statement.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

import pytest
from voyager_ogm import VoyagerError
from voyager_ogm._voyager_rs import compile_query_from_spec

PERSON_PATH = [[("node", "p", ["Person"])]]

BASE: dict[str, Any] = {
    "spec_version": 1,
    "matches": [{"spec_version": 1, "paths": PERSON_PATH}],
    "projections": [("field", "p", "name", "n")],
}


def compile_cypher(spec: Any) -> dict[str, Any]:
    return compile_query_from_spec(spec, "cypher")


def assert_invalid_spec(spec: Any, path: str) -> VoyagerError:
    with pytest.raises(VoyagerError) as exc_info:
        compile_cypher(spec)
    error = exc_info.value
    assert error.code == "INVALID_SPEC"
    assert error.path == path
    return error


def where_spec(expression: Any) -> dict[str, Any]:
    """A valid match plus a projection, with ``expression`` as its where."""
    return {
        "spec_version": 1,
        "matches": [
            {
                "spec_version": 1,
                "paths": PERSON_PATH,
                "where": expression,
            }
        ],
        "projections": [("field", "p", "name", "n")],
    }


def projection_spec(expression: Any) -> dict[str, Any]:
    return {**BASE, "projections": [("expr", expression, None)]}


# --- F1 / F2: root pagination needs a RETURN, and a query may not end in WITH


@pytest.mark.parametrize(
    ("key", "value"),
    [
        ("distinct", True),
        ("order_by", [("p", "age", False)]),
        ("skip", 0),
        ("offset", 0),
        ("limit", 5),
    ],
)
def test_root_pagination_without_projections_is_rejected(key: str, value: Any) -> None:
    spec = {
        "spec_version": 1,
        "matches": [{"spec_version": 1, "paths": PERSON_PATH}],
        key: value,
    }
    error = assert_invalid_spec(spec, f"/{key}")
    assert "requires 'projections'" in str(error)


def test_root_pagination_does_not_fold_into_the_last_with() -> None:
    spec = {
        "spec_version": 1,
        "matches": [{"spec_version": 1, "paths": PERSON_PATH}],
        "with_clauses": [{"projections": [("field", "p", "name", "n")]}],
        "limit": 5,
    }
    error = assert_invalid_spec(spec, "/limit")
    assert "requires 'projections'" in str(error)


def test_query_ending_in_with_needs_root_projections() -> None:
    spec = {
        "spec_version": 1,
        "matches": [{"spec_version": 1, "paths": PERSON_PATH}],
        "with_clauses": [{"projections": [("field", "p", "name", "n")]}],
    }
    error = assert_invalid_spec(spec, "/projections")
    assert "ending in WITH" in str(error)


def test_with_clauses_and_mutations_do_not_end_in_with() -> None:
    spec = {
        "spec_version": 1,
        "matches": [{"spec_version": 1, "paths": PERSON_PATH}],
        "with_clauses": [{"projections": [("field", "p", "name", "n")]}],
        "mutations": [("set", "p", "age", ("lit", 30))],
    }
    assert compile_cypher(spec)["statement"].startswith("MATCH (p:Person) WITH")


def test_mutations_only_spec_still_compiles() -> None:
    compiled = compile_cypher(
        {
            "spec_version": 1,
            "mutations": [("create", [[("node", "c", ["Company"])]])],
        }
    )
    assert compiled["statement"] == "CREATE (c:Company)"


def test_root_projections_with_root_pagination_still_compile() -> None:
    spec = {**BASE, "order_by": [("p", "age", False)], "skip": 0, "limit": 5}
    assert compile_cypher(spec)["statement"] == (
        "MATCH (p:Person) RETURN p.name AS n ORDER BY p.age DESC SKIP 0 LIMIT 5"
    )


# --- F3 / F4: empty WITH projections and empty match paths


def test_empty_with_projections_is_rejected() -> None:
    spec = {
        **BASE,
        "with_clauses": [
            {
                "projections": [],
                "where": [["bin", "gt", ["prop", "p", "age"], ["lit", 21]]],
                "limit": 5,
            }
        ],
    }
    error = assert_invalid_spec(spec, "/with_clauses/0/projections")
    assert "at least one projection" in str(error)


def test_empty_match_paths_is_rejected() -> None:
    spec = {
        "spec_version": 1,
        "matches": [
            {
                "paths": [],
                "where": [["bin", "gt", ["prop", "p", "age"], ["lit", 21]]],
            }
        ],
        "projections": [("field", "p", "name", "n")],
    }
    error = assert_invalid_spec(spec, "/matches/0/paths")
    assert "at least one path" in str(error)


# --- F8: every fixed-arity array rejects extra items at the extra item's path

ARITY_CASES: list[tuple[str, Any, str]] = [
    (
        "load_csv",
        {"load_csv": ["file:///a.csv", True, "row", "extra"]},
        "/load_csv/3",
    ),
    (
        "unwinds",
        {"unwinds": [("$batch", "row", "extra")]},
        "/unwinds/0/2",
    ),
    (
        "create",
        {"mutations": [("create", [[("node", "c", ["Company"])]], "extra")]},
        "/mutations/0/2",
    ),
    (
        "merge",
        {"mutations": [("merge", [("node", "c", ["Company"])], [], [], "extra")]},
        "/mutations/0/4",
    ),
    (
        "set",
        {"mutations": [("set", "p", "age", ("lit", 30), "extra")]},
        "/mutations/0/4",
    ),
    (
        "delete",
        {"mutations": [("delete", False, ["p"], "extra")]},
        "/mutations/0/3",
    ),
    (
        "remove",
        {"mutations": [("remove", "p", "age", "extra")]},
        "/mutations/0/3",
    ),
    (
        "merge-on-create",
        {
            "mutations": [
                (
                    "merge",
                    [("node", "c", ["Company"])],
                    [("c", "name", ("lit", "Acme"), "extra")],
                    [],
                )
            ]
        },
        "/mutations/0/2/0/3",
    ),
    (
        "merge-on-match",
        {
            "mutations": [
                (
                    "merge",
                    [("node", "c", ["Company"])],
                    [],
                    [("c", "name", ("lit", "Acme"), "extra")],
                )
            ]
        },
        "/mutations/0/3/0/3",
    ),
    (
        "projection-field",
        {"projections": [("field", "p", "name", None, "extra")]},
        "/projections/0/4",
    ),
    (
        "projection-agg",
        {"projections": [("agg", "p", "age", "count", None, "extra")]},
        "/projections/0/5",
    ),
    (
        "projection-expr",
        {"projections": [("expr", ("lit", 1), None, "extra")]},
        "/projections/0/3",
    ),
    (
        "order-by-variable",
        {"order_by": [("p", "name", True, "extra")]},
        "/order_by/0/3",
    ),
    (
        "order-by-expression",
        {"order_by": [(("ident", "n"), True, "extra")]},
        "/order_by/0/2",
    ),
    (
        "linear-let",
        {"linear_clauses": [("let", "x", ("lit", 1), "extra")]},
        "/linear_clauses/0/3",
    ),
    (
        "linear-filter",
        {"linear_clauses": [("filter", ("lit", True), "extra")]},
        "/linear_clauses/0/2",
    ),
    (
        "expression-prop",
        {"projections": [("expr", ("prop", "p", "name", "extra"), None)]},
        "/projections/0/1/3",
    ),
    (
        "expression-ident",
        {"projections": [("expr", ("ident", "n", "extra"), None)]},
        "/projections/0/1/2",
    ),
    (
        "expression-param",
        {"projections": [("expr", ("param", "n", "extra"), None)]},
        "/projections/0/1/2",
    ),
    (
        "expression-lit",
        {"projections": [("expr", ("lit", 1, "extra"), None)]},
        "/projections/0/1/2",
    ),
    (
        "expression-bin",
        {"projections": [("expr", ("bin", "eq", ("lit", 1), ("lit", 2), "x"), None)]},
        "/projections/0/1/4",
    ),
    (
        "expression-unary",
        {"projections": [("expr", ("unary", "not", ("lit", True), "x"), None)]},
        "/projections/0/1/3",
    ),
    (
        "expression-fn",
        {"projections": [("expr", ("fn", "size", [], "x"), None)]},
        "/projections/0/1/3",
    ),
    (
        "expression-case",
        {"projections": [("expr", ("case", None, [], None, "x"), None)]},
        "/projections/0/1/4",
    ),
    (
        "expression-case-branch",
        {
            "projections": [
                (
                    "expr",
                    ("case", None, [(("lit", 1), ("lit", 2), "x")], None),
                    None,
                )
            ]
        },
        "/projections/0/1/2/0/2",
    ),
    (
        "expression-list-comp",
        {
            "projections": [
                ("expr", ("list_comp", "x", ("prop", "p", "tags"), None, None, "x"), None)
            ]
        },
        "/projections/0/1/5",
    ),
    (
        "expression-pattern-comp",
        {
            "projections": [
                (
                    "expr",
                    ("pattern_comp", (("node", "f", ["Person"]),), None, ("ident", "f"), "x"),
                    None,
                )
            ]
        },
        "/projections/0/1/4",
    ),
    (
        "expression-exists",
        {
            "projections": [
                (
                    "expr",
                    (
                        "exists",
                        {"spec_version": 1, "matches": [{"paths": PERSON_PATH}]},
                        "x",
                    ),
                    None,
                )
            ]
        },
        "/projections/0/1/2",
    ),
    (
        "expression-count",
        {
            "projections": [
                (
                    "expr",
                    (
                        "count",
                        {"spec_version": 1, "matches": [{"paths": PERSON_PATH}]},
                        "x",
                    ),
                    None,
                )
            ]
        },
        "/projections/0/1/2",
    ),
    (
        "expression-list",
        {"projections": [("expr", ("list", [("lit", 1)], "x"), None)]},
        "/projections/0/1/2",
    ),
    (
        "expression-alias",
        {"projections": [("expr", ("alias", ("lit", 1), "x", "extra"), None)]},
        "/projections/0/1/3",
    ),
]


@pytest.mark.parametrize(
    ("spec", "path"),
    [case[1:] for case in ARITY_CASES],
    ids=[case[0] for case in ARITY_CASES],
)
def test_fixed_arity_arrays_reject_extra_items(spec: dict[str, Any], path: str) -> None:
    spec = {**BASE, **spec}
    error = assert_invalid_spec(spec, path)
    assert "accepts at most" in str(error)


# --- F9: alias names are strings and only take effect in projections


def test_non_string_alias_name_is_rejected() -> None:
    error = assert_invalid_spec(
        where_spec([("bin", "eq", ("alias", ("lit", 1), 42), ("lit", 1))]),
        "/matches/0/where/0/2/2",
    )
    assert "alias name" in str(error)


def test_alias_name_takes_effect_in_an_expr_projection() -> None:
    compiled = compile_cypher(
        {**BASE, "projections": [("expr", ("alias", ("lit", 1), "one"), None)]}
    )
    assert compiled["statement"] == "MATCH (p:Person) RETURN $p0 AS one"


def test_alias_name_outside_projection_uses_the_inner_expression() -> None:
    compiled = compile_cypher(where_spec([("bin", "eq", ("alias", ("lit", 1), "one"), ("lit", 1))]))
    assert "AS one" not in compiled["statement"]


# --- F16: an object in an expression position needs the wrapped form


def test_dict_expression_without_expr_key_is_rejected() -> None:
    error = assert_invalid_spec(where_spec({"spec_version": 1}), "/matches/0/where")
    assert "object-wrapped expression with an 'expr' key" in str(error)


def test_wrapped_expression_and_nested_subqueries_still_compile() -> None:
    wrapped = compile_cypher(
        {
            **BASE,
            "projections": [("expr", {"spec_version": 1, "expr": ("lit", 7)}, None)],
        }
    )
    assert wrapped["statement"] == "MATCH (p:Person) RETURN $p0"
    exists = compile_cypher(
        {
            **BASE,
            "projections": [
                (
                    "expr",
                    (
                        "exists",
                        {"spec_version": 1, "matches": [{"paths": PERSON_PATH}]},
                    ),
                    "e",
                )
            ],
        }
    )
    assert "EXISTS { MATCH (p:Person) }" in exists["statement"]


# --- F27: build/emission failures are VoyagerError without debug dumps


def test_sql_pgq_exists_failure_is_unsupported_feature_without_a_debug_dump() -> None:
    spec = where_spec([["exists", {"spec_version": 1, "matches": [{"paths": PERSON_PATH}]}]])
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "sql_pgq")
    error = exc_info.value
    assert error.code == "UNSUPPORTED_FEATURE"
    assert error.path == ""
    message = str(error)
    assert "does not support feature" in message
    assert "EXISTS subquery expressions" in message
    assert "QueryStatement(" not in message
    assert "QueryStatementData" not in message


def test_sql_pgq_unsupported_feature_keeps_its_code() -> None:
    spec = {
        **BASE,
        "unwinds": [("$batch", "row")],
    }
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "sql_pgq")
    error = exc_info.value
    assert error.code == "UNSUPPORTED_FEATURE"
    assert error.path == ""
    assert "UNSUPPORTED_FEATURE" not in str(error)  # message is human text


# --- F29: tuple parity for pattern_comp paths and exists/count subqueries


@pytest.mark.parametrize(
    ("expression", "fragment"),
    [
        (
            ("pattern_comp", (("node", "f", ["Person"]),), None, ("prop", "f", "name")),
            "[(f:Person) | f.name]",
        ),
        (
            ("exists", (("node", "f", ["Person"]),)),
            "EXISTS { MATCH (f:Person) }",
        ),
        (
            ("count", (("node", "f", ["Person"]),)),
            "COUNT { MATCH (f:Person) }",
        ),
    ],
    ids=["pattern_comp", "exists", "count"],
)
def test_tuple_payloads_are_accepted(expression: Any, fragment: str) -> None:
    compiled = compile_cypher({**BASE, "projections": [("expr", expression, "x")]})
    assert fragment in compiled["statement"]


# --- F30: empty variables and identifiers


@pytest.mark.parametrize(
    ("expression", "path"),
    [
        (("prop", "", "name"), "/matches/0/where/1"),
        (("ident", ""), "/matches/0/where/1"),
        (("param", ""), "/matches/0/where/1"),
    ],
    ids=["prop", "ident", "param"],
)
def test_empty_identifiers_are_rejected(expression: Any, path: str) -> None:
    error = assert_invalid_spec(where_spec(expression), path)
    assert "non-empty" in str(error)


def test_empty_prop_variable_inside_a_bin_is_rejected() -> None:
    error = assert_invalid_spec(
        where_spec([("bin", "eq", ("prop", "", "name"), ("lit", "x"))]),
        "/matches/0/where/0/2/1",
    )
    assert "non-empty" in str(error)


# --- F35: one leading '$' is stripped, matching the Python SDK's param()


def test_one_leading_dollar_is_stripped_from_parameters() -> None:
    with_marker = compile_cypher({**BASE, "projections": [("expr", ("param", "$name"), "n")]})
    without_marker = compile_cypher({**BASE, "projections": [("expr", ("param", "name"), "n")]})
    assert with_marker["statement"] == without_marker["statement"]
    assert with_marker["statement"].endswith("RETURN $name AS n")


def test_unwind_parameter_marker_is_stripped() -> None:
    compiled = compile_cypher(
        {
            "spec_version": 1,
            "unwinds": [("$batch", "row")],
            "projections": [("expr", ("ident", "row"), "r")],
        }
    )
    assert compiled["statement"] == "UNWIND $batch AS row RETURN row AS r"


# --- F35(b): every JSON spec example in spec/README.md compiles

README_PATH = Path(__file__).resolve().parents[3] / "spec" / "README.md"
CLAUSE_KEYS = {
    "load_csv",
    "unwinds",
    "matches",
    "mutations",
    "with_clauses",
    "projections",
    "linear_clauses",
}


def _readme_specs() -> list[dict[str, Any]]:
    text = README_PATH.read_text(encoding="utf-8")
    specs: list[dict[str, Any]] = []
    for block in re.findall(r"```json\s*\n(.*?)```", text, re.S):
        try:
            document = json.loads(block)
        except json.JSONDecodeError:
            # The README also contains expression fragments, not whole docs.
            continue
        if (
            isinstance(document, dict)
            and document.get("spec_version") == 1
            and CLAUSE_KEYS.intersection(document)
        ):
            specs.append(document)
    return specs


README_SPECS = _readme_specs()


def test_readme_has_a_compilable_spec_example() -> None:
    assert README_SPECS, "spec/README.md must contain at least one complete query spec"


@pytest.mark.filterwarnings("ignore:Dialect 'cypher' does not support explicit path search modes")
@pytest.mark.parametrize("dialect", ["cypher", "iso_gql", "age", "sql_pgq"])
@pytest.mark.parametrize("spec", README_SPECS, ids=range(len(README_SPECS)))
def test_readme_example_compiles(spec: dict[str, Any], dialect: str) -> None:
    try:
        compiled = compile_query_from_spec(json.loads(json.dumps(spec)), dialect)
    except VoyagerError as error:
        # ISO GQL and SQL:PGQ reject some constructs (for example LOAD CSV and
        # WITH pipelines) instead of emitting a partial statement.
        assert dialect in {"iso_gql", "sql_pgq"}
        assert error.code in {"UNSUPPORTED_FEATURE", "UNSUPPORTED_DIALECT"}
        return
    assert isinstance(compiled["statement"], str)


# --- F42 / F43: no Python repr, non-string keys report at the object path


def test_expression_errors_do_not_leak_a_python_repr() -> None:
    error = assert_invalid_spec(projection_spec([{"a": 1}]), "/projections/0/1")
    message = str(error)
    assert "got object" in message
    assert "{'a': 1}" not in message


def test_non_string_object_key_reports_at_the_object_path() -> None:
    spec: dict[Any, Any] = {"spec_version": 1, 42: "x", **BASE}
    error = assert_invalid_spec(spec, "")
    message = str(error)
    assert "object keys must be strings" in message
    assert "got number" in message
