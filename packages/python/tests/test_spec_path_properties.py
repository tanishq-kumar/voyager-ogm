"""Query Spec v1 node and relationship properties in path steps (issue #263).

Path steps accept an optional properties object: the 4th item of a node step
and the 7th item of an edge step. Values are Expressions, so a property may be a
literal or a parameter. The properties must reach every clause and dialect;
SQL:PGQ has no inline property map and renders the same values as predicates
inside its node/edge pattern instead.
"""

from __future__ import annotations

import json
from typing import Any

import pytest
from voyager_ogm import VoyagerError
from voyager_ogm._voyager_rs import NativeQueryBuilder, compile_query_from_spec

PERSON_AND_KNOWS_PATH = (
    '[["node", "a", ["Person"], {"name": ["lit", "Alice"]}], '
    '["edge", "outgoing", ["KNOWS"], "r", null, null, {"since": ["lit", 2020]}], '
    '["node", "b", ["Person"]]]'
)


def _match_spec(path_json: str, *, optional: bool = False) -> dict[str, Any]:
    optional_json = "true" if optional else "false"
    return json.loads(
        '{"spec_version": 1, "matches": [{"spec_version": 1, "optional": '
        + optional_json
        + ', "paths": ['
        + path_json
        + ']}], "projections": [["field", "b", "name", "n"]]}'
    )


def _cypher(spec: dict[str, Any]) -> dict[str, Any]:
    return compile_query_from_spec(spec, "cypher")


def _assert_invalid_spec(spec: dict[str, Any], path: str) -> VoyagerError:
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "cypher")
    error = exc_info.value
    assert error.code == "INVALID_SPEC"
    assert error.path == path
    assert path in str(error)
    return error


# --- Clauses ----------------------------------------------------------------


def test_create_node_properties_literal() -> None:
    """The issue's acceptance example compiles to an inline property map."""
    spec = json.loads(
        '{"spec_version": 1, "mutations": [["create", [['
        '["node", "a", ["Person"], {"name": ["lit", "Alice"]}]'
        "]]]]}"
    )
    compiled = _cypher(spec)
    assert compiled["statement"] == "CREATE (a:Person {name: $p0})"
    assert compiled["parameters"] == {"p0": "Alice"}


def test_match_node_properties() -> None:
    spec = _match_spec('[["node", "b", ["Person"], {"name": ["lit", "Alice"]}]]')
    compiled = _cypher(spec)
    assert compiled["statement"] == ("MATCH (b:Person {name: $p0}) RETURN b.name AS n")
    assert compiled["parameters"] == {"p0": "Alice"}


def test_optional_match_node_properties() -> None:
    spec = _match_spec('[["node", "b", ["Person"], {"city": ["lit", "Berlin"]}]]', optional=True)
    compiled = _cypher(spec)
    assert compiled["statement"] == ("OPTIONAL MATCH (b:Person {city: $p0}) RETURN b.name AS n")
    assert compiled["parameters"] == {"p0": "Berlin"}


def test_merge_key_properties() -> None:
    """MERGE uses the same inline property map for its key properties."""
    spec = json.loads(
        '{"spec_version": 1, "mutations": [["merge", ['
        '["node", "p", ["Person"], {"email": ["param", "email"], '
        '"name": ["lit", "Alice"]}]'
        "], [], []]]}"
    )
    compiled = _cypher(spec)
    assert compiled["statement"] == "MERGE (p:Person {email: $email, name: $p0})"
    assert compiled["parameters"] == {"p0": "Alice"}


# --- Edges and hops ---------------------------------------------------------


def test_match_node_and_edge_properties() -> None:
    compiled = _cypher(_match_spec(PERSON_AND_KNOWS_PATH))
    assert compiled["statement"] == (
        "MATCH (a:Person {name: $p0})-[r:KNOWS {since: $p1}]->(b:Person) RETURN b.name AS n"
    )
    assert compiled["parameters"] == {"p0": "Alice", "p1": 2020}


