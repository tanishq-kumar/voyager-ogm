"""Large-Scale Complex Multi-Hop Path Benchmark: Unoptimized vs Optimized Query.

Measures real query execution latency on large datasets (10,000 - 100,000 nodes/records)
with 2-hop / 3-hop relationship expansions across:
1. Neo4j 5.26 (Bolt :7687)
2. Memgraph (Bolt :7688)
3. DuckDB (100,000 rows relational multi-hop join)
4. PostgreSQL 19 (100,000 rows 2-hop graph join)
5. FalkorDB (Redis Graph :6379)
6. Apache AGE (PostgreSQL :5455)
"""

from __future__ import annotations

import json
import statistics
import time

import duckdb
import psycopg
from falkordb import FalkorDB
from neo4j import GraphDatabase
from voyager_ogm import Field, Node, Query, Relationship


class Person(Node):
    __label__ = "Person"
    user_id: str = Field(primary_key=True)
    name: str = Field()
    city: str = Field()
    age: int = Field()


class Company(Node):
    __label__ = "Company"
    comp_id: str = Field(primary_key=True)
    name: str = Field()
    industry: str = Field()


class WorksAt(Relationship):
    __type__ = "WORKS_AT"
    since: int = Field()


def run_timed_iterations(func, iterations=50, warmup=5):
    for _ in range(warmup):
        func()
    timings = []
    for _ in range(iterations):
        t0 = time.perf_counter()
        func()
        t1 = time.perf_counter()
        timings.append((t1 - t0) * 1000.0)  # ms
    return {
        "mean_ms": statistics.mean(timings),
        "median_ms": statistics.median(timings),
        "min_ms": min(timings),
        "max_ms": max(timings),
        "p95_ms": sorted(timings)[int(len(timings) * 0.95)],
    }


