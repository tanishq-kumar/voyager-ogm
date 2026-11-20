"""Query Spec v1 JSON round-trip conformance (issue #185).

The spec consumer must accept both spellings the protocol allows at every
array position: Python tuples (the SDK's native form) and Python lists (the
JSON array form produced by ``json.loads``/``json.dumps``). A spec that has
gone through a JSON round-trip must compile to exactly the same statement and
parameters as the tuple form, in every dialect that supports the construct.
"""

from __future__ import annotations

import json
from typing import Any

import pytest
from voyager_ogm import VoyagerError
from voyager_ogm._voyager_rs import compile_query_from_spec

DIALECTS = ("cypher", "iso_gql", "age", "sql_pgq")

# Dialects whose emitter genuinely renders each construct. Missing dialects
# are skipped in the round-trip matrix with the recorded reason: either the
# emitter raises UnsupportedFeature, or it silently drops an openCypher-only
# clause, so the construct is not exercised there either way.
SUPPORTED_DIALECTS: dict[str, tuple[str, ...]] = {
    "paths_where_projections": DIALECTS,
    "order_by": DIALECTS,
    "case_branches": DIALECTS,
    "load_csv": ("cypher", "age"),
    "unwinds": ("cypher", "iso_gql", "age"),
    "merge_sets": ("cypher", "iso_gql", "age"),
    "linear_clauses": ("cypher", "iso_gql", "age"),
    "with_clauses_order_by": ("cypher", "iso_gql", "age"),
}
SKIP_REASONS: dict[str, str] = {
    "load_csv": "LOAD CSV is openCypher-only; the ISO GQL and SQL:PGQ emitters drop the clause",
    "unwinds": "sql_pgq raises UnsupportedFeature for UNWIND (read-only GRAPH_TABLE operator)",
    "merge_sets": "sql_pgq raises UnsupportedFeature for DML mutations (read-only GRAPH_TABLE operator)",
    "linear_clauses": "sql_pgq raises UnsupportedFeature for LET/FILTER linear statements",
    "with_clauses_order_by": "sql_pgq raises UnsupportedFeature for WITH pipelines",
}

# One tuple-form spec per construct. Each is compiled twice below: verbatim
# (tuples) and after ``json.loads(json.dumps(spec))`` (lists everywhere).
PERSON_MATCH: list[dict[str, Any]] = [
    {
        "spec_version": 1,
        "paths": [[("node", "p", ["Person"])]],
        "where": [("bin", "gt", ("prop", "p", "age"), ("lit", 21))],
    }
]

CONSTRUCT_SPECS: dict[str, dict[str, Any]] = {
    # Baseline: paths, where and projections are already covered by the
    # tier-1 spec tests; included here to pin their tuple/list parity too.
    "paths_where_projections": {
        "spec_version": 1,
        "matches": PERSON_MATCH,
        "projections": [("field", "p", "name", "user_name")],
    },
    "order_by": {
        "spec_version": 1,
        "matches": PERSON_MATCH,
        "projections": [("field", "p", "name", "user_name")],
        "order_by": [
            ("p", "age", False),
            ("p", "name", True),
            # String direction spellings must behave like the boolean form.
            ("p", "name", "DESC"),
            (("fn", "size", [("prop", "p", "name")]), "ASC"),
        ],
    },
    "load_csv": {
        "spec_version": 1,
        "load_csv": ("file:///var/lib/graphs/nodes.csv", True, "row"),
        "matches": PERSON_MATCH,
        "projections": [("field", "row", "name", None)],
    },
    "unwinds": {
        "spec_version": 1,
        "unwinds": [("$batch", "row"), ("$extra", "item")],
        "matches": PERSON_MATCH,
        "projections": [("field", "p", "name", None)],
    },
    "case_branches": {
        "spec_version": 1,
        "matches": PERSON_MATCH,
        "projections": [
            (
                "expr",
                (
                    "case",
                    ("prop", "p", "age"),
                    [(("lit", 18), ("lit", "adult")), (("lit", 65), ("lit", "senior"))],
                    ("lit", "minor"),
                ),
                "age_group",
            )
        ],
    },
    "merge_sets": {
        "spec_version": 1,
        "matches": PERSON_MATCH,
        "mutations": [
            (
                "merge",
                [("node", "c", ["Company"])],
                [("c", "name", ("lit", "Acme")), ("c", "founded", ("lit", 2020))],
                [("c", "name", ("lit", "AcmeCorp"))],
            )
        ],
    },
    "linear_clauses": {
        "spec_version": 1,
        "linear_clauses": [
            ("let", "threshold", ("lit", 30)),
            ("filter", ("bin", "gt", ("ident", "threshold"), ("lit", 0))),
        ],
        "matches": PERSON_MATCH,
        "projections": [("field", "p", "name", None)],
    },
    "with_clauses_order_by": {
        "spec_version": 1,
        "matches": PERSON_MATCH,
        "with_clauses": [
            {
                "spec_version": 1,
                "projections": [
                    ("field", "p", "name", "n"),
                    ("agg", "p", "age", "avg", "avg_age"),
                ],
                "order_by": [
                    ("p", "name", True),
                    (("ident", "avg_age"), "DESC"),
                ],
                "skip": 0,
                "limit": 5,
            }
        ],
        "projections": [("expr", ("ident", "n"), None)],
    },
}


