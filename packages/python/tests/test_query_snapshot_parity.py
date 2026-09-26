"""Parity Snapshot Verification Harness for Voyager OGM Multi-Dialect Compilers.

Verifies RFC-0004 Single Source of Truth invariants:
Guarantees 100% byte-for-byte deterministic statement and parameter parity
across supported dialects (Cypher, ISO GQL, and SQL:2023 PGQ) against golden baselines.
"""

from __future__ import annotations

import json
import os
import warnings
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    fn,
    node,
    relationship,
    reset_alias_counters,
)

SNAPSHOT_FILE = Path(__file__).parent / "snapshots" / "query_parity_snapshots.json"
SUPPORTED_DIALECTS: tuple[str, ...] = ("cypher", "iso_gql", "sql_pgq")


# ---------------------------------------------------------------------------
# Domain Models for Snapshot Testing
# ---------------------------------------------------------------------------


@node(label="Person")
class Person(Node):
    name = Field()
    age = Field()
    city = Field()
    skills = Field()


@node(label="Company")
class Company(Node):
    name = Field()


@relationship(type_name="KNOWS")
class Knows(Relationship):
    since = Field()


@relationship(type_name="WORKS_AT")
class WorksAt(Relationship):
    role = Field()


@pytest.fixture(autouse=True)
def reset_aliases():
    reset_alias_counters()


# ---------------------------------------------------------------------------
# Comprehensive Query Corpus (43 Distinct Scenarios)
# ---------------------------------------------------------------------------

