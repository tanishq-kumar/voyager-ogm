"""Live Database Profiling: Unoptimized vs Optimized Constant Folding against Neo4j.

Executes real PROFILE queries against a live Neo4j instance started via compose:
1. Connects to Neo4j via Bolt protocol (bolt://127.0.0.1:7687).
2. Seeds 10,000 benchmark nodes and indexed relationships.
3. Profiles execution plans (dbHits, operator trees, server execution time) for:
   - Scenario 1: Constant folding in WHERE clause on indexed property.
   - Scenario 2: High-volume formula evaluation across graph edges.
   - Scenario 3: Execution latency benchmark over repeated runs.
"""

from __future__ import annotations

import sys
import time
from typing import Any

from neo4j import GraphDatabase

NEO4J_URI = "bolt://127.0.0.1:7687"
NEO4J_AUTH = ("neo4j", "voyagerpass123")


def wait_for_neo4j(timeout_sec: int = 90) -> bool:
    print(f"Waiting for Neo4j at {NEO4J_URI} to be ready...", end="", flush=True)
    start = time.time()
    while time.time() - start < timeout_sec:
        try:
            with GraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH) as driver:
                driver.verify_connectivity()
                print(" Connected!")
                return True
        except Exception:
            print(".", end="", flush=True)
            time.sleep(2)
    print("\nTimeout waiting for Neo4j!")
    return False


def setup_benchmark_dataset(driver):
    print("\n[Setup] Initializing benchmark dataset in Neo4j...")
    with driver.session() as session:
        # Create index on age
        session.run("CREATE INDEX bench_user_age IF NOT EXISTS FOR (u:BenchUser) ON (u.age);")

        # Check if already populated
        res = session.run("MATCH (u:BenchUser) RETURN count(u) AS count").single()
        if res and res["count"] >= 10000:
            print(
                f"  Benchmark data already present ({res['count']} BenchUser nodes). Skipping seeding."
            )
            return

        print("  Wiping prior benchmark data...")
        session.run("MATCH (u:BenchUser) DETACH DELETE u;")
        session.run("MATCH (e:BenchEvent) DETACH DELETE e;")

        print("  Generating 10,000 users and 10,000 events with relationships in batches...")
        batch_cypher = """
        UNWIND range(1, 10000) AS id
        CREATE (u:BenchUser {
            user_id: 'user_' + toString(id),
            age: (id % 50) + 1,
            base_quota: (id * 13) % 200000
        })
        CREATE (e:BenchEvent {
            event_id: 'event_' + toString(id),
            duration_sec: ((id * 37) % 1000000) + 500000
        })
        CREATE (u)-[:PERFORMED]->(e)
        """
        session.run(batch_cypher)
        count = session.run("MATCH (u:BenchUser) RETURN count(u) AS count").single()["count"]
        print(f"  Successfully seeded {count} User-Event graph pairs.")


def print_plan_tree(plan_or_profile: Any, indent: int = 0):
    """Recursively formats and prints the Cypher execution plan tree."""
    prefix = "  " * indent + "+-- " if indent > 0 else ""
    op = plan_or_profile.get("operatorType", "Unknown")
    db_hits = plan_or_profile.get("dbHits", 0)
    rows = plan_or_profile.get("rows", 0)
    args = plan_or_profile.get("arguments", {})
    details = f"dbHits: {db_hits}, rows: {rows}"
    if "Details" in args:
        details += f" | {args['Details']}"
    print(f"{prefix}{op} ({details})")
    for child in plan_or_profile.get("children", []):
        print_plan_tree(child, indent + 1)