@pytest.mark.parametrize(
    ("edge_json", "expected"),
    [
        (
            '["edge", "outgoing", ["KNOWS"], "r", null, null, {"since": ["lit", 2020]}]',
            "MATCH (a:Person)-[r:KNOWS {since: $p0}]->(b:Person) RETURN b.name AS n",
        ),
        (
            '["edge", "incoming", ["KNOWS"], "r", null, null, {"since": ["lit", 2020]}]',
            "MATCH (a:Person)<-[r:KNOWS {since: $p0}]-(b:Person) RETURN b.name AS n",
        ),
        (
            '["edge", "undirected", ["KNOWS"], "r", null, null, {"since": ["lit", 2020]}]',
            "MATCH (a:Person)-[r:KNOWS {since: $p0}]-(b:Person) RETURN b.name AS n",
        ),
    ],
)
def test_edge_properties_without_hops(edge_json: str, expected: str) -> None:
    spec = _match_spec('[["node", "a", ["Person"]], ' + edge_json + ', ["node", "b", ["Person"]]]')
    compiled = _cypher(spec)
    assert compiled["statement"] == expected
    assert compiled["parameters"] == {"p0": 2020}


def test_edge_properties_combine_with_hops() -> None:
    spec = _match_spec(
        '[["node", "a", ["Person"]], '
        '["edge", "outgoing", ["KNOWS"], "r", 1, 3, {"since": ["lit", 2020]}], '
        '["node", "b", ["Person"]]]'
    )
    compiled = _cypher(spec)
    assert compiled["statement"] == (
        "MATCH (a:Person)-[r:KNOWS*1..3 {since: $p0}]->(b:Person) RETURN b.name AS n"
    )
    assert compiled["parameters"] == {"p0": 2020}


# --- Values and optionality -------------------------------------------------


def test_literal_and_parameter_property_values() -> None:
    spec = json.loads(
        '{"spec_version": 1, "matches": [{"spec_version": 1, "paths": [['
        '["node", "p", ["Person"], {"city": ["lit", "Berlin"], '
        '"email": ["param", "email"]}]'
        ']]}], "projections": [["field", "p", "name", "n"]]}'
    )
    compiled = _cypher(spec)
    assert compiled["statement"] == (
        "MATCH (p:Person {city: $p0, email: $email}) RETURN p.name AS n"
    )
    # A parameter reference is supplied by the caller, not compiled to a literal.
    assert compiled["parameters"] == {"p0": "Berlin"}


def test_omitted_null_and_empty_properties_are_identical() -> None:
    """Omitted, explicit null and {} all mean "no properties"."""
    specs = [
        _match_spec('[["node", "b", ["Person"]]]'),
        _match_spec('[["node", "b", ["Person"], null]]'),
        _match_spec('[["node", "b", ["Person"], {}]]'),
    ]
    for dialect in ("cypher", "age", "iso_gql", "sql_pgq"):
        compiled = [compile_query_from_spec(spec, dialect) for spec in specs]
        assert compiled[0] == compiled[1] == compiled[2]


# --- Rejections -------------------------------------------------------------


def test_bare_scalar_property_value_is_rejected_with_expression_hint() -> None:
    error = _assert_invalid_spec(
        _match_spec('[["node", "b", ["Person"], {"name": "Alice"}]]'),
        "/matches/0/paths/0/0/3/name",
    )
    assert "got string" in str(error)
    assert '["lit", value]' in str(error)


@pytest.mark.parametrize(
    ("path_json", "path"),
    [
        ('[["node", "b", ["Person"], "Alice"]]', "/matches/0/paths/0/0/3"),
        (
            '[["node", "b", ["Person"]], '
            '["edge", "out", ["KNOWS"], null, null, null, ["since", 2020]]]',
            "/matches/0/paths/0/1/6",
        ),
    ],
)
def test_non_object_properties_are_rejected(path_json: str, path: str) -> None:
    error = _assert_invalid_spec(_match_spec(path_json), path)
    assert "expected a JSON object" in str(error)


@pytest.mark.parametrize(
    ("path_json", "path"),
    [
        (
            '[["node", "b", ["Person"], {"name": ["lit", "x"]}, 1]]',
            "/matches/0/paths/0/0/4",
        ),
        (
            '[["node", "b", ["Person"]], '
            '["edge", "out", ["KNOWS"], null, null, null, '
            '{"since": ["lit", 2020]}, 1]]',
            "/matches/0/paths/0/1/7",
        ),
    ],
)
def test_items_past_the_new_shape_are_rejected(path_json: str, path: str) -> None:
    error = _assert_invalid_spec(_match_spec(path_json), path)
    assert "accepts no further items" in str(error)


# --- Dialects ---------------------------------------------------------------