QUERY_CORPUS: dict[str, Callable[[], Query]] = {
    # 1. Node Patterns & Label Expressions
    "single_node_match": lambda: Query.match(Person("p")).return_(Person("p").name),
    "single_node_multi_label": lambda: Query.match_node("p", labels=["Person", "Employee"]).return_(
        "p.name"
    ),
    "label_expression_disjunction": lambda: Query.match_node(
        "n", labels="Person | Company"
    ).return_("n.name"),
    "label_expression_negation": lambda: Query.match_node("n", labels="!Inactive").return_(
        "n.name"
    ),
    "label_expression_conjunction": lambda: Query.match_node(
        "n", labels=["Person", "Developer"]
    ).return_("n.name"),
    # 2. Path Traversals & Repetitions
    "directed_outgoing_traversal": lambda: (
        Query.match(Person("a"))
        .to(Knows, "r")
        .node(Person("b"))
        .return_(Person("a").name, Person("b").name)
    ),
    "directed_incoming_traversal": lambda: (
        Query.match(Person("a"))
        .from_(WorksAt, "w")
        .node(Company("c"))
        .return_(Person("a").name, Company("c").name)
    ),
    "undirected_traversal": lambda: (
        Query.match(Person("a"))
        .edge(Knows, "r")
        .node(Person("b"))
        .return_(Person("a").name, Person("b").name)
    ),
    "anonymous_edge_traversal": lambda: (
        Query.match(Person("a")).to().node(Person("b")).return_(Person("a").name, Person("b").name)
    ),
    "quantified_hops_range": lambda: (
        Query.match(Person("a"))
        .to(Knows, "r")
        .hops(1, 3)
        .node(Person("b"))
        .return_(Person("a").name, Person("b").name)
    ),
    "quantified_hops_exact": lambda: (
        Query.match(Person("a"))
        .to(Knows, "r")
        .hops(2, 2)
        .node(Person("b"))
        .return_(Person("a").name, Person("b").name)
    ),
    "multi_pattern_branching": lambda: (
        Query.match(Person("a"))
        .to(Knows, "r1")
        .node(Person("b"))
        .pattern()
        .match(Person("b"))
        .to(WorksAt, "r2")
        .node(Company("c"))
        .return_(Person("a").name, Person("b").name, Company("c").name)
    ),
    # 3. Predicates & Expressions
    "relational_predicates_chained": lambda: (
        Query.match(Person("p"))
        .where(Person("p").age > 21, Person("p").age <= 65)
        .return_(Person("p").name, Person("p").age)
    ),
    "equality_inequality_predicates": lambda: (
        Query.match(Person("p"))
        .where(Person("p").city == "London", Person("p").name != "Bob")
        .return_(Person("p").name)
    ),
    "boolean_and_or_expressions": lambda: (
        Query.match(Person("p"))
        .where((Person("p").age > 18) & (Person("p").age < 65))
        .return_(Person("p").name)
    ),
    "in_and_not_in_predicates": lambda: (
        Query.match(Person("p"))
        .where(
            Person("p").city.in_(["London", "Paris", "Berlin"]),
            Person("p").name.not_in(["Mallory"]),
        )
        .return_(Person("p").name)
    ),
    "string_predicates_starts_contains_ends": lambda: (
        Query.match(Person("p"))
        .where(
            Person("p").name.startswith("Al"),
            Person("p").city.contains("York"),
            Person("p").name.endswith("ce"),
        )
        .return_(Person("p").name)
    ),
    "null_and_not_null_predicates": lambda: (
        Query.match(Person("p"))
        .where(Person("p").skills.is_null(), Person("p").city.is_not_null())
        .return_(Person("p").name)
    ),
    "where_not_predicate": lambda: (
        Query.match(Person("p")).where_not(Person("p").age < 18).return_(Person("p").name)
    ),
    "math_arithmetic_expressions": lambda: Query.match(Person("p")).return_(
        doubled=Person("p").age * 2,
        offset=Person("p").age + 10,
        ratio=Person("p").age / 5,
    ),
    "string_concatenation_and_functions": lambda: Query.match(Person("p")).return_(
        greeting=Person("p").name + " lives in " + Person("p").city,
        upper_name=Person("p").name.upper(),
        lower_city=Person("p").city.lower(),
    ),
    # 4. Projections, Aggregations & Pagination
    "aggregate_functions": lambda: (
        Query.match(Person("p"))
        .to(WorksAt, "w")
        .node(Company("c"))
        .return_(
            company=Company("c").name,
            total=fn.count(Person("p")),
            avg_age=fn.avg(Person("p").age),
            min_age=fn.min(Person("p").age),
            max_age=fn.max(Person("p").age),
        )
    ),
    "distinct_projections": lambda: Query.match(Person("p")).distinct().return_(Person("p").city),
    "order_by_multi_and_pagination": lambda: (
        Query.match(Person("p"))
        .return_(Person("p").name, Person("p").age)
        .order_by(Person("p").age, ascending=False)
        .order_by(Person("p").name, ascending=True)
        .skip(10)
        .limit(25)
    ),
    "with_pipeline_and_filtering": lambda: (
        Query.match(Person("p"))
        .where(Person("p").age > 21)
        .with_(Person("p").name, Person("p").age)
        .where(Person("p").age < 50)
        .return_(Person("p").name)
        .limit(10)
    ),
    "with_distinct_pipeline": lambda: (
        Query.match(Person("p")).with_(Person("p").city, distinct=True).return_(Person("p").city)
    ),
    # 5. Path Search Modes & Path Variables
    "path_mode_trail": lambda: (
        Query.match().trail("p").node(Person("a")).to(Knows, "k").node(Person("b")).return_("p")
    ),
    "path_mode_simple": lambda: (
        Query.match().simple("p").node(Person("a")).to(Knows, "k").node(Person("b")).return_("p")
    ),
    "path_mode_acyclic": lambda: (
        Query.match().acyclic("p").node(Person("a")).to(Knows, "k").node(Person("b")).return_("p")
    ),
    "path_mode_walk": lambda: (
        Query.match().walk("p").node(Person("a")).to(Knows, "k").node(Person("b")).return_("p")
    ),
    "path_variable_binding": lambda: (
        Query.match()
        .path_variable("p")
        .node(Person("a"))
        .to(Knows, "k")
        .node(Person("b"))
        .return_("p")
    ),
    # 6. Linear Statements (GQL LET / FILTER)
    "linear_let_and_linear_filter": lambda: (
        Query.match(Person("p"))
        .let_("full_name", Person("p").name + " Senior")
        .linear_filter(Person("p").age >= 60)
        .return_("full_name")
    ),
    # 7. Graph Mutations (DML)
    "mutation_create": lambda: Query.create(Person("p")).return_(Person("p").name),
    "mutation_merge_with_on_create_on_match": lambda: (
        Query.merge(Person("p"))
        .on_create_set(Person("p").name == "Alice")
        .on_match_set(Person("p").city == "Paris")
        .return_(Person("p").name)
    ),
    "mutation_set_and_remove": lambda: (
        Query.match(Person("p"))
        .where(Person("p").name == "Alice")
        .set(Person("p").age == 30)
        .remove(Person("p").city)
        .return_(Person("p").name)
    ),
    "mutation_delete": lambda: (
        Query.match(Person("p")).where(Person("p").age < 18).delete(Person("p"))
    ),
    "mutation_detach_delete": lambda: (
        Query.match(Person("p")).where(Person("p").age < 18).detach_delete(Person("p"))
    ),
    # 8. Ingestion & Ingest Plans
    "unwind_batch": lambda: (
        Query.unwind("$batch", "item").match(Person("p")).return_(Person("p").name)
    ),
    "load_csv": lambda: (
        Query.load_csv("file:///data.csv", with_headers=True, alias="row")
        .match(Person("p"))
        .return_(Person("p").name)
    ),
    # 9. Optimizer & Execution Modes
    "optimizer_predicate_pushdown": lambda: (
        Query.match(Person("p"))
        .where(Person("p").city == "London")
        .return_(Person("p").name)
        .optimize()
    ),
    "execution_mode_explain": lambda: Query.match(Person("p")).return_(Person("p").name).explain(),
    "execution_mode_profile": lambda: Query.match(Person("p")).return_(Person("p").name).profile(),
    "execution_mode_explain_and_profile": lambda: (
        Query.match(Person("p")).return_(Person("p").name).explain().profile()
    ),
}