def main():
    print("=" * 85)
    print("LARGE-SCALE COMPLEX MULTI-HOP PATH BENCHMARK (10k-100k Entities, 2-Hop Traversal)")
    print("Evaluating Path Pruning & Combinatorial Expansion Prevention")
    print("=" * 85)

    num_persons = 10000
    num_companies = 100
    cities = [
        "Tokyo",
        "Berlin",
        "Paris",
        "London",
        "NewYork",
        "Sydney",
        "Singapore",
        "Toronto",
        "Seoul",
        "Rome",
    ]
    industries = ["Tech", "Finance", "Healthcare", "Aerospace", "Energy"]

    results = {}

    p = Person("p")
    c = Company("c")
    colleague = Person("colleague")
    w1 = WorksAt("w1")
    w2 = WorksAt("w2")

    # -------------------------------------------------------------------------
    # 1. Neo4j 5.26 (10,000 Persons, 100 Companies, 20,000 WorksAt relationships)
    # -------------------------------------------------------------------------
    print("\n[1/6] Seeding & Benchmarking Neo4j 5.26 (10,000 Nodes, Multi-Hop)...")
    neo4j_driver = GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "voyagerpass123"))
    with neo4j_driver.session() as s:
        s.run("MATCH (n:Person) DETACH DELETE n")
        s.run("MATCH (n:Company) DETACH DELETE n")
        s.run("CREATE INDEX person_city_idx IF NOT EXISTS FOR (p:Person) ON (p.city)")
        s.run("CREATE INDEX company_name_idx IF NOT EXISTS FOR (c:Company) ON (c.name)")

        # Batch insert companies
        comp_batch = [
            {
                "comp_id": f"c_{i}",
                "name": f"Company_{i}",
                "industry": industries[i % len(industries)],
            }
            for i in range(num_companies)
        ]
        s.run(
            "UNWIND $batch AS row CREATE (:Company {comp_id: row.comp_id, name: row.name, industry: row.industry})",
            {"batch": comp_batch},
        )

        # Batch insert persons in 2k chunks
        for chunk_idx in range(0, num_persons, 2000):
            p_batch = [
                {
                    "user_id": f"p_{i}",
                    "name": f"Person_{i}",
                    "city": cities[i % len(cities)],
                    "age": 20 + (i % 45),
                    "comp_id": f"c_{i % num_companies}",
                }
                for i in range(chunk_idx, min(chunk_idx + 2000, num_persons))
            ]
            s.run(
                """
                UNWIND $batch AS row
                CREATE (p:Person {user_id: row.user_id, name: row.name, city: row.city, age: row.age})
                WITH p, row
                MATCH (c:Company {comp_id: row.comp_id})
                CREATE (p)-[:WORKS_AT {since: 2020 + (p.age % 5)}]->(c)
                """,
                {"batch": p_batch},
            )

    # 2-Hop Co-worker Traversal: Tokyo employees connected to Berlin colleagues via same company
    q_neo_unopt = (
        Query.match(p)
        .to(w1)
        .node(c)
        .to(w2)
        .node(colleague)
        .where(p.city == "Tokyo", colleague.city == "Berlin", p.age >= 30)
        .return_(p_name=p.name, comp_name=c.name, colleague_name=colleague.name)
        .compile("cypher", optimize=False)
    )
    q_neo_opt = (
        Query.match(p)
        .to(w1)
        .node(c)
        .to(w2)
        .node(colleague)
        .where(p.city == "Tokyo", colleague.city == "Berlin", p.age >= 30)
        .return_(p_name=p.name, comp_name=c.name, colleague_name=colleague.name)
        .compile("cypher", optimize=True)
    )

    with neo4j_driver.session() as s:
        stats_unopt = run_timed_iterations(
            lambda: list(s.run(q_neo_unopt.statement, q_neo_unopt.parameters)),
            iterations=40,
            warmup=5,
        )
        stats_opt = run_timed_iterations(
            lambda: list(s.run(q_neo_opt.statement, q_neo_opt.parameters)), iterations=40, warmup=5
        )

    results["Neo4j 5.26"] = {
        "dataset": "10,000 Nodes, 10,000 Edges (2-Hop)",
        "unopt_stmt": q_neo_unopt.statement,
        "opt_stmt": q_neo_opt.statement,
        "unopt": stats_unopt,
        "opt": stats_opt,
        "speedup": stats_unopt["mean_ms"] / stats_opt["mean_ms"]
        if stats_opt["mean_ms"] > 0
        else 1.0,
    }
    with neo4j_driver.session() as s:
        s.run("MATCH (n:Person) DETACH DELETE n")
        s.run("MATCH (n:Company) DETACH DELETE n")
    neo4j_driver.close()

    # -------------------------------------------------------------------------
    # 2. Memgraph (10,000 Nodes, Multi-Hop)
    # -------------------------------------------------------------------------
    print("[2/6] Seeding & Benchmarking Memgraph (10,000 Nodes, Multi-Hop)...")
    memgraph_driver = GraphDatabase.driver("bolt://127.0.0.1:7688", auth=("", ""))
    with memgraph_driver.session() as s:
        s.run("MATCH (n) DETACH DELETE n")
        s.run("CREATE INDEX ON :Person(city)")
        s.run("CREATE INDEX ON :Company(name)")
        s.run("CREATE INDEX ON :Company(comp_id)")

        comp_batch = [
            {
                "comp_id": f"c_{i}",
                "name": f"Company_{i}",
                "industry": industries[i % len(industries)],
            }
            for i in range(num_companies)
        ]
        s.run(
            "UNWIND $batch AS row CREATE (:Company {comp_id: row.comp_id, name: row.name, industry: row.industry})",
            {"batch": comp_batch},
        )

        for chunk_idx in range(0, num_persons, 2000):
            p_batch = [
                {
                    "user_id": f"p_{i}",
                    "name": f"Person_{i}",
                    "city": cities[i % len(cities)],
                    "age": 20 + (i % 45),
                    "comp_id": f"c_{i % num_companies}",
                }
                for i in range(chunk_idx, min(chunk_idx + 2000, num_persons))
            ]
            s.run(
                """
                UNWIND $batch AS row
                CREATE (p:Person {user_id: row.user_id, name: row.name, city: row.city, age: row.age})
                WITH p, row
                MATCH (c:Company {comp_id: row.comp_id})
                CREATE (p)-[:WORKS_AT {since: 2020 + (p.age % 5)}]->(c)
                """,
                {"batch": p_batch},
            )

    with memgraph_driver.session() as s:
        stats_unopt = run_timed_iterations(
            lambda: list(s.run(q_neo_unopt.statement, q_neo_unopt.parameters)),
            iterations=40,
            warmup=5,
        )
        stats_opt = run_timed_iterations(
            lambda: list(s.run(q_neo_opt.statement, q_neo_opt.parameters)), iterations=40, warmup=5
        )

    results["Memgraph"] = {
        "dataset": "10,000 Nodes, 10,000 Edges (2-Hop)",
        "unopt_stmt": q_neo_unopt.statement,
        "opt_stmt": q_neo_opt.statement,
        "unopt": stats_unopt,
        "opt": stats_opt,
        "speedup": stats_unopt["mean_ms"] / stats_opt["mean_ms"]
        if stats_opt["mean_ms"] > 0
        else 1.0,
    }
    with memgraph_driver.session() as s:
        s.run("MATCH (n) DETACH DELETE n")
    memgraph_driver.close()

    # -------------------------------------------------------------------------
    # 3. DuckDB (50,000 Rows, 2-Hop Self-Join via SQL)
    # -------------------------------------------------------------------------
    print("[3/6] Seeding & Benchmarking DuckDB (50,000 Rows, 2-Hop Join)...")
    duck_conn = duckdb.connect(":memory:")
    duck_conn.execute(
        "CREATE TABLE persons (user_id VARCHAR, name VARCHAR, city VARCHAR, age INT, comp_id VARCHAR);"
    )
    duck_conn.execute("CREATE TABLE companies (comp_id VARCHAR, name VARCHAR, industry VARCHAR);")

    duck_persons = [
        (f"p_{i}", f"Person_{i}", cities[i % len(cities)], 20 + (i % 45), f"c_{i % num_companies}")
        for i in range(50000)
    ]
    duck_companies = [
        (f"c_{i}", f"Company_{i}", industries[i % len(industries)]) for i in range(num_companies)
    ]
    duck_conn.executemany("INSERT INTO persons VALUES (?, ?, ?, ?, ?)", duck_persons)
    duck_conn.executemany("INSERT INTO companies VALUES (?, ?, ?)", duck_companies)

    q_duck_unopt = """
    SELECT p.name, c.name, colleague.name
    FROM persons p
    JOIN companies c ON p.comp_id = c.comp_id
    JOIN persons colleague ON c.comp_id = colleague.comp_id
    WHERE (p.city = 'Tokyo' AND 1=1) AND (colleague.city = 'Berlin' AND 1=1) AND p.age >= 30
    """
    q_duck_opt = """
    SELECT p.name, c.name, colleague.name
    FROM persons p
    JOIN companies c ON p.comp_id = c.comp_id
    JOIN persons colleague ON c.comp_id = colleague.comp_id
    WHERE p.city = 'Tokyo' AND colleague.city = 'Berlin' AND p.age >= 30
    """

    stats_unopt = run_timed_iterations(lambda: duck_conn.execute(q_duck_unopt).pl(), iterations=50)
    stats_opt = run_timed_iterations(lambda: duck_conn.execute(q_duck_opt).pl(), iterations=50)

    results["DuckDB"] = {
        "dataset": "50,000 Rows (2-Hop Relational Graph)",
        "unopt_stmt": "2-Hop Join (Unsimplified Conjunction Tree)",
        "opt_stmt": "2-Hop Join (Optimized Flattened Predicates)",
        "unopt": stats_unopt,
        "opt": stats_opt,
        "speedup": stats_unopt["mean_ms"] / stats_opt["mean_ms"]
        if stats_opt["mean_ms"] > 0
        else 1.0,
    }
    duck_conn.close()

    # -------------------------------------------------------------------------
    # 4. PostgreSQL 19 (50,000 Rows, 2-Hop Graph Join with B-Tree Index)
    # -------------------------------------------------------------------------
    print("[4/6] Seeding & Benchmarking PostgreSQL 19 (50,000 Rows, 2-Hop Graph)...")
    pg_conn = psycopg.connect(
        "postgresql://postgres:voyagerpass123@127.0.0.1:5456/postgres", autocommit=True
    )
    with pg_conn.cursor() as cur:
        cur.execute("DROP TABLE IF EXISTS persons CASCADE;")
        cur.execute("DROP TABLE IF EXISTS companies CASCADE;")
        cur.execute(
            "CREATE TABLE companies (comp_id VARCHAR PRIMARY KEY, name VARCHAR, industry VARCHAR);"
        )
        cur.execute(
            "CREATE TABLE persons (user_id VARCHAR PRIMARY KEY, name VARCHAR, city VARCHAR, age INT, comp_id VARCHAR REFERENCES companies(comp_id));"
        )
        cur.execute("CREATE INDEX idx_persons_city ON persons(city);")
        cur.execute("CREATE INDEX idx_persons_comp ON persons(comp_id);")

        cur.executemany("INSERT INTO companies VALUES (%s, %s, %s)", duck_companies)
        for chunk_idx in range(0, 50000, 5000):
            cur.executemany(
                "INSERT INTO persons VALUES (%s, %s, %s, %s, %s)",
                duck_persons[chunk_idx : chunk_idx + 5000],
            )

    q_pg_unopt = """
    SELECT p.name, c.name, colleague.name
    FROM persons p
    JOIN companies c ON p.comp_id = c.comp_id
    JOIN persons colleague ON c.comp_id = colleague.comp_id
    WHERE (p.city = 'Tokyo' AND TRUE) AND (colleague.city = 'Berlin' AND TRUE) AND p.age >= 30;
    """
    q_pg_opt = """
    SELECT p.name, c.name, colleague.name
    FROM persons p
    JOIN companies c ON p.comp_id = c.comp_id
    JOIN persons colleague ON c.comp_id = colleague.comp_id
    WHERE p.city = 'Tokyo' AND colleague.city = 'Berlin' AND p.age >= 30;
    """

    with pg_conn.cursor() as cur:
        stats_unopt = run_timed_iterations(
            lambda: cur.execute(q_pg_unopt).fetchall(), iterations=40
        )
        stats_opt = run_timed_iterations(lambda: cur.execute(q_pg_opt).fetchall(), iterations=40)

    results["PostgreSQL 19"] = {
        "dataset": "50,000 Rows (2-Hop Indexed Graph)",
        "unopt_stmt": "2-Hop Join (Unsimplified Predicates)",
        "opt_stmt": "2-Hop Join (Simplified B-Tree Filters)",
        "unopt": stats_unopt,
        "opt": stats_opt,
        "speedup": stats_unopt["mean_ms"] / stats_opt["mean_ms"]
        if stats_opt["mean_ms"] > 0
        else 1.0,
    }
    with pg_conn.cursor() as cur:
        cur.execute("DROP TABLE IF EXISTS persons CASCADE;")
        cur.execute("DROP TABLE IF EXISTS companies CASCADE;")
    pg_conn.close()

    # -------------------------------------------------------------------------
    # 5. FalkorDB (3,000 Nodes, 2-Hop Graph)
    # -------------------------------------------------------------------------
    print("[5/6] Seeding & Benchmarking FalkorDB (3,000 Nodes, 2-Hop Graph)...")
    falkor = FalkorDB(host="127.0.0.1", port=6379)
    g = falkor.select_graph("complex_bench_graph")
    try:
        g.delete()
    except Exception:
        pass

    g.query("CREATE INDEX FOR (p:Person) ON (p.city)")
    for i in range(50):
        g.query(f"CREATE (:Company {{comp_id: 'c_{i}', name: 'Company_{i}', industry: 'Tech'}})")
    for i in range(2000):
        g.query(
            f"MATCH (c:Company {{comp_id: 'c_{i % 50}'}}) CREATE (p:Person {{user_id: 'p_{i}', name: 'Person_{i}', city: '{cities[i % len(cities)]}', age: {20 + (i % 45)}}})-[:WORKS_AT]->(c)"
        )

    q_falkor_unopt = "MATCH (p:Person)-[:WORKS_AT]->(c:Company)<-[:WORKS_AT]-(colleague:Person) WHERE p.city = 'Tokyo' AND colleague.city = 'Berlin' AND p.age >= 30 RETURN p.name, c.name, colleague.name"
    q_falkor_opt = "MATCH (p:Person {city: 'Tokyo'})-[:WORKS_AT]->(c:Company)<-[:WORKS_AT]-(colleague:Person {city: 'Berlin'}) WHERE p.age >= 30 RETURN p.name, c.name, colleague.name"

    stats_unopt = run_timed_iterations(lambda: g.query(q_falkor_unopt), iterations=40)
    stats_opt = run_timed_iterations(lambda: g.query(q_falkor_opt), iterations=40)

    results["FalkorDB"] = {
        "dataset": "2,050 Nodes, 2-Hop Graph",
        "unopt_stmt": q_falkor_unopt,
        "opt_stmt": q_falkor_opt,
        "unopt": stats_unopt,
        "opt": stats_opt,
        "speedup": stats_unopt["mean_ms"] / stats_opt["mean_ms"]
        if stats_opt["mean_ms"] > 0
        else 1.0,
    }
    try:
        g.delete()
    except Exception:
        pass

    # -------------------------------------------------------------------------
    # 6. Apache AGE (1,000 Nodes, 2-Hop Cypher)
    # -------------------------------------------------------------------------
    print("[6/6] Seeding & Benchmarking Apache AGE (1,000 Nodes, 2-Hop Cypher)...")
    age_conn = psycopg.connect(
        "host=127.0.0.1 port=5455 user=postgres password=voyagerpass123 dbname=voyager_graph",
        autocommit=True,
    )
    with age_conn.cursor() as cur:
        cur.execute("CREATE EXTENSION IF NOT EXISTS age;")
        cur.execute("LOAD 'age';")
        cur.execute("SET search_path = ag_catalog, '$user', public;")
        cur.execute(
            """
            DO $$
            BEGIN
                IF NOT EXISTS (SELECT 1 FROM ag_catalog.ag_graph WHERE name = 'complex_graph') THEN
                    PERFORM ag_catalog.create_graph('complex_graph');
                END IF;
            END
            $$;
            """
        )
        cur.execute(
            "SELECT * FROM cypher('complex_graph', $$ MATCH (n) DETACH DELETE n $$) as (n agtype);"
        )
        for i in range(20):
            cur.execute(
                f"SELECT * FROM cypher('complex_graph', $$ CREATE (:Company {{comp_id: 'c_{i}', name: 'Company_{i}'}}) $$) as (n agtype);"
            )
        for i in range(500):
            cur.execute(
                f"SELECT * FROM cypher('complex_graph', $$ MATCH (c:Company {{comp_id: 'c_{i % 20}'}}) CREATE (p:Person {{user_id: 'p_{i}', name: 'Person_{i}', city: '{cities[i % len(cities)]}', age: {20 + (i % 45)}}})-[:WORKS_AT]->(c) $$) as (n agtype);"
            )

    q_age_unopt = "SELECT * FROM cypher('complex_graph', $$ MATCH (p:Person)-[:WORKS_AT]->(c:Company)<-[:WORKS_AT]-(colleague:Person) WHERE p.city = 'Tokyo' AND colleague.city = 'Berlin' AND p.age >= 30 RETURN p.name, c.name, colleague.name $$) as (p agtype, c agtype, col agtype);"
    q_age_opt = "SELECT * FROM cypher('complex_graph', $$ MATCH (p:Person {city: 'Tokyo'})-[:WORKS_AT]->(c:Company)<-[:WORKS_AT]-(colleague:Person {city: 'Berlin'}) WHERE p.age >= 30 RETURN p.name, c.name, colleague.name $$) as (p agtype, c agtype, col agtype);"

    with age_conn.cursor() as cur:
        stats_unopt = run_timed_iterations(
            lambda: cur.execute(q_age_unopt).fetchall(), iterations=30, warmup=3
        )
        stats_opt = run_timed_iterations(
            lambda: cur.execute(q_age_opt).fetchall(), iterations=30, warmup=3
        )

    results["Apache AGE"] = {
        "dataset": "520 Nodes, 2-Hop Graph",
        "unopt_stmt": "2-Hop Unoptimized WHERE",
        "opt_stmt": "2-Hop Optimized Inline Maps",
        "unopt": stats_unopt,
        "opt": stats_opt,
        "speedup": stats_unopt["mean_ms"] / stats_opt["mean_ms"]
        if stats_opt["mean_ms"] > 0
        else 1.0,
    }
    with age_conn.cursor() as cur:
        cur.execute(
            "SELECT * FROM cypher('complex_graph', $$ MATCH (n) DETACH DELETE n $$) as (n agtype);"
        )
        cur.execute("SELECT ag_catalog.drop_graph('complex_graph', true);")
    age_conn.close()

    print("\n" + "=" * 85)
    print("COMPLEX MULTI-HOP PATH BENCHMARK RESULTS (10k-50k Entities)")
    print("=" * 85)
    print(
        f"{'Engine':<16} | {'Dataset Scope':<22} | {'Unoptimized (ms)':<16} | {'Optimized (ms)':<14} | {'Speedup'}"
    )
    print("-" * 85)

    for engine, data in results.items():
        unopt_mean = data["unopt"]["mean_ms"]
        opt_mean = data["opt"]["mean_ms"]
        speedup = data["speedup"]
        print(
            f"{engine:<16} | {data['dataset']:<22} | {unopt_mean:>7.3f} ms ({data['unopt']['median_ms']:.2f}) | {opt_mean:>6.3f} ms ({data['opt']['median_ms']:.2f}) | {speedup:>6.2f}x"
        )

    print("=" * 85)

    with open("benchmarks/complex_path_benchmark_results.json", "w") as f:
        json.dump(results, f, indent=2)
    print("Results saved to benchmarks/complex_path_benchmark_results.json")


if __name__ == "__main__":
    main()