def run_profile_scenario(
    driver,
    scenario_name: str,
    unopt_cypher: str,
    opt_cypher: str,
    iterations: int = 50,
):
    print("\n" + "=" * 80)
    print(f"PROFILING: {scenario_name}")
    print("=" * 80)

    print(f"\n[Unoptimized Cypher] ({len(unopt_cypher.encode('utf-8'))} bytes)")
    print(f"  {unopt_cypher.strip()}")
    print(f"\n[Optimized Cypher (Folded Constant)] ({len(opt_cypher.encode('utf-8'))} bytes)")
    print(f"  {opt_cypher.strip()}")

    with driver.session() as session:
        # Profile unoptimized
        res_unopt = session.run(f"PROFILE {unopt_cypher}")
        summary_unopt = res_unopt.consume()
        profile_unopt = summary_unopt.profile

        # Profile optimized
        res_opt = session.run(f"PROFILE {opt_cypher}")
        summary_opt = res_opt.consume()
        profile_opt = summary_opt.profile

        print("\n--- Neo4j Execution Plan: Unoptimized ---")
        if (
            profile_unopt
            and "args" in profile_unopt
            and "string-representation" in profile_unopt["args"]
        ):
            print(profile_unopt["args"]["string-representation"].strip())
        elif profile_unopt:
            print_plan_tree(profile_unopt)

        print("\n--- Neo4j Execution Plan: Optimized ---")
        if profile_opt and "args" in profile_opt and "string-representation" in profile_opt["args"]:
            print(profile_opt["args"]["string-representation"].strip())
        elif profile_opt:
            print_plan_tree(profile_opt)

        # Latency benchmark with interleaved runs to prevent JVM bias
        print(f"\n--- Latency Benchmark ({iterations} interleaved executions) ---")
        for _ in range(10):
            session.run(unopt_cypher).consume()
            session.run(opt_cypher).consume()

        t_unopt_total = 0.0
        t_opt_total = 0.0
        for _ in range(iterations):
            t0 = time.perf_counter_ns()
            session.run(unopt_cypher).consume()
            t_unopt_total += time.perf_counter_ns() - t0

            t0 = time.perf_counter_ns()
            session.run(opt_cypher).consume()
            t_opt_total += time.perf_counter_ns() - t0

        unopt_ms = t_unopt_total / (iterations * 1_000_000.0)
        opt_ms = t_opt_total / (iterations * 1_000_000.0)
        speedup = ((unopt_ms - opt_ms) / unopt_ms) * 100.0 if unopt_ms > 0 else 0.0

        print(f"  Unoptimized Mean Latency : {unopt_ms:.3f} ms")
        print(f"  Optimized Mean Latency   : {opt_ms:.3f} ms")
        print(f"  Delta                    : {speedup:+.2f}%")


def main():
    if not wait_for_neo4j(90):
        sys.exit(1)

    with GraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH) as driver:
        setup_benchmark_dataset(driver)

        # Scenario 1: Static formula in WHERE filter across 10,000 traversals
        run_profile_scenario(
            driver,
            scenario_name="Scenario 1: Complex Expression in Traversal Filter (7 * 24 * 60 * 60)",
            unopt_cypher="MATCH (u:BenchUser)-[:PERFORMED]->(e:BenchEvent) WHERE e.duration_sec >= u.base_quota + (7 * 24 * 60 * 60) RETURN count(e) AS cnt",
            opt_cypher="MATCH (u:BenchUser)-[:PERFORMED]->(e:BenchEvent) WHERE e.duration_sec >= u.base_quota + 604800 RETURN count(e) AS cnt",
            iterations=50,
        )

        # Scenario 2: Arithmetic folding with index matching
        run_profile_scenario(
            driver,
            scenario_name="Scenario 2: Indexed Property Lookup with Foldable Arithmetic ((10 + 15))",
            unopt_cypher="MATCH (u:BenchUser) WHERE u.age = (10 + 15) RETURN u.user_id",
            opt_cypher="MATCH (u:BenchUser) WHERE u.age = 25 RETURN u.user_id",
            iterations=100,
        )

        # Scenario 3: Multi-hop associative arithmetic reassociation
        run_profile_scenario(
            driver,
            scenario_name="Scenario 3: Associative Reassociation ((u.age + 10) + 20 > 50) vs (u.age + 30 > 50)",
            unopt_cypher="MATCH (u:BenchUser) WHERE ((u.age + 10) + 20) > 50 RETURN count(u) AS cnt",
            opt_cypher="MATCH (u:BenchUser) WHERE (u.age + 30) > 50 RETURN count(u) AS cnt",
            iterations=50,
        )


if __name__ == "__main__":
    main()
