"""Identifier injection regression tests for the Query Spec consumer (#186)."""

from __future__ import annotations

from typing import Any

import pytest
from voyager_ogm._voyager_rs import compile_query_from_spec

DIALECTS = ["cypher", "age", "iso_gql", "sql_pgq"]

LABEL_PAYLOAD = "Person) DETACH DELETE p //"
PARAM_PAYLOAD = "x RETURN 1 //"
PROP_PAYLOAD = "name} RETURN 1 //"
FUNC_PAYLOAD = "toLower(1)) RETURN 1 //"


def _dialect_kwargs(dialect: str) -> dict[str, Any]:
    return {"graph_name": "graph_name"} if dialect == "sql_pgq" else {}


def _compile(spec: dict[str, Any], dialect: str, *, optimize: bool = False) -> Any:
    return compile_query_from_spec(
        spec,
        dialect,
        use_cache=False,
        optimize=optimize,
        **_dialect_kwargs(dialect),
    )


def _assert_rejected(
    spec: dict[str, Any], dialect: str, kind: str, payload: str, *, optimize: bool = False
) -> None:
    with pytest.raises(ValueError) as exc_info:
        _compile(spec, dialect, optimize=optimize)
    message = str(exc_info.value)
    assert "Invalid" in message, message
    assert kind in message, message
    assert payload in message, message


def _base_spec(**overrides: Any) -> dict[str, Any]:
    spec: dict[str, Any] = {
        "spec_version": 1,
        "matches": [
            {
                "spec_version": 1,
                "paths": [[("node", "p", ["Person"], None)]],
            }
        ],
        "projections": [["field", "p", "name", None]],
    }
    spec.update(overrides)
    return spec


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_node_variable(dialect: str) -> None:
    spec = _base_spec(
        matches=[
            {
                "spec_version": 1,
                "paths": [[("node", LABEL_PAYLOAD, ["Person"], None)]],
            }
        ]
    )
    _assert_rejected(spec, dialect, "node variable", LABEL_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_node_label(dialect: str) -> None:
    spec = _base_spec(
        matches=[
            {
                "spec_version": 1,
                "paths": [[("node", "p", [LABEL_PAYLOAD], None)]],
            }
        ]
    )
    _assert_rejected(spec, dialect, "node label", LABEL_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_edge_variable(dialect: str) -> None:
    spec = _base_spec(
        matches=[
            {
                "spec_version": 1,
                "paths": [
                    [
                        ("node", "p", ["Person"], None),
                        ("edge", "outgoing", ["KNOWS"], LABEL_PAYLOAD, None, None),
                        ("node", "q", ["Person"], None),
                    ]
                ],
            }
        ]
    )
    _assert_rejected(spec, dialect, "edge variable", LABEL_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_relationship_type(dialect: str) -> None:
    spec = _base_spec(
        matches=[
            {
                "spec_version": 1,
                "paths": [
                    [
                        ("node", "p", ["Person"], None),
                        ("edge", "outgoing", [LABEL_PAYLOAD], "r", None, None),
                        ("node", "q", ["Person"], None),
                    ]
                ],
            }
        ]
    )
    _assert_rejected(spec, dialect, "relationship type", LABEL_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_property_read(dialect: str) -> None:
    spec = _base_spec(
        matches=[
            {
                "spec_version": 1,
                "paths": [[("node", "p", ["Person"], None)]],
                "where": [["bin", "eq", ["prop", "p", PROP_PAYLOAD], ["lit", 1]]],
            }
        ]
    )
    _assert_rejected(spec, dialect, "property name", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_inline_property_map(dialect: str) -> None:
    spec = _base_spec(
        matches=[
            {
                "spec_version": 1,
                "paths": [[("node", "p", ["Person"], None)]],
                "where": [["bin", "eq", ["prop", "p", PROP_PAYLOAD], ["lit", 1]]],
            }
        ]
    )
    _assert_rejected(spec, dialect, "property name", PROP_PAYLOAD, optimize=True)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_projection_alias(dialect: str) -> None:
    spec = _base_spec(projections=[["field", "p", "name", PROP_PAYLOAD]])
    _assert_rejected(spec, dialect, "alias", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_path_variable(dialect: str) -> None:
    spec = _base_spec(
        matches=[
            {
                "spec_version": 1,
                "path_variable": PROP_PAYLOAD,
                "paths": [[("node", "p", ["Person"], None)]],
            }
        ]
    )
    _assert_rejected(spec, dialect, "path variable", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_unwind_alias(dialect: str) -> None:
    spec = _base_spec(unwinds=[("batch", PROP_PAYLOAD)])
    _assert_rejected(spec, dialect, "unwind alias", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_let_variable(dialect: str) -> None:
    spec = _base_spec(linear_clauses=[("let", PROP_PAYLOAD, ("lit", 1))])
    _assert_rejected(spec, dialect, "let variable", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_delete_target(dialect: str) -> None:
    spec = _base_spec(mutations=[["delete", True, [PROP_PAYLOAD]]])
    _assert_rejected(spec, dialect, "delete target", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_parameter_name(dialect: str) -> None:
    spec = _base_spec(
        matches=[
            {
                "spec_version": 1,
                "paths": [[("node", "p", ["Person"], None)]],
                "where": [["bin", "eq", ["prop", "p", "name"], ["param", PARAM_PAYLOAD]]],
            }
        ]
    )
    _assert_rejected(spec, dialect, "parameter name", PARAM_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_function_name(dialect: str) -> None:
    spec = _base_spec(projections=[["expr", ["fn", FUNC_PAYLOAD, [["prop", "p", "name"]]], None]])
    _assert_rejected(spec, dialect, "function name", FUNC_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_set_key(dialect: str) -> None:
    spec = _base_spec(mutations=[["set", "p", PROP_PAYLOAD, ["lit", 1]]])
    _assert_rejected(spec, dialect, "property name", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_remove_key(dialect: str) -> None:
    spec = _base_spec(mutations=[["remove", "p", PROP_PAYLOAD]])
    _assert_rejected(spec, dialect, "property name", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_merge_set_key(dialect: str) -> None:
    spec = _base_spec(
        mutations=[
            (
                "merge",
                [("node", "p", ["Person"], None)],
                [("p", PROP_PAYLOAD, ("lit", 1))],
                [],
            )
        ]
    )
    _assert_rejected(spec, dialect, "property name", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_rejects_injection_in_load_csv_alias(dialect: str) -> None:
    spec = _base_spec(load_csv=("file:///data.csv", True, PROP_PAYLOAD))
    _assert_rejected(spec, dialect, "load csv alias", PROP_PAYLOAD)


@pytest.mark.parametrize("dialect", DIALECTS)
def test_issue_186_parameter_repro_raises(dialect: str) -> None:
    """The exact issue #186 repro must now raise instead of emitting injected text."""
    spec = {
        "spec_version": 1,
        "matches": [
            {
                "paths": [[("node", "p", ["Person"], None)]],
                "where": [["bin", "eq", ["prop", "p", "name"], ["param", PARAM_PAYLOAD]]],
            }
        ],
        "projections": [["field", "p", "name", "n"]],
    }
    _assert_rejected(spec, dialect, "parameter name", PARAM_PAYLOAD)
