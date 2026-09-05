#!/usr/bin/env python3
"""
Voyager OGM Conformance Test Fixture Auditor.

Parses SQL:2023 PGQ sqllogictest (.test) files, ISO GQL conformance (.feature)
files, golden multi-dialect AST fixtures (.json), and vendor extensions to
report scenario counts and test fixture status.
"""

import glob
import json
import os
import re


def audit_golden_queries(queries_dir: str):
    json_files = glob.glob(os.path.join(queries_dir, "*.json"))
    cases = []
    for fpath in json_files:
        fname = os.path.basename(fpath)
        with open(fpath, encoding="utf-8") as f:
            data = json.load(f)
        cases.append((fname, data.get("test_name", fname), data.get("description", "")))
    return json_files, cases


def audit_sql_pgq_tck(tck_dir: str):
    test_files = glob.glob(os.path.join(tck_dir, "*.test"))
    queries = []

    for fpath in test_files:
        fname = os.path.basename(fpath)
        with open(fpath, encoding="utf-8") as f:
            lines = f.readlines()

        current_query = None
        for line in lines:
            if line.startswith("query"):
                current_query = line.strip()
            elif line.startswith("SELECT") and current_query:
                queries.append((fname, current_query, line.strip()))
                current_query = None

    return test_files, queries


def audit_iso_gql_tck(tck_dir: str):
    feature_files = glob.glob(os.path.join(tck_dir, "*.feature"))
    scenarios = []

    for fpath in feature_files:
        fname = os.path.basename(fpath)
        with open(fpath, encoding="utf-8") as f:
            content = f.read()

        found = re.findall(r"Scenario:\s*\[(\d+)\]\s*(.*)", content)
        for num, desc in found:
            scenarios.append((fname, num, desc))

    return feature_files, scenarios


def audit_vendor_extensions(vendor_dir: str):
    json_files = glob.glob(os.path.join(vendor_dir, "*.json"))
    cases = []
    for fpath in json_files:
        fname = os.path.basename(fpath)
        with open(fpath, encoding="utf-8") as f:
            data = json.load(f)
        for tc in data.get("test_cases", []):
            cases.append((fname, tc.get("vendor"), tc.get("feature"), tc.get("description")))
    return json_files, cases


def main():
    base_dir = os.path.dirname(os.path.abspath(__file__))
    test_data_dir = os.path.dirname(base_dir)
    queries_dir = os.path.join(test_data_dir, "queries")
    pgq_dir = os.path.join(base_dir, "sql_pgq")
    gql_dir = os.path.join(base_dir, "iso_gql")
    vendor_dir = os.path.join(base_dir, "vendor_extensions")

    print("=" * 70)
    print("Voyager OGM Dialect & Vendor Conformance Fixtures Audit")
    print("=" * 70)

    # 1. Golden Multi-Dialect Queries
    q_files, q_cases = audit_golden_queries(queries_dir)
    print(f"\n[1] Golden Multi-Dialect Queries ({len(q_files)} files, {len(q_cases)} fixtures):")
    for fname, name, desc in q_cases:
        print(f"    - [{fname}] {name}: {desc}")

    # 2. SQL:2023 PGQ & DuckPGQ Tests
    pgq_files, pgq_queries = audit_sql_pgq_tck(pgq_dir)
    print(
        f"\n[2] SQL:2023 PGQ & DuckPGQ Suite ({len(pgq_files)} files, {len(pgq_queries)} queries):"
    )
    for fname, q_type, query_lead in pgq_queries:
        print(f"    - [{fname}] ({q_type}): {query_lead}")

    # 3. ISO GQL Conformance
    gql_files, gql_scenarios = audit_iso_gql_tck(gql_dir)
    print(f"\n[3] ISO GQL Suite ({len(gql_files)} files, {len(gql_scenarios)} scenarios):")
    for fname, num, desc in gql_scenarios:
        print(f"    - [{fname}] #{num}: {desc}")

    # 4. Database Vendor Extensions (Neo4j APOC/Vector, Memgraph MAGE, DuckDB)
    v_files, v_cases = audit_vendor_extensions(vendor_dir)
    print(
        f"\n[4] Database Vendor Extensions ({len(v_files)} files, {len(v_cases)} procedure tests):"
    )
    for fname, vendor, feat, desc in v_cases:
        print(f"    - [{fname}] ({vendor.upper()}) {feat}: {desc}")

    total_fixtures = len(q_cases) + len(pgq_queries) + len(gql_scenarios) + len(v_cases)
    print("\n" + "=" * 70)
    print(f"Total Conformance Fixtures Verified: {total_fixtures}")
    print("=" * 70)


if __name__ == "__main__":
    main()