MATCH_NODE_PROPS_SPEC = _match_spec('[["node", "b", ["Person"], {"name": ["lit", "Alice"]}]]')
MATCH_PARAM_PROP_SPEC = _match_spec('[["node", "b", ["Person"], {"email": ["param", "email"]}]]')
MATCH_NODE_EDGE_PROPS_SPEC = _match_spec(PERSON_AND_KNOWS_PATH)
EDGE_PROPS_HOPS_SPEC = _match_spec(
    '[["node", "a", ["Person"]], '
    '["edge", "outgoing", ["KNOWS"], "r", 1, 3, {"since": ["lit", 2020]}], '
    '["node", "b", ["Person"]]]'
)


@pytest.mark.parametrize(
    ("spec", "dialect", "expected"),
    [
        (
            MATCH_NODE_PROPS_SPEC,
            "age",
            "SELECT * FROM cypher('age_graph', $$ MATCH (b:Person {name: $p0}) "
            "RETURN b.name AS n $$, %s) AS (n agtype)",
        ),
        (
            MATCH_NODE_PROPS_SPEC,
            "iso_gql",
            "MATCH (b:Person {name: $p0}) RETURN b.name AS n",
        ),
        (
            MATCH_NODE_PROPS_SPEC,
            "sql_pgq",
            "SELECT * FROM GRAPH_TABLE (graph_table MATCH "
            "(b IS Person WHERE b.name = $p0) COLUMNS (b.name AS n))",
        ),
        (
            MATCH_PARAM_PROP_SPEC,
            "sql_pgq",
            "SELECT * FROM GRAPH_TABLE (graph_table MATCH "
            "(b IS Person WHERE b.email = $email) COLUMNS (b.name AS n))",
        ),
        (
            MATCH_NODE_EDGE_PROPS_SPEC,
            "sql_pgq",
            "SELECT * FROM GRAPH_TABLE (graph_table MATCH "
            "(a IS Person WHERE a.name = $p0) "
            "-[r IS KNOWS WHERE r.since = $p1]-> "
            "(b IS Person) COLUMNS (b.name AS n))",
        ),
        (
            EDGE_PROPS_HOPS_SPEC,
            "iso_gql",
            "MATCH ((a:Person)-[r:KNOWS {since: $p0}]->(b:Person)){1,3} RETURN b.name AS n",
        ),
        (
            EDGE_PROPS_HOPS_SPEC,
            "sql_pgq",
            "SELECT * FROM GRAPH_TABLE (graph_table MATCH (a IS Person) "
            "-[r IS KNOWS WHERE r.since = $p0]->{1,3} (b IS Person) "
            "COLUMNS (b.name AS n))",
        ),
        (
            json.loads(
                '{"spec_version": 1, "mutations": [["create", [['
                '["node", "a", ["Person"], {"name": ["lit", "Alice"]}]'
                "]]]]}"
            ),
            "age",
            "SELECT * FROM cypher('age_graph', $$ CREATE (a:Person {name: $p0}) $$, "
            "%s) AS (result agtype)",
        ),
        (
            json.loads(
                '{"spec_version": 1, "mutations": [["create", [['
                '["node", "a", ["Person"], {"name": ["lit", "Alice"]}]'
                "]]]]}"
            ),
            "iso_gql",
            "INSERT (a:Person {name: $p0})",
        ),
        (
            json.loads(
                '{"spec_version": 1, "mutations": [["merge", ['
                '["node", "p", ["Person"], {"email": ["param", "email"]}]'
                "], [], []]]}"
            ),
            "age",
            "SELECT * FROM cypher('age_graph', $$ MERGE (p:Person "
            "{email: $email}) $$, %s) AS (result agtype)",
        ),
        (
            json.loads(
                '{"spec_version": 1, "mutations": [["merge", ['
                '["node", "p", ["Person"], {"email": ["param", "email"]}]'
                "], [], []]]}"
            ),
            "iso_gql",
            "UPSERT (p:Person {email: $email})",
        ),
    ],
)
def test_properties_per_dialect(spec: dict[str, Any], dialect: str, expected: str) -> None:
    assert compile_query_from_spec(spec, dialect)["statement"] == expected


@pytest.mark.parametrize(
    "spec",
    [
        json.loads(
            '{"spec_version": 1, "mutations": [["create", [['
            '["node", "a", ["Person"], {"name": ["lit", "Alice"]}]'
            "]]]]}"
        ),
        json.loads(
            '{"spec_version": 1, "mutations": [["merge", ['
            '["node", "p", ["Person"], {"email": ["lit", "a@b.c"]}]'
            "], [], []]]}"
        ),
    ],
)
def test_sql_pgq_raises_for_dml_properties_instead_of_dropping_them(
    spec: dict[str, Any],
) -> None:
    with pytest.raises(ValueError, match="does not support feature: DML mutations"):
        compile_query_from_spec(spec, "sql_pgq")


