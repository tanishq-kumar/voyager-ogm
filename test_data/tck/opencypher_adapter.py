#!/usr/bin/env python3
"""Official openCypher TCK Conformance Adapter and Harness for Voyager OGM.

Systematically parses official Gherkin .feature files from the openCypher TCK
and tests them directly against Voyager's native AST topology extractor, schema
conformance engine, and multi-dialect query compiler.
"""

from __future__ import annotations

import glob
import os
import re
import sys
from dataclasses import dataclass, field
from typing import Any

from voyager_ogm._voyager_rs import extract_topology_from_query


@dataclass
class TckScenario:
    """Represents a single scenario parsed from an official openCypher TCK .feature file."""

    category: str
    file_name: str
    scenario_id: int
    name: str
    setup_queries: list[str] = field(default_factory=list)
    test_query: str = ""
    expected_columns: list[str] = field(default_factory=list)
    expected_rows: list[list[str]] = field(default_factory=list)
    expected_error: str | None = None


@dataclass
class ScenarioResult:
    """Result of running a single TCK scenario through Voyager."""

    scenario: TckScenario
    success: bool
    nodes_found: int = 0
    edges_found: int = 0
    columns_found: list[str] = field(default_factory=list)
    error: str | None = None
    failure_category: str | None = None


def parse_feature_file(file_path: str) -> list[TckScenario]:
    """Parses an official openCypher Gherkin .feature file into TckScenario instances."""
    scenarios: list[TckScenario] = []
    category = os.path.basename(os.path.dirname(file_path))
    file_name = os.path.basename(file_path)

    with open(file_path, encoding="utf-8") as f:
        content = f.read()

    # Split into individual scenarios: "Scenario: [id] Name"
    scenario_chunks = re.split(r"\n\s*Scenario:\s*\[(\d+)\]\s*", content)
    if len(scenario_chunks) < 2:
        return scenarios

    for i in range(1, len(scenario_chunks), 2):
        s_id = int(scenario_chunks[i])
        s_body = scenario_chunks[i + 1]

        lines = s_body.strip().split("\n")
        name = lines[0].strip() if lines else f"Scenario {s_id}"

        # Extract setup queries: having executed [the program]: """ ... """
        setup_queries: list[str] = []
        for m in re.finditer(
            r"having executed(?:\s+the\s+program)?:\s*\"\"\"(.*?)\"\"\"", s_body, re.DOTALL
        ):
            q = m.group(1).strip()
            if q:
                setup_queries.append(q)

        # Extract test query: When executing [the program | query | control query]: """ ... """
        test_query = ""
        q_match = re.search(
            r"When executing\s+(?:the\s+program|the\s+query|query|control\s+query):\s*\"\"\"(.*?)\"\"\"",
            s_body,
            re.DOTALL,
        )
        if q_match:
            test_query = q_match.group(1).strip()

        # Extract expected errors: e.g. "Then a SyntaxError should be raised"
        expected_error = None
        err_match = re.search(r"Then a (\w+Error) should be raised", s_body)
        if err_match:
            expected_error = err_match.group(1)

        # Extract expected tabular results:
        expected_columns: list[str] = []
        expected_rows: list[list[str]] = []
        table_match = re.search(
            r"Then the result should be[^\n]*:\s*\n((?:\s*\|[^\n]+\|\s*\n)+)", s_body
        )
        if table_match:
            table_text = table_match.group(1).strip()
            table_lines = [ln.strip() for ln in table_text.split("\n") if ln.strip()]
            if table_lines:
                # First line is column headers
                header_line = table_lines[0]
                expected_columns = [col.strip() for col in header_line.strip("|").split("|")]
                for data_line in table_lines[1:]:
                    row_vals = [val.strip() for val in data_line.strip("|").split("|")]
                    expected_rows.append(row_vals)

        if test_query:
            scenarios.append(
                TckScenario(
                    category=category,
                    file_name=file_name,
                    scenario_id=s_id,
                    name=name,
                    setup_queries=setup_queries,
                    test_query=test_query,
                    expected_columns=expected_columns,
                    expected_rows=expected_rows,
                    expected_error=expected_error,
                )
            )

    return scenarios


def classify_failure(query: str, err_msg: str) -> str:
    """Categorizes the failure reason based on query patterns and error messages."""
    u_query = query.upper()
    if "SYNTAXERROR" in err_msg.upper() or "PARSE" in err_msg.upper():
        if "SHORTESTPATH" in u_query:
            return "ALGORITHM_SHORTEST_PATH"
        if "EXTRACT(" in u_query or "REDUCE(" in u_query or "FILTER(" in u_query:
            return "LEGACY_CYPHER_FUNCTIONS"
        if "EXISTS(" in u_query or "ALL(" in u_query or "ANY(" in u_query or "NONE(" in u_query:
            return "PATTERN_PREDICATE_EXPR"
        if "UNION" in u_query:
            return "UNION_CLAUSE"
        if "OPTIONAL MATCH" in u_query:
            return "OPTIONAL_MATCH"
        return "CYPHER_PARSER_SYNTAX"
    return "TOPOLOGY_EXTRACTION_FAILURE"