def generate_snapshots() -> None:
    """Generates canonical golden snapshot baselines directly from QUERY_CORPUS."""
    snapshots: dict[str, dict[str, Any]] = {}
    for scenario_name, builder_fn in QUERY_CORPUS.items():
        snapshots[scenario_name] = {}
        for dialect in SUPPORTED_DIALECTS:
            reset_alias_counters()
            query = builder_fn()
            kwargs = {"graph_name": "g"} if dialect == "sql_pgq" else {}
            try:
                with warnings.catch_warnings():
                    warnings.simplefilter("ignore")
                    compiled = query.compile(dialect=dialect, **kwargs)
                snapshots[scenario_name][dialect] = {
                    "status": "ok",
                    "statement": compiled.statement,
                    "parameters": compiled.parameters,
                    "execution_mode": compiled.execution_mode,
                }
            except Exception as e:
                snapshots[scenario_name][dialect] = {
                    "status": "unsupported",
                    "error_type": type(e).__name__,
                    "error_message": str(e),
                }

    SNAPSHOT_FILE.parent.mkdir(parents=True, exist_ok=True)
    with open(SNAPSHOT_FILE, "w", encoding="utf-8") as f:
        json.dump(snapshots, f, indent=2, sort_keys=True)


def load_snapshots() -> dict[str, dict[str, Any]]:
    """Loads recorded snapshot baselines from JSON file."""
    if os.environ.get("UPDATE_SNAPSHOTS") == "1" or not SNAPSHOT_FILE.exists():
        generate_snapshots()

    assert SNAPSHOT_FILE.exists(), f"Snapshot file {SNAPSHOT_FILE} does not exist!"
    with open(SNAPSHOT_FILE, encoding="utf-8") as f:
        return json.load(f)


@pytest.fixture(scope="session")
def snapshots() -> dict[str, dict[str, Any]]:
    """Session-scoped fixture providing the loaded snapshot map."""
    return load_snapshots()


def test_snapshot_corpus_completeness(snapshots: dict[str, dict[str, Any]]):
    """Verifies that all corpus scenarios exist in the snapshot map with all supported dialects."""
    assert set(snapshots.keys()) == set(QUERY_CORPUS.keys()), (
        f"Snapshot keys mismatch! Missing in snapshots: {set(QUERY_CORPUS.keys()) - set(snapshots.keys())}, "
        f"Extra in snapshots: {set(snapshots.keys()) - set(QUERY_CORPUS.keys())}"
    )

    for scenario_name, dialect_map in snapshots.items():
        assert set(dialect_map.keys()) == set(SUPPORTED_DIALECTS), (
            f"Scenario '{scenario_name}' missing dialects in snapshot file: "
            f"expected {SUPPORTED_DIALECTS}, got {set(dialect_map.keys())}"
        )