def assert_json_roundtrip(spec: dict[str, Any], dialect: str) -> None:
    """Assert a spec compiles identically before and after a JSON round-trip.

    Args:
        spec: A Query Spec v1 document using Python tuples at its array
            positions.
        dialect: Target dialect name accepted by ``compile_query_from_spec``.

    """
    tuple_form = compile_query_from_spec(spec, dialect)
    json_form = compile_query_from_spec(json.loads(json.dumps(spec)), dialect)
    assert json_form["statement"] == tuple_form["statement"], (
        f"JSON round-trip changed the {dialect} statement: "
        f"{tuple_form['statement']!r} != {json_form['statement']!r}"
    )
    assert json_form["parameters"] == tuple_form["parameters"], (
        f"JSON round-trip changed the {dialect} parameters: "
        f"{tuple_form['parameters']!r} != {json_form['parameters']!r}"
    )


@pytest.mark.parametrize("dialect", DIALECTS)
@pytest.mark.parametrize("construct", sorted(CONSTRUCT_SPECS))
def test_construct_json_roundtrip(construct: str, dialect: str) -> None:
    """Every construct compiles identically as JSON lists and as tuples."""
    if dialect not in SUPPORTED_DIALECTS[construct]:
        pytest.skip(SKIP_REASONS[construct])
    assert_json_roundtrip(CONSTRUCT_SPECS[construct], dialect)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_issue_185_repro(dialect: str) -> None:
    """The exact spec from issue #185 compiles after a JSON round-trip."""
    spec = {
        "spec_version": 1,
        "matches": [{"spec_version": 1, "paths": [[["node", "p", ["Person"]]]]}],
        "projections": [["field", "p", "name", "n"]],
        "order_by": [["p", "name", "ASC"]],
    }
    compiled = compile_query_from_spec(spec, dialect)
    if dialect == "cypher":
        assert compiled["statement"] == "MATCH (p:Person) RETURN p.name AS n ORDER BY p.name ASC"
    assert_json_roundtrip(
        {
            "spec_version": 1,
            "matches": [{"spec_version": 1, "paths": [[("node", "p", ["Person"])]]}],
            "projections": [("field", "p", "name", "n")],
            "order_by": [("p", "name", "ASC")],
        },
        dialect,
    )


@pytest.mark.parametrize("dialect", ("cypher", "age"))
def test_kitchen_sink_json_roundtrip(dialect: str) -> None:
    """A full document mixing every fixed construct round-trips identically.

    Cypher and Apache AGE are the only dialects whose emitters render every
    construct used here (LOAD CSV, UNWIND, WITH, MERGE ON CREATE/ON MATCH SET,
    LET/FILTER and ORDER BY), so the kitchen sink runs on those two only.
    """
    spec = {
        "spec_version": 1,
        "load_csv": ("file:///var/lib/graphs/nodes.csv", True, "row"),
        "unwinds": [("$batch", "row")],
        "matches": PERSON_MATCH,
        "linear_clauses": [("let", "threshold", ("lit", 30))],
        "with_clauses": [
            {
                "spec_version": 1,
                "projections": [("field", "p", "name", "n")],
                "order_by": [("p", "name", True)],
                "limit": 5,
            }
        ],
        "mutations": [
            (
                "merge",
                [("node", "c", ["Company"])],
                [("c", "name", ("lit", "Acme"))],
                [("c", "name", ("lit", "AcmeCorp"))],
            )
        ],
        "projections": [("field", "row", "name", None)],
        "order_by": [("row", "name", "ASC")],
        "skip": 0,
        "limit": 10,
    }
    assert_json_roundtrip(spec, dialect)


