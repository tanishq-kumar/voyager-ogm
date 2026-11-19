"""Voyager OGM: Live Multi-Engine Execution (Memgraph, FalkorDB, Apache AGE).

Teaches:
    - Executing graph queries against real live database engines
    - Memgraph: High-throughput in-memory graph execution over Bolt protocol (port 7688)
    - FalkorDB: Low-latency sparse-matrix graph execution over Redis RESP (port 6379)
    - Apache AGE: Cypher-in-SQL graph queries over PostgreSQL (port 5455)
    - Cross-dialect query compilation matching each database's native execution engine
    - Graceful offline detection when containers are not running

Prerequisites:
    - Engine: Memgraph, FalkorDB, and/or Apache AGE containers running via:
      `docker compose -f containers/compose.yaml up -d memgraph falkordb apache-age`
      (or `podman compose -f containers/compose.yaml up -d memgraph falkordb apache-age`)
    - Extras: `neo4j`, `falkordb`, `psycopg`

Run with:
    `uv run python examples/python/31_live_multi_engine_execution.py`
"""

from __future__ import annotations

import socket

from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
    create_bridge,
    node,
    relationship,
)


@node(label="Service")
class Service(Node):
    name: str = Field(primary_key=True)
    language: str = Field()
    latency_ms: float = Field(default=10.0)


@relationship(type_name="DEPENDS_ON")
class DependsOn(Relationship):
    protocol: str = Field(default="gRPC")


def is_port_open(host: str, port: int, timeout: float = 0.5) -> bool:
    """Quickly probes whether a network socket is reachable."""
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def main() -> None:
    print("=== Voyager OGM: Live Multi-Engine Database Execution ===\n")

    svc1 = Service(alias="s1", name="auth-service", language="Rust", latency_ms=4.2)
    svc2 = Service(alias="s2", name="payment-service", language="Go", latency_ms=12.5)
    dep = DependsOn(alias="d", protocol="gRPC")

    # Build universal microservices dependency query
    q = (
        Query.match(svc1)
        .to(dep)
        .node(svc2)
        .where(svc1.language == "Rust")
        .return_(source=svc1.name, target=svc2.name, protocol=dep.protocol)
    )

    print("Universal Graph Traversal Query across Dialects:")
    print("  openCypher (Memgraph / FalkorDB):")
    print(f"    {q.compile('cypher').statement}")
    print("  Apache AGE (PostgreSQL Extension):")
    print(f"    {q.compile('age', graph_name='microservices_graph').statement}\n")

    # -------------------------------------------------------------------------
    # 2. Universal Setup, Seed & Teardown Mutations via OGM Builder
    # -------------------------------------------------------------------------
    svc_clean = Service(alias="n")
    q_clean = Query.match(svc_clean).detach_delete(svc_clean)

    s1_seed = Service(alias="s1", name="auth-service", language="Rust", latency_ms=4.2)
    s2_seed = Service(alias="s2", name="payment-service", language="Go", latency_ms=12.5)
    dep_seed = DependsOn(alias="d", protocol="gRPC")
    q_seed = Query.create(s1_seed).to(dep_seed).node(s2_seed)

    engines_tested = 0

    # -------------------------------------------------------------------------
    # 1. Memgraph (Bolt on port 7688)
    # -------------------------------------------------------------------------
    print("--- [1] Memgraph (Bolt on port 7688) ---")
    if is_port_open("127.0.0.1", 7688):
        try:
            bridge_mg = create_bridge("bolt://127.0.0.1:7688")
            session_mg = Session(bridge=bridge_mg, dialect="cypher")
            session_mg.execute(q_clean)
            session_mg.execute(q_seed)
            res_mg = session_mg.execute(q)
            print(f"Memgraph live execution returned {len(res_mg)} rows:")
            for row in res_mg.mappings().all():
                print(f"  {row['source']} -> {row['target']} via {row['protocol']}")
            session_mg.execute(q_clean)
            session_mg.close()
            engines_tested += 1
        except Exception as err:
            print(f"[SKIP] Memgraph connected but failed: {err}")
    else:
        print(
            "[SKIP] Memgraph is not reachable on bolt://127.0.0.1:7688 (start with `docker compose -f containers/compose.yaml up -d memgraph` or `podman compose ...`)."
        )
    print()

    # -------------------------------------------------------------------------
    # 2. FalkorDB (Redis RESP on port 6379)
    # -------------------------------------------------------------------------
    print("--- [2] FalkorDB (port 6379) ---")
    if is_port_open("127.0.0.1", 6379):
        try:
            bridge_falkor = create_bridge("falkordb://127.0.0.1:6379")
            session_falkor = Session(bridge=bridge_falkor, dialect="falkordb")
            session_falkor.execute(q_clean)
            session_falkor.execute(q_seed)
            res_falkor = session_falkor.execute(q)
            print(f"FalkorDB live execution returned {len(res_falkor)} rows:")
            for row in res_falkor.mappings().all():
                print(f"  {row['source']} -> {row['target']} via {row['protocol']}")
            session_falkor.execute(q_clean)
            session_falkor.close()
            engines_tested += 1
        except Exception as err:
            print(f"[SKIP] FalkorDB connected but failed: {err}")
    else:
        print(
            "[SKIP] FalkorDB is not reachable on falkordb://127.0.0.1:6379 (start with `docker compose -f containers/compose.yaml up -d falkordb` or `podman compose ...`)."
        )
    print()

    # -------------------------------------------------------------------------
    # 3. Apache AGE (PostgreSQL on port 5455)
    # -------------------------------------------------------------------------
    print("--- [3] Apache AGE (PostgreSQL port 5455) ---")
    if is_port_open("127.0.0.1", 5455):
        try:
            age_uri = "postgresql://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph"
            bridge_age = create_bridge(age_uri)
            session_age = Session(
                bridge=bridge_age, dialect="age", graph_name="microservices_graph"
            )

            session_age.execute(q_clean)
            session_age.execute(q_seed)
            res_age = session_age.execute(q)
            print(f"Apache AGE live execution returned {len(res_age)} rows:")
            for row in res_age.mappings().all():
                print(f"  {row['source']} -> {row['target']} via {row['protocol']}")
            session_age.execute(q_clean)
            session_age.close()
            engines_tested += 1
        except Exception as err:
            print(f"[SKIP] Apache AGE connected but failed: {err}")
    else:
        print(
            "[SKIP] Apache AGE is not reachable on postgresql://127.0.0.1:5455 (start with `docker compose -f containers/compose.yaml up -d apache-age` or `podman compose ...`)."
        )
    print()

    if engines_tested == 0:
        print(
            "Note: No live engine containers were running. Start the engines with your Compose runner:"
        )
        print("  docker compose -f containers/compose.yaml up -d memgraph falkordb apache-age")
        print(
            "  # or: podman compose -f containers/compose.yaml up -d memgraph falkordb apache-age"
        )
    else:
        print(
            f"[SUCCESS] Successfully executed queries across {engines_tested} live graph engine(s)."
        )


if __name__ == "__main__":
    main()
