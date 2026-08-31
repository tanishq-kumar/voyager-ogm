#!/usr/bin/env python3
"""
Voyager OGM TCK Test Suite Auditor & Parser.

Parses official openCypher TCK Gherkin (.feature) files, SQL:2023 PGQ
sqllogictest (.test) files, and ISO GQL conformance files to report
scenario counts, clauses tested, and dialect coverage.
"""

import glob
import os
import re


def audit_opencypher_tck(tck_dir: str):
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
    import json

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
    oc_dir = os.path.join(base_dir, "opencypher")
    pgq_dir = os.path.join(base_dir, "sql_pgq")
    gql_dir = os.path.join(base_dir, "iso_gql")
    vendor_dir = os.path.join(base_dir, "vendor_extensions")

    print("=" * 70)
    print("Voyager OGM Official Dialect & Vendor TCK Verification Suite")
    print("=" * 70)

    # 1. openCypher TCK
    oc_files, oc_scenarios = audit_opencypher_tck(oc_dir)
    print(f"\n[1] openCypher TCK Suite ({len(oc_files)} files, {len(oc_scenarios)} scenarios):")
    for fname, num, desc in oc_scenarios:
        print(f"    - [{fname}] #{num}: {desc}")

    # 2. SQL:2023 PGQ Tests
    pgq_files, pgq_queries = audit_sql_pgq_tck(pgq_dir)
    print(f"\n[2] SQL:2023 PGQ Suite ({len(pgq_files)} files, {len(pgq_queries)} queries):")
    for fname, q_type, query_lead in pgq_queries:
        print(f"    - [{fname}] ({q_type}): {query_lead}")

    # 3. ISO GQL Conformance
    gql_files, gql_scenarios = audit_iso_gql_tck(gql_dir)
    print(f"\n[3] ISO GQL Suite ({len(gql_files)} files, {len(gql_scenarios)} scenarios):")
    for fname, num, desc in gql_scenarios:
        print(f"    - [{fname}] #{num}: {desc}")

    # 4. Database-Specific Vendor Extensions (Neo4j APOC/Vector, Memgraph MAGE, DuckDB)
    v_files, v_cases = audit_vendor_extensions(vendor_dir)
    print(
        f"\n[4] Database Vendor Extensions ({len(v_files)} files, {len(v_cases)} procedure tests):"
    )
    for fname, vendor, feat, desc in v_cases:
        print(f"    - [{fname}] ({vendor.upper()}) {feat}: {desc}")

    total_scenarios = len(oc_scenarios) + len(pgq_queries) + len(gql_scenarios) + len(v_cases)
    print("\n" + "=" * 70)
    print(f"Total Dialect & Vendor Conformance Scenarios Ready: {total_scenarios}")
    print("=" * 70)


if __name__ == "__main__":
    main()