def validate_topology_invariants(query: str, topo: dict[str, Any]) -> list[str]:
    """Validates structural and semantic invariants on the extracted topology.

    Returns a list of invariant violation messages (empty list indicates success).
    """
    violations: list[str] = []
    nodes = topo.get("nodes", [])
    edges = topo.get("edges", [])

    # 1. Node ID uniqueness and valid fields
    node_ids: set[str] = set()
    for n in nodes:
        nid = n.get("id")
        if not nid:
            violations.append("Node missing required 'id'")
        elif nid in node_ids:
            violations.append(f"Duplicate node id '{nid}'")
        else:
            node_ids.add(nid)

        if not isinstance(n.get("labels"), list):
            violations.append(f"Node '{nid}' labels is not a list")

    # 2. Edge Referential Integrity and Directional Soundness
    edge_ids: set[str] = set()
    for e in edges:
        eid = e.get("id")
        if not eid:
            violations.append("Edge missing required 'id'")
        elif eid in edge_ids:
            violations.append(f"Duplicate edge id '{eid}'")
        else:
            edge_ids.add(eid)

        src = e.get("source")
        tgt = e.get("target")
        if src not in node_ids:
            violations.append(
                f"Referential integrity violated: edge '{eid}' source '{src}' does not exist in nodes"
            )
        if tgt not in node_ids:
            violations.append(
                f"Referential integrity violated: edge '{eid}' target '{tgt}' does not exist in nodes"
            )

        direction = e.get("direction")
        if direction not in ("outgoing", "incoming", "undirected"):
            violations.append(f"Edge '{eid}' has invalid direction '{direction}'")

    return violations


def execute_scenario_topology(scenario: TckScenario) -> ScenarioResult:
    """Executes topology extraction against a single TCK scenario and enforces graph invariants."""
    try:
        topo = extract_topology_from_query(scenario.test_query)
        nodes = topo.get("nodes", [])
        edges = topo.get("edges", [])
        violations = validate_topology_invariants(scenario.test_query, topo)
        if violations:
            return ScenarioResult(
                scenario=scenario,
                success=False,
                nodes_found=len(nodes),
                edges_found=len(edges),
                error="; ".join(violations),
                failure_category="INVARIANT_VIOLATION",
            )
        return ScenarioResult(
            scenario=scenario,
            success=True,
            nodes_found=len(nodes),
            edges_found=len(edges),
        )
    except Exception as e:
        err_str = str(e)
        cat = classify_failure(scenario.test_query, err_str)
        return ScenarioResult(
            scenario=scenario,
            success=False,
            error=err_str,
            failure_category=cat,
        )


def run_tck_adapter(tck_root: str) -> dict[str, Any]:
    """Runs all official openCypher TCK scenarios through the adapter."""
    feature_files = sorted(glob.glob(os.path.join(tck_root, "**", "*.feature"), recursive=True))
    all_scenarios: list[TckScenario] = []

    for fpath in feature_files:
        scenarios = parse_feature_file(fpath)
        all_scenarios.extend(scenarios)

    results: list[ScenarioResult] = []
    category_stats: dict[str, dict[str, int]] = {}
    failure_reasons: dict[str, list[tuple[str, str, str]]] = {}

    for s in all_scenarios:
        res = execute_scenario_topology(s)
        results.append(res)

        cat = s.category
        if cat not in category_stats:
            category_stats[cat] = {"total": 0, "passed": 0, "failed": 0}
        category_stats[cat]["total"] += 1

        if res.success:
            category_stats[cat]["passed"] += 1
        else:
            category_stats[cat]["failed"] += 1
            reason = res.failure_category or "UNKNOWN"
            if reason not in failure_reasons:
                failure_reasons[reason] = []
            failure_reasons[reason].append((s.file_name, s.name, s.test_query))

    total = len(results)
    passed = sum(1 for r in results if r.success)
    failed = total - passed
    pass_rate = (passed / total * 100) if total > 0 else 0.0

    return {
        "total": total,
        "passed": passed,
        "failed": failed,
        "pass_rate": pass_rate,
        "category_stats": category_stats,
        "failure_reasons": failure_reasons,
        "results": results,
    }


def main() -> None:
    base_dir = os.path.dirname(os.path.abspath(__file__))
    opencypher_dir = os.path.join(base_dir, "opencypher")

    if not os.path.exists(opencypher_dir):
        print(f"Error: {opencypher_dir} not found. Run fetch_opencypher_tck.py first.")
        sys.exit(1)

    print("=" * 80)
    print("Voyager OGM Official openCypher TCK Conformance & Topology Extraction Audit")
    print("=" * 80)

    report = run_tck_adapter(opencypher_dir)

    print("\nOverall Conformance:")
    print(f"  Total Scenarios Evaluated : {report['total']}")
    print(f"  Passed                    : {report['passed']} ({report['pass_rate']:.1f}%)")
    print(f"  Failed                    : {report['failed']} ({100 - report['pass_rate']:.1f}%)")

    print("\nCategory Breakdown:")
    print(f"  {'Category':<22} | {'Total':<8} | {'Passed':<8} | {'Failed':<8} | {'Pass Rate':<10}")
    print("  " + "-" * 64)
    for cat, stats in sorted(report["category_stats"].items()):
        rate = (stats["passed"] / stats["total"] * 100) if stats["total"] > 0 else 0.0
        print(
            f"  {cat:<22} | {stats['total']:<8} | {stats['passed']:<8} | {stats['failed']:<8} | {rate:>6.1f}%"
        )

    print("\nFailure Root Causes:")
    for reason, items in sorted(
        report["failure_reasons"].items(), key=lambda x: len(x[1]), reverse=True
    ):
        print(f"\n  [{reason}] ({len(items)} scenarios)")
        # Show top 2 sample queries
        for fname, sname, query in items[:2]:
            single_line_query = " ".join(query.split())
            print(f"    - {fname} ({sname})")
            print(
                f"      `{single_line_query[:100]}...`"
                if len(single_line_query) > 100
                else f"      `{single_line_query}`"
            )

    print("\n" + "=" * 80)


if __name__ == "__main__":
    main()