@pytest.mark.parametrize("scenario_name", sorted(QUERY_CORPUS.keys()))
@pytest.mark.parametrize("dialect", SUPPORTED_DIALECTS)
def test_query_snapshot_parity(
    scenario_name: str,
    dialect: str,
    snapshots: dict[str, dict[str, Any]],
):
    """Verifies byte-for-byte query parity against snapshot baselines.

    Asserts statement string, parameter dictionary, and execution mode
    are 100% byte-identical to golden snapshots.
    """
    expected = snapshots[scenario_name][dialect]
    builder_fn = QUERY_CORPUS[scenario_name]
    query = builder_fn()

    kwargs = {"graph_name": "g"} if dialect == "sql_pgq" else {}

    if expected["status"] == "ok":
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            compiled = query.compile(dialect=dialect, **kwargs)

        assert compiled.statement == expected["statement"], (
            f"Parity mismatch in scenario '{scenario_name}' [{dialect}]!\n"
            f"Expected: {expected['statement']}\n"
            f"Actual:   {compiled.statement}"
        )
        assert compiled.parameters == expected["parameters"], (
            f"Parameter mismatch in scenario '{scenario_name}' [{dialect}]!\n"
            f"Expected: {expected['parameters']}\n"
            f"Actual:   {compiled.parameters}"
        )
        assert compiled.execution_mode == expected["execution_mode"], (
            f"Execution mode mismatch in scenario '{scenario_name}' [{dialect}]!\n"
            f"Expected: {expected['execution_mode']}\n"
            f"Actual:   {compiled.execution_mode}"
        )
    elif expected["status"] == "unsupported":
        with pytest.raises(ValueError) as exc_info:
            query.compile(dialect=dialect, **kwargs)
        actual_err = str(exc_info.value)
        assert expected["error_message"] in actual_err or "does not support" in actual_err, (
            f"Unexpected error in scenario '{scenario_name}' [{dialect}]:\n"
            f"Expected substring: {expected['error_message']}\n"
            f"Actual error:       {actual_err}"
        )


def test_deterministic_byte_reproducibility():
    """Compiles the entire query corpus 5 times in a loop to ensure 100% byte determinism."""
    snapshots = load_snapshots()

    for _ in range(5):
        reset_alias_counters()
        for scenario_name, builder_fn in QUERY_CORPUS.items():
            for dialect in SUPPORTED_DIALECTS:
                expected = snapshots[scenario_name][dialect]
                if expected["status"] != "ok":
                    continue
                kwargs = {"graph_name": "g"} if dialect == "sql_pgq" else {}
                with warnings.catch_warnings():
                    warnings.simplefilter("ignore")
                    compiled = builder_fn().compile(dialect=dialect, **kwargs)
                assert compiled.statement == expected["statement"]
                assert compiled.parameters == expected["parameters"]


def test_cloned_query_parity_identical_to_original():
    """Verifies that Query.clone() produces 100% byte-identical compiled output to the original."""
    snapshots = load_snapshots()

    for scenario_name, builder_fn in QUERY_CORPUS.items():
        original = builder_fn()
        cloned = original.clone()

        for dialect in SUPPORTED_DIALECTS:
            expected = snapshots[scenario_name][dialect]
            if expected["status"] != "ok":
                continue
            kwargs = {"graph_name": "g"} if dialect == "sql_pgq" else {}
            with warnings.catch_warnings():
                warnings.simplefilter("ignore")
                orig_compiled = original.compile(dialect=dialect, **kwargs)
                clone_compiled = cloned.compile(dialect=dialect, **kwargs)

            assert orig_compiled.statement == clone_compiled.statement
            assert orig_compiled.parameters == clone_compiled.parameters
            assert orig_compiled.execution_mode == clone_compiled.execution_mode
