"""Seed 1,000,000 nodes and 2,000,000 edges into live Neo4j database for large-scale benchmarks."""

from __future__ import annotations

import time

from neo4j import GraphDatabase

NEO4J_URI = "bolt://127.0.0.1:7687"
NEO4J_USER = "neo4j"
NEO4J_PASS = "voyagerpass123"

TOTAL_NODES = 1_000_000
CHUNK_SIZE = 100_000


def seed_dataset() -> None:
    """Seed 1M nodes and 2M edges into the target database."""
    print("=" * 80)
    print(
        f"SEEDING LARGE-SCALE BENCHMARK DATASET: {TOTAL_NODES:,} NODES & {TOTAL_NODES * 2:,} EDGES"
    )
    print(f"Target: Neo4j at {NEO4J_URI}")
    print("=" * 80)

    driver = GraphDatabase.driver(NEO4J_URI, auth=(NEO4J_USER, NEO4J_PASS))
    driver.verify_connectivity()

    with driver.session() as s:
        # 1. Check existing count
        existing_nodes = s.run("MATCH (n:BenchPerson) RETURN count(n) AS c").single()["c"]
        existing_edges = s.run("MATCH ()-[r:KNOWS]->() RETURN count(r) AS c").single()["c"]
        print(
            f"[CHECK] Current database state: {existing_nodes:,} nodes, {existing_edges:,} edges."
        )

        if existing_nodes >= TOTAL_NODES and existing_edges >= (TOTAL_NODES * 2):
            print("[INFO] Dataset already fully seeded. Skipping seeding.")
            return

        # 2. Clean if partially seeded
        if 0 < existing_nodes < TOTAL_NODES:
            print("[CLEAN] Detected partial dataset, cleaning previous nodes...")
            s.run(
                "MATCH (n:BenchPerson) CALL (n) { DETACH DELETE n } IN TRANSACTIONS OF 10000 ROWS"
            )
            print("[CLEAN] Cleaned existing nodes.")

        # 3. Create Constraints and Indexes
        print(
            "\n[SCHEMA] Ensuring unique constraint on BenchPerson(id) and index on BenchPerson(city)..."
        )
        s.run("CREATE CONSTRAINT IF NOT EXISTS FOR (p:BenchPerson) REQUIRE p.id IS UNIQUE")
        s.run("CREATE INDEX IF NOT EXISTS FOR (p:BenchPerson) ON (p.city)")
        time.sleep(1.0)  # Wait for index to come online

        # 4. Ingest 1,000,000 Nodes in chunks of 100,000
        print(
            f"\n[NODES] Starting ingestion of {TOTAL_NODES:,} nodes across {TOTAL_NODES // CHUNK_SIZE} chunks..."
        )
        total_t0 = time.time()

        for chunk_idx in range(TOTAL_NODES // CHUNK_SIZE):
            start_id = chunk_idx * CHUNK_SIZE
            end_id = start_id + CHUNK_SIZE - 1
            chunk_t0 = time.time()

            query = """
            UNWIND range($start_id, $end_id) AS id
            CALL (id) {
                CREATE (:BenchPerson {
                    id: id,
                    name: 'User_' + toString(id),
                    age: 18 + (id % 60),
                    city: CASE (id % 8)
                        WHEN 0 THEN 'San Francisco'
                        WHEN 1 THEN 'New York'
                        WHEN 2 THEN 'London'
                        WHEN 3 THEN 'Tokyo'
                        WHEN 4 THEN 'Berlin'
                        WHEN 5 THEN 'Singapore'
                        WHEN 6 THEN 'Sydney'
                        ELSE 'Toronto' END,
                    score: 85.5 + toFloat(id % 15),
                    active: (id % 2 = 0)
                })
            } IN TRANSACTIONS OF 10000 ROWS
            """
            s.run(query, {"start_id": start_id, "end_id": end_id})
            dur = time.time() - chunk_t0
            rate = CHUNK_SIZE / dur
            print(
                f"  [Chunk {chunk_idx + 1:2d}/10] Seeded {CHUNK_SIZE:,} nodes (id: {start_id:,}..{end_id:,}) in {dur:.2f}s ({rate:,.0f} nodes/sec)"
            )

        print(f"[NODES] Completed {TOTAL_NODES:,} nodes in {time.time() - total_t0:.2f}s.")

        # 5. Ingest 2,000,000 Relationships in chunks of 100,000 (2 edges per node)
        print(
            f"\n[EDGES] Starting ingestion of {TOTAL_NODES * 2:,} edges across {TOTAL_NODES // CHUNK_SIZE} chunks..."
        )
        edges_t0 = time.time()

        for chunk_idx in range(TOTAL_NODES // CHUNK_SIZE):
            start_id = chunk_idx * CHUNK_SIZE
            end_id = start_id + CHUNK_SIZE - 1
            chunk_t0 = time.time()

            query = """
            UNWIND range($start_id, $end_id) AS id
            CALL (id) {
                MATCH (a:BenchPerson {id: id})
                MATCH (b1:BenchPerson {id: (id * 7 + 1) % 1000000})
                MATCH (b2:BenchPerson {id: (id * 13 + 3) % 1000000})
                CREATE (a)-[:KNOWS {since: 2015 + (id % 9)}]->(b1)
                CREATE (a)-[:KNOWS {since: 2018 + (id % 6)}]->(b2)
            } IN TRANSACTIONS OF 5000 ROWS
            """
            s.run(query, {"start_id": start_id, "end_id": end_id})
            dur = time.time() - chunk_t0
            edge_count = CHUNK_SIZE * 2
            rate = edge_count / dur
            print(
                f"  [Chunk {chunk_idx + 1:2d}/10] Seeded {edge_count:,} edges (src: {start_id:,}..{end_id:,}) in {dur:.2f}s ({rate:,.0f} edges/sec)"
            )

        print(f"[EDGES] Completed {TOTAL_NODES * 2:,} edges in {time.time() - edges_t0:.2f}s.")

        # 6. Final verification
        final_nodes = s.run("MATCH (n:BenchPerson) RETURN count(n) AS c").single()["c"]
        final_edges = s.run("MATCH ()-[r:KNOWS]->() RETURN count(r) AS c").single()["c"]
        print("\n" + "=" * 80)
        print(f"[SUCCESS] Verification: {final_nodes:,} nodes and {final_edges:,} edges in Neo4j.")
        print(f"Total time elapsed: {time.time() - total_t0:.2f}s")
        print("=" * 80)

    driver.close()


if __name__ == "__main__":
    seed_dataset()