def test_mixed_tuple_and_list_arrays_compile_identically() -> None:
    """Array positions accept tuples and lists interchangeably, even mixed."""
    all_tuples = {
        "spec_version": 1,
        "matches": PERSON_MATCH,
        "with_clauses": [{"spec_version": 1, "projections": [("field", "p", "name", "n")]}],
        "projections": [("expr", ("ident", "n"), None)],
        "order_by": [("p", "name", True)],
    }
    # Same document, but every array position spelled as a JSON list.
    all_lists = json.loads(json.dumps(all_tuples))
    # Mixed spellings inside one document must behave like either pure form:
    # tuple containers holding list items and vice versa. (With-clause and
    # match entries stay dicts: they are JSON objects, not array positions.)
    mixed = {
        "spec_version": 1,
        "matches": [
            {
                "spec_version": 1,
                # Path steps as lists.
                "paths": [[["node", "p", ["Person"]]]],
                # Predicate list holding one tuple expression.
                "where": [["bin", "gt", ("prop", "p", "age"), ["lit", 21]]],
            }
        ],
        # With-clauses container as a tuple, its projections as lists.
        "with_clauses": ({"spec_version": 1, "projections": [["field", "p", "name", "n"]]},),
        "projections": [["expr", ("ident", "n"), None]],
        "order_by": [("p", "name", True)],
    }
    expected = compile_query_from_spec(all_tuples, "cypher")
    for name, spec in (("all_lists", all_lists), ("mixed", mixed)):
        compiled = compile_query_from_spec(spec, "cypher")
        assert compiled["statement"] == expected["statement"], name
        assert compiled["parameters"] == expected["parameters"], name


@pytest.mark.parametrize(
    ("spec", "field", "path"),
    [
        # load_csv: non-sequence and wrong length.
        ({"spec_version": 1, "load_csv": "file:///a.csv"}, "load_csv", "/load_csv"),
        (
            {"spec_version": 1, "load_csv": ("file:///a.csv", True)},
            "load_csv",
            "/load_csv",
        ),
        (
            {"spec_version": 1, "load_csv": ["file:///a.csv", True]},
            "load_csv",
            "/load_csv",
        ),
        # unwinds entry: wrong length.
        ({"spec_version": 1, "unwinds": [("$batch",)]}, "unwinds entry", "/unwinds/0"),
        ({"spec_version": 1, "unwinds": [["$batch"]]}, "unwinds entry", "/unwinds/0"),
        # case branch: wrong length (branch needs [when, then]).
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "projections": [("expr", ("case", None, [(("lit", 1),)], None), None)],
            },
            "case branch",
            "/projections/0/1/2/0",
        ),
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "projections": [("expr", ["case", None, [[["lit", 1]]], None], None)],
            },
            "case branch",
            "/projections/0/1/2/0",
        ),
        # merge ON CREATE / ON MATCH set entry: wrong length.
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "mutations": [
                    (
                        "merge",
                        [("node", "c", ["Company"])],
                        [("c", "name")],
                        [],
                    )
                ],
            },
            "merge 'on_create_set' entry",
            "/mutations/0/2/0",
        ),
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "mutations": [("merge", [("node", "c", ["Company"])], [], [["c", "name"]])],
            },
            "merge 'on_match_set' entry",
            "/mutations/0/3/0",
        ),
        # order_by entry: wrong length, non-sequence and bad direction spelling.
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "projections": [("field", "p", "name", None)],
                "order_by": [("p", "name")],
            },
            "order_by entry",
            "/order_by/0",
        ),
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "projections": [("field", "p", "name", None)],
                "order_by": [["p", "name"]],
            },
            "order_by entry",
            "/order_by/0",
        ),
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "projections": [("field", "p", "name", None)],
                "order_by": [["p", "name", "sideways"]],
            },
            "order_by entry",
            "/order_by/0/2",
        ),
        (
            {
                "spec_version": 1,
                "matches": PERSON_MATCH,
                "projections": [("field", "p", "name", None)],
                "order_by": [42],
            },
            "order_by entry",
            "/order_by/0",
        ),
        # linear_clauses entry: wrong length.
        (
            {"spec_version": 1, "linear_clauses": [("let", "x")]},
            "linear clause 'let'",
            "/linear_clauses/0",
        ),
        (
            {"spec_version": 1, "linear_clauses": [["filter"]]},
            "linear clause 'filter'",
            "/linear_clauses/0",
        ),
        # path step: wrong length (edge needs all six items).
        (
            {
                "spec_version": 1,
                "matches": [
                    {
                        "spec_version": 1,
                        "paths": [[("node", "p", ["Person"]), ("edge", "out")]],
                    }
                ],
                "projections": [("field", "p", "name", None)],
            },
            "path step 'edge'",
            "/matches/0/paths/0/1",
        ),
    ],
)
def test_wrong_shapes_raise_clear_field_named_errors(
    spec: dict[str, Any], field: str, path: str
) -> None:
    """Malformed array shapes fail with INVALID_SPEC naming the field and path."""
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "cypher")
    assert exc_info.value.code == "INVALID_SPEC"
    assert isinstance(exc_info.value, ValueError)
    assert exc_info.value.path == path
    assert field in str(exc_info.value)
    assert path in str(exc_info.value)


