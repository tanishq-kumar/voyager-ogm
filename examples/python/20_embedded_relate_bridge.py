"""Voyager OGM: Embedded Relate Database Bridge (relate://).

Teaches:
    - In-process, zero-network graph database execution via Relate (relatedb.ai)
    - Auto-instantiation with `create_bridge("relate://:memory:")`
    - Connectivity liveness verification via `bridge.ping()`
    - Declarative schema and unique constraints in Relate
    - CRUD entity modeling and fluent Query execution
    - High-throughput UNWIND bulk ingestion in embedded mode
    - Zero-copy Arrow / Polars DataFrame extraction from Relate query results

Prerequisites:
    - Engine: None (runs embedded in-process; no container or server needed)
    - Extras: `relate` extra (`pip install "voyager-ogm[relate]"`)

Run with:
    `uv run python examples/python/20_embedded_relate_bridge.py`
"""

from __future__ import annotations

import sys

try:
    import relate

    _ = relate
except ImportError:
    print("relate-db is not installed (install with `pip install 'voyager-ogm[relate]'`).")
    sys.exit(0)

from voyager_ogm import (
    Field,
    Node,
    Query,
    RelateBridge,
    Relationship,
    Session,
    create_bridge,
    func,
    node,
    relationship,
)


@node(label="Member")
class Member(Node):
    id: int = Field(primary_key=True)
    username: str = Field(unique=True)
    reputation: int = Field(default=100)


@node(label="Community")
class Community(Node):
    id: int = Field(primary_key=True)
    name: str = Field(unique=True)
    topic: str = Field()


@relationship(type_name="JOINED")
class Joined(Relationship):
    role: str = Field(default="contributor")
    since_year: int = Field(default=2024)


def main() -> None:
    print("=== Voyager OGM: Embedded Relate Database Bridge ===\n")

    # -------------------------------------------------------------------------
    # 1. Auto-Instantiate Relate Bridge from relate:// URI
    # -------------------------------------------------------------------------
    print("--- [1] Auto-Instantiating RelateBridge ---")
    bridge = create_bridge("relate://:memory:")
    assert isinstance(bridge, RelateBridge)
    print(f"Bridge Class:  {type(bridge).__name__}")
    print(f"Ping Liveness: {bridge.ping()} (in-process engine is active)\n")

    session = Session(bridge=bridge, dialect="cypher")

    # -------------------------------------------------------------------------
    # 2. Schema DDL: Unique Constraints in Relate via OGM
    # -------------------------------------------------------------------------
    print("--- [2] Schema DDL & Constraints ---")
    applied_constraints = session.create_constraints(Member, Community)
    for c_stmt in applied_constraints:
        print(f"  Applied constraint: {c_stmt}")
    print("Unique constraints created for Member and Community.\n")

    # -------------------------------------------------------------------------
    # 3. CRUD: Creating Entities and Querying via OGM
    # -------------------------------------------------------------------------
    print("--- [3] CRUD & Multi-Hop Query Execution ---")
    # Create nodes with parameterized properties using OGM Query
    ada = Member(alias="ada", id=1, username="Ada", reputation=950)
    alan = Member(alias="alan", id=2, username="Alan", reputation=880)
    comm = Community(alias="c10", id=10, name="GraphAI", topic="Machine Learning")

    session.execute(Query.create(ada))
    session.execute(Query.create(alan))
    session.execute(Query.create(comm))

    # Connect matched nodes via relationship patterns using OGM Query
    m_match = Member(alias="m")
    c_match = Community(alias="c")
    j1 = Joined(alias="j1")
    j2 = Joined(alias="j2")

    q_rel1 = (
        Query.match(m_match)
        .match(c_match)
        .where(m_match.id == 1, c_match.id == 10)
        .create("m")
        .to(j1)
        .node("c")
        .set(j1.role == "founder", j1.since_year == 2023)
    )
    session.execute(q_rel1)

    q_rel2 = (
        Query.match(m_match)
        .match(c_match)
        .where(m_match.id == 2, c_match.id == 10)
        .create("m")
        .to(j2)
        .node("c")
        .set(j2.role == "moderator", j2.since_year == 2024)
    )
    session.execute(q_rel2)

    # Query with fluent OGM Builder
    m = Member(alias="m")
    c = Community(alias="c")
    j = Joined(alias="j")

    q = (
        Query.match(m)
        .to(j)
        .node(c)
        .where(m.reputation >= 800)
        .return_(member=m.username, rep=m.reputation, community=c.name, role=j.role)
        .order_by(m.reputation)
    )

    results = session.execute(q)
    print(f"Found {len(results)} community members:")
    for row in results.mappings().all():
        print(f"  - {row['member']} (rep: {row['rep']}) in {row['community']} as {row['role']}")
    print()

    # -------------------------------------------------------------------------
    # 4. Zero-Copy Polars DataFrame Streaming
    # -------------------------------------------------------------------------
    print("--- [4] Zero-Copy Arrow / Polars Streaming ---")
    df = results.to_polars()
    print("Polars DataFrame:")
    print(df)
    print()

    # -------------------------------------------------------------------------
    # 5. High-Throughput UNWIND Bulk Ingestion
    # -------------------------------------------------------------------------
    print("--- [5] High-Throughput UNWIND Bulk Ingestion ---")
    new_members = [
        {"id": i, "username": f"user_{i}", "reputation": 100 + i * 5} for i in range(100, 150)
    ]

    bulk_plan = session.bulk_create(Member, new_members, batch_size=25)
    res_bulk = session.run_bulk(bulk_plan)
    print(
        f"Bulk ingested {res_bulk.total_records} members across {res_bulk.total_batches} batches in {res_bulk.duration_seconds:.5f}s."
    )

    m_all = Member(alias="m")
    total_count = (
        session.execute(Query.match(m_all).return_(total=func.count(m_all))).scalars().first()
    )
    print(f"Total Member nodes in embedded Relate database: {total_count}")

    session.close()
    print("\n[SUCCESS] Embedded Relate database bridge demonstration completed successfully.")


if __name__ == "__main__":
    main()
