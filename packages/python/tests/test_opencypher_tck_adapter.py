"""Tests for the official openCypher TCK Conformance Adapter and Topology Validation."""

import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from test_data.tck.opencypher_adapter import (  # noqa: E402
    TckScenario,
    execute_scenario_topology,
    parse_feature_file,
    run_tck_adapter,
)

TCK_DIR = REPO_ROOT / "test_data" / "tck" / "opencypher"


def test_tck_feature_files_present():
    """Verifies that all official openCypher TCK feature files exist on disk."""
    assert TCK_DIR.exists(), f"TCK directory {TCK_DIR} not found"
    feature_files = list(TCK_DIR.glob("**/*.feature"))
    assert len(feature_files) >= 70, (
        f"Expected at least 70 feature files, found {len(feature_files)}"
    )


def test_tck_parser_extracts_scenarios():
    """Verifies that the Gherkin parser correctly parses Match1.feature."""
    match1 = TCK_DIR / "clauses" / "match" / "Match1.feature"
    assert match1.exists()
    scenarios = parse_feature_file(str(match1))
    assert len(scenarios) >= 5
    s1 = scenarios[0]
    assert s1.scenario_id == 1
    assert "MATCH (n)" in s1.test_query
    assert "RETURN n" in s1.test_query
    assert s1.expected_columns == ["n"]


def test_tck_topology_extraction_across_all_clauses():
    """Verifies topology extraction against all 632 official openCypher TCK scenarios."""
    report = run_tck_adapter(str(TCK_DIR))
    assert report["total"] >= 600
    assert report["passed"] == report["total"], f"Failed scenarios: {report['failed']}"
    assert report["pass_rate"] == 100.0


@pytest.mark.parametrize(
    "clause_dir",
    [
        "match",
        "match-where",
        "create",
        "merge",
        "delete",
        "return",
        "set",
        "remove",
        "return-orderby",
        "return-skip-limit",
        "unwind",
        "with",
    ],
)
def test_tck_category_extraction_completeness(clause_dir: str):
    """Verifies each clause category achieves 100% topology extraction."""
    cat_path = TCK_DIR / "clauses" / clause_dir
    assert cat_path.exists()
    feature_files = list(cat_path.glob("*.feature"))
    assert len(feature_files) > 0

    scenarios: list[TckScenario] = []
    for f in feature_files:
        scenarios.extend(parse_feature_file(str(f)))

    assert len(scenarios) > 0
    for s in scenarios:
        res = execute_scenario_topology(s)
        assert res.success, (
            f"Failed topology on {s.file_name} [{s.scenario_id}] {s.name}: {res.error}"
        )


def test_tck_topology_nodes_and_edges_integrity():
    """Verifies that non-empty pattern queries extract valid non-empty node and edge structures."""
    match2 = TCK_DIR / "clauses" / "match" / "Match2.feature"
    scenarios = parse_feature_file(str(match2))

    # Scenario [2] matches (:A)-[r]->(:B)
    s2 = next(s for s in scenarios if s.scenario_id == 2)
    res2 = execute_scenario_topology(s2)
    assert res2.success
    assert res2.edges_found >= 1
    assert res2.nodes_found >= 2

    # Scenario [4] matches ()-[r]->()
    s4 = next(s for s in scenarios if s.scenario_id == 4)
    res4 = execute_scenario_topology(s4)
    assert res4.success
    assert res4.edges_found >= 1
    assert res4.nodes_found >= 2


def _check_port(host: str, port: int) -> bool:
    import socket

    try:
        with socket.create_connection((host, port), timeout=0.3):
            return True
    except OSError:
        return False


def test_tck_tier3_live_neo4j_execution():
    """Tier 3: Executes official openCypher Match1 scenarios on live Neo4j Enterprise (port 7687)."""
    if not _check_port("127.0.0.1", 7687):
        pytest.skip("Neo4j Enterprise not available on port 7687")

    from neo4j import GraphDatabase

    driver = GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "voyex1234"))
    try:
        driver.verify_connectivity()
    except Exception as e:
        pytest.skip(f"Neo4j connectivity failed: {e}")

    match1 = TCK_DIR / "clauses" / "match" / "Match1.feature"
    scenarios = parse_feature_file(str(match1))

    with driver.session() as session:
        for sc in scenarios:
            if sc.expected_error:
                continue

            session.run("MATCH (n) DETACH DELETE n")
            for q in sc.setup_queries:
                session.run(q)

            res = session.run(sc.test_query)
            keys = list(res.keys())
            if sc.expected_columns:
                assert keys == sc.expected_columns

            records = list(res)
            if sc.expected_rows:
                assert len(records) == len(sc.expected_rows)

            # Invariant check on extracted topology
            topo_res = execute_scenario_topology(sc)
            assert topo_res.success

        session.run("MATCH (n) DETACH DELETE n")
    driver.close()


def test_tck_tier3_live_falkordb_execution():
    """Tier 3: Executes official openCypher Match1 scenarios on live FalkorDB (port 6379)."""
    if not _check_port("127.0.0.1", 6379):
        pytest.skip("FalkorDB container not available on port 6379")

    try:
        from falkordb import FalkorDB

        db = FalkorDB(host="127.0.0.1", port=6379)
        g = db.select_graph("voyager_tck_live_test")
        g.query("RETURN 1")
    except Exception as e:
        pytest.skip(f"FalkorDB connectivity failed: {e}")

    match1 = TCK_DIR / "clauses" / "match" / "Match1.feature"
    scenarios = parse_feature_file(str(match1))

    for sc in scenarios:
        if sc.expected_error:
            continue

        g.query("MATCH (n) DETACH DELETE n")
        for q in sc.setup_queries:
            g.query(q)

        res = g.query(sc.test_query)
        if sc.expected_rows:
            assert len(res.result_set) == len(sc.expected_rows)

        topo_res = execute_scenario_topology(sc)
        assert topo_res.success

    g.query("MATCH (n) DETACH DELETE n")