ORDER_BY_ENTRY_FORMS = {
    "property": lambda direction: ("p", "name", direction),
    "expression": lambda direction: (("prop", "p", "name"), direction),
}


def _order_by_spec(location: str, entry: Any) -> dict[str, Any]:
    """Build a compilable spec whose only ORDER BY entry is ``entry``."""
    if location == "root":
        return {
            "spec_version": 1,
            "matches": PERSON_MATCH,
            "projections": [("field", "p", "name", "user_name")],
            "order_by": [entry],
        }
    return {
        "spec_version": 1,
        "matches": PERSON_MATCH,
        "with_clauses": [
            {
                "spec_version": 1,
                "projections": [("field", "p", "name", "n")],
                "order_by": [entry],
            }
        ],
        "projections": [("expr", ("ident", "n"), None)],
    }


@pytest.mark.parametrize("location", ("root", "with_clauses"))
@pytest.mark.parametrize("variant", sorted(ORDER_BY_ENTRY_FORMS))
def test_order_by_direction_boolean_and_uppercase_strings_are_equivalent(
    location: str, variant: str
) -> None:
    """true/false and "ASC"/"DESC" compile identically for both entry variants."""
    make_entry = ORDER_BY_ENTRY_FORMS[variant]
    for boolean, spelling in ((True, "ASC"), (False, "DESC")):
        boolean_compiled = compile_query_from_spec(
            _order_by_spec(location, make_entry(boolean)), "cypher"
        )
        string_compiled = compile_query_from_spec(
            _order_by_spec(location, make_entry(spelling)), "cypher"
        )
        assert string_compiled["parameters"] == boolean_compiled["parameters"]
        assert string_compiled["statement"] == boolean_compiled["statement"]
        assert spelling in boolean_compiled["statement"]
        json_compiled = compile_query_from_spec(
            json.loads(json.dumps(_order_by_spec(location, make_entry(spelling)))),
            "cypher",
        )
        assert json_compiled["statement"] == boolean_compiled["statement"]


@pytest.mark.parametrize("location", ("root", "with_clauses"))
@pytest.mark.parametrize("direction", ["asc", "desc", "Asc", "ASC ", "up", 1], ids=repr)
def test_order_by_rejects_values_the_schema_does_not_allow(location: str, direction: Any) -> None:
    """Only booleans and uppercase "ASC"/"DESC" are accepted, as the schema says."""
    spec = _order_by_spec(location, ("p", "name", direction))
    prefix = "/order_by" if location == "root" else "/with_clauses/0/order_by"
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "cypher")
    assert exc_info.value.code == "INVALID_SPEC"
    assert exc_info.value.path == f"{prefix}/0/2"
    assert "order_by" in str(exc_info.value)


def test_expression_array_positions_accept_tuples_and_lists() -> None:
    """Expression-level arrays (fn args, case branches, list items) accept both."""
    base = {"spec_version": 1, "matches": PERSON_MATCH}
    cases = {
        "fn args as list": ("expr", ("fn", "size", [("prop", "p", "name")]), None),
        "fn args as tuple": ("expr", ("fn", "size", (("prop", "p", "name"),)), None),
        "case branches as list of lists": (
            "expr",
            ("case", None, [(("lit", 1), ("lit", "a"))], ("lit", "b")),
            None,
        ),
        "case branches as tuple of tuples": (
            "expr",
            ("case", None, ((("lit", 1), ("lit", "a")),), ("lit", "b")),
            None,
        ),
        "list items as list": ("expr", ("list", [("lit", 1), ("lit", 2)]), None),
        "list items as tuple": ("expr", ("list", (("lit", 1), ("lit", 2))), None),
    }
    statements = {}
    for name, projection in cases.items():
        compiled = compile_query_from_spec({**base, "projections": [projection]}, "cypher")
        statements[name] = (compiled["statement"], compiled["parameters"])
    assert statements["fn args as list"] == statements["fn args as tuple"]
    assert (
        statements["case branches as list of lists"]
        == statements["case branches as tuple of tuples"]
    )
    assert statements["list items as list"] == statements["list items as tuple"]