# --- Subqueries and pattern comprehensions ---------------------------------


def _filtered_subquery() -> dict[str, Any]:
    return {
        "spec_version": 1,
        "matches": [
            {
                "spec_version": 1,
                "paths": [
                    [
                        ["node", "p", ["Person"]],
                        [
                            "edge",
                            "outgoing",
                            ["KNOWS"],
                            None,
                            None,
                            None,
                            {"since": ["lit", 2020]},
                        ],
                        ["node", "f", ["Person"], {"city": ["lit", "Berlin"]}],
                    ]
                ],
            }
        ],
    }


@pytest.mark.parametrize(
    ("expression", "expected"),
    [
        (
            ["exists", _filtered_subquery()],
            "MATCH (p:Person) RETURN EXISTS { MATCH (p:Person)-[:KNOWS {since: $p0}]"
            "->(f:Person {city: $p1}) } AS n",
        ),
        (
            ["count", _filtered_subquery()],
            "MATCH (p:Person) RETURN COUNT { MATCH (p:Person)-[:KNOWS {since: $p0}]"
            "->(f:Person {city: $p1}) } AS n",
        ),
    ],
)
def test_subqueries_carry_path_properties(expression: list[Any], expected: str) -> None:
    spec = json.loads(
        '{"spec_version": 1, "matches": [{"spec_version": 1, '
        '"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", null, "n"]]}'
    )
    spec["projections"][0][1] = expression
    compiled = _cypher(spec)
    assert compiled["statement"] == expected
    assert compiled["parameters"] == {"p0": 2020, "p1": "Berlin"}


def test_pattern_comprehension_carries_path_properties() -> None:
    spec = json.loads(
        '{"spec_version": 1, "matches": [{"spec_version": 1, '
        '"paths": [[["node", "p", ["Person"]]]]}], '
        '"projections": [["expr", ["pattern_comp", ['
        '["node", "p", ["Person"], {"active": ["lit", true]}], '
        '["edge", "outgoing", ["KNOWS"], "r", null, null, '
        '{"since": ["lit", 2020]}], '
        '["node", "f", ["Person"], {"city": ["lit", "Berlin"]}]'
        '], null, ["prop", "f", "name"]], "friends"]]}'
    )
    compiled = _cypher(spec)
    assert compiled["statement"] == (
        "MATCH (p:Person) RETURN [(p:Person {active: $p0})"
        "-[r:KNOWS {since: $p1}]->(f:Person {city: $p2}) | f.name] AS friends"
    )
    assert compiled["parameters"] == {"p0": True, "p1": 2020, "p2": "Berlin"}


# --- Parity with the fluent API ---------------------------------------------


def test_create_with_properties_matches_fluent_api() -> None:
    spec = json.loads(
        '{"spec_version": 1, "mutations": [["create", [['
        '["node", "a", ["Person"], {"name": ["lit", "Alice"]}], '
        '["edge", "outgoing", ["KNOWS"], "r", null, null, '
        '{"since": ["lit", 2020]}], '
        '["node", "b", ["Person"], {"name": ["lit", "Bob"]}]'
        "]]]]}"
    )

    fluent = NativeQueryBuilder()
    fluent.create()
    fluent.node("a", ["Person"], [("name", "Alice")])
    fluent.to(["KNOWS"], "r", [("since", 2020)])
    fluent.node("b", ["Person"], [("name", "Bob")])

    from_spec = compile_query_from_spec(spec, "cypher")
    from_fluent = fluent.compile("cypher")
    assert from_spec["statement"] == from_fluent["statement"]
    assert from_spec["parameters"] == from_fluent["parameters"]


def test_match_with_properties_matches_fluent_api() -> None:
    fluent = NativeQueryBuilder()
    fluent.match()
    fluent.node("a", ["Person"], [("name", "Alice")])
    fluent.to(["KNOWS"], "r", [("since", 2020)])
    fluent.node("b", ["Person"])
    fluent.return_()
    fluent.field("b", "name", "n")

    from_spec = _cypher(MATCH_NODE_EDGE_PROPS_SPEC)
    from_fluent = fluent.compile("cypher")
    assert from_spec["statement"] == from_fluent["statement"]
    assert from_spec["parameters"] == from_fluent["parameters"]