def test_alias_expression_keeps_its_alias_in_both_spellings() -> None:
    """An `['alias', expr, name]` projection applies the alias for lists too."""
    tuple_spec = {
        "spec_version": 1,
        "matches": PERSON_MATCH,
        "projections": [("expr", ("alias", ("prop", "p", "name"), "n"), None)],
    }
    list_spec = json.loads(json.dumps(tuple_spec))
    tuple_stmt = compile_query_from_spec(tuple_spec, "cypher")["statement"]
    list_stmt = compile_query_from_spec(list_spec, "cypher")["statement"]
    assert tuple_stmt == list_stmt
    assert "RETURN p.name AS n" in tuple_stmt


def test_tuple_spelling_behaviour_preserved() -> None:
    """Tuple forms keep their pre-existing tolerance and expression rule.

    Pins the behaviours the fix must not regress: four-item node tuples are
    still tolerated and a bare tuple ``where`` is still one expression.
    """
    # 4-item node step (documented 3-item shape plus an ignored fourth) still
    # compiles, matching the existing tier-1 spec tests.
    compiled = compile_query_from_spec(
        {
            "spec_version": 1,
            "matches": [{"spec_version": 1, "paths": [[("node", "p", ["Person"], None)]]}],
            "projections": [("field", "p", "name", None)],
        },
        "cypher",
    )
    assert compiled["statement"] == "MATCH (p:Person) RETURN p.name"

    # `where` as a bare tuple is a single expression, `where` as a list is a
    # list of expressions: unchanged semantics.
    bare = compile_query_from_spec(
        {
            "spec_version": 1,
            "matches": [
                {
                    "spec_version": 1,
                    "paths": [[("node", "p", ["Person"])]],
                    "where": ("bin", "gt", ("prop", "p", "age"), ("lit", 21)),
                }
            ],
            "projections": [("field", "p", "name", None)],
        },
        "cypher",
    )
    assert "WHERE p.age > $p0" in bare["statement"]


def test_bare_expression_arrays_are_rejected_and_explicit_lists_work() -> None:
    """No implicit literal lists: every expression array must be tagged.

    See spec/README.md: a literal list is written as ``["lit", [...]]`` and a
    list whose items are expressions as ``["list", [...]]``.
    """
    base = {
        "spec_version": 1,
        "matches": PERSON_MATCH,
    }

    for expression in ([1, 2, 3], ["red", "green"], [], ["propp", "x"]):
        with pytest.raises(VoyagerError) as exc_info:
            compile_query_from_spec(
                {**base, "projections": [("expr", expression, "xs")]},
                "cypher",
            )
        assert exc_info.value.code == "INVALID_SPEC"
        assert exc_info.value.path == "/projections/0/1"

    # The tuple spelling is a sequence too, so an unknown tag is rejected.
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(
            {**base, "projections": [("expr", ("propp", "p", "age"), None)]},
            "cypher",
        )
    assert exc_info.value.code == "INVALID_SPEC"
    assert exc_info.value.path == "/projections/0/1"
    assert "Unknown expression tag 'propp'" in str(exc_info.value)

    # Explicit literal and expression list forms still compile.
    numeric = compile_query_from_spec(
        {**base, "projections": [("expr", ["lit", [1, 2, 3]], "xs")]},
        "cypher",
    )
    assert numeric["statement"].endswith("AS xs")
    assert [1, 2, 3] in numeric["parameters"].values()

    strings = compile_query_from_spec(
        {**base, "projections": [("expr", ["lit", ["red", "green"]], "xs")]},
        "cypher",
    )
    assert strings["statement"].endswith("AS xs")
    assert ["red", "green"] in strings["parameters"].values()

    expressions = compile_query_from_spec(
        {
            **base,
            "projections": [("expr", ["list", [["lit", 1], ["lit", 2], ["lit", 3]]], "xs")],
        },
        "cypher",
    )
    assert numeric["statement"] == "MATCH (p:Person) WHERE p.age > $p0 RETURN $p1 AS xs"
    assert numeric["parameters"] == {"p0": 21, "p1": [1, 2, 3]}
    assert strings["statement"] == "MATCH (p:Person) WHERE p.age > $p0 RETURN $p1 AS xs"
    assert strings["parameters"] == {"p0": 21, "p1": ["red", "green"]}
    assert (
        expressions["statement"]
        == "MATCH (p:Person) WHERE p.age > $p0 RETURN [$p1, $p2, $p3] AS xs"
    )
    assert expressions["parameters"] == {"p0": 21, "p1": 1, "p2": 2, "p3": 3}
