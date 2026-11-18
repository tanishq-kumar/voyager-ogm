"""Voyager OGM: Standalone AsyncSession & HybridSession.

Teaches:
    - Non-blocking asynchronous query execution with `AsyncSession`
    - Managing async session lifecycles with `async with AsyncSession(...)`
    - Asynchronous graph queries, health probes, and model lookups
    - Standalone `HybridSession` coordinating relational (SQLAlchemy / SQLite) and graph sessions
    - Cross-engine joins producing unified Polars DataFrames via `query_graph_from_relational()`

Prerequisites:
    - Engine: None (runs offline in-memory using AsyncMockBridge and in-memory SQLite)
    - Extras: `sqlalchemy` extra (`pip install "voyager-ogm[sqlalchemy]"`)

Run with:
    `uv run python examples/python/22_async_and_hybrid_sessions.py`
"""

from __future__ import annotations

import asyncio

import polars as pl
from sqlalchemy import Column, Integer, String, create_engine, select
from sqlalchemy.orm import DeclarativeBase, sessionmaker
from voyager_ogm import (
    AsyncHybridSession,
    AsyncMockBridge,
    AsyncSession,
    Field,
    HybridSession,
    MockBridge,
    Node,
    Query,
    Session,
    node,
)


# -----------------------------------------------------------------------------
# 1. Models: Relational (SQLAlchemy) & Graph (Voyager)
# -----------------------------------------------------------------------------
class Base(DeclarativeBase):
    pass


class SqlEmployee(Base):
    __tablename__ = "employees"

    id = Column(Integer, primary_key=True)
    name = Column(String(50), nullable=False)
    department = Column(String(50), nullable=False)
    title = Column(String(50), nullable=False)


@node(label="GraphUser")
class GraphUser(Node):
    user_id: int = Field(primary_key=True)
    skills: list[str] = Field(default_factory=list)
    centrality: float = Field(default=0.0)


# -----------------------------------------------------------------------------
# 2. AsyncSession Demonstration
# -----------------------------------------------------------------------------
async def demo_async_session() -> None:
    print("--- [1] Standalone AsyncSession ---")

    async_bridge = AsyncMockBridge()
    async_bridge.queue_result(
        [
            {
                "_graphuser_0.user_id": 1,
                "_graphuser_0.skills": ["Rust", "Python"],
                "_graphuser_0.centrality": 0.95,
            },
            {
                "_graphuser_0.user_id": 2,
                "_graphuser_0.skills": ["Cypher", "GQL"],
                "_graphuser_0.centrality": 0.88,
            },
        ]
    )

    async with AsyncSession(
        bridge=async_bridge, dialect="cypher", enable_identity_map=True
    ) as session:
        print(f"AsyncSession active: backend={session.backend}, dialect={session.dialect}")

        # Async query execution
        u = GraphUser(alias="u")
        q = Query.match(u).where(u.centrality > 0.8).return_(u.user_id, u.skills, u.centrality)

        results = await session.execute(q)
        print(f"Async query returned {len(results)} rows.")
        for row in results.mappings().all():
            print(
                f"  Row: user_id={row.get('_graphuser_0.user_id')}, centrality={row.get('_graphuser_0.centrality')}"
            )

        # Async session health check
        health = await session.health()
        print(f"Async session health: {health}\n")


# -----------------------------------------------------------------------------
# 3. HybridSession: Relational + Graph Coordination
# -----------------------------------------------------------------------------
def demo_hybrid_session() -> None:
    print("--- [2] Standalone HybridSession (SQLAlchemy + Graph) ---")

    # Set up in-memory SQLite relational database
    sa_engine = create_engine("sqlite:///:memory:")
    Base.metadata.create_all(sa_engine)
    sa_session_maker = sessionmaker(bind=sa_engine)

    with sa_session_maker() as sa_session:
        sa_session.add_all(
            [
                SqlEmployee(id=1, name="Alice", department="Core AI", title="Lead Architect"),
                SqlEmployee(id=2, name="Bob", department="Core AI", title="Systems Engineer"),
                SqlEmployee(id=3, name="Charlie", department="Platform", title="DevOps Engineer"),
            ]
        )
        sa_session.commit()

        # Set up graph mock bridge with canned graph analytics
        graph_bridge = MockBridge()
        graph_bridge.queue_result(
            [
                {"user_id": 1, "community": "Cluster-A", "score": 0.99},
                {"user_id": 2, "community": "Cluster-A", "score": 0.85},
            ]
        )
        graph_session = Session(bridge=graph_bridge, dialect="cypher")

        hybrid = HybridSession(sa_session, graph_session)

        # Cross-engine join: Filter relational table first, then enrich with graph topology
        sql_stmt = select(SqlEmployee).where(SqlEmployee.department == "Core AI")

        def build_graph_query(employee_ids: list[int]) -> Query:
            u = GraphUser(alias="gu")
            return (
                Query.match(u)
                .where(u.user_id.in_(employee_ids))
                .return_(user_id=u.user_id, community=u.centrality)
            )

        joined_df: pl.DataFrame = hybrid.query_graph_from_relational(
            sa_statement=sql_stmt,
            relational_key=SqlEmployee.id,
            graph_query_fn=build_graph_query,
            join_column="user_id",
        )

        print("Unified Relational + Graph DataFrame:")
        print(joined_df)
        print()


# -----------------------------------------------------------------------------
# 4. AsyncHybridSession Coordination
# -----------------------------------------------------------------------------
async def demo_async_hybrid_session() -> None:
    print("--- [3] Standalone AsyncHybridSession ---")

    sa_engine = create_engine("sqlite:///:memory:")
    Base.metadata.create_all(sa_engine)
    sa_session_maker = sessionmaker(bind=sa_engine)

    with sa_session_maker() as sa_session:
        sa_session.add(
            SqlEmployee(id=1, name="Diana", department="Research", title="Staff Scientist")
        )
        sa_session.commit()

        async_bridge = AsyncMockBridge()
        async_bridge.queue_result([{"user_id": 1, "pagerank": 0.98}])
        graph_async_session = AsyncSession(bridge=async_bridge, dialect="cypher")

        async with AsyncHybridSession(sa_session, graph_async_session) as async_hybrid:
            stmt = select(SqlEmployee).where(SqlEmployee.department == "Research")
            gu = GraphUser(alias="gu")
            df = await async_hybrid.query_graph_from_relational(
                sa_statement=stmt,
                relational_key=SqlEmployee.id,
                graph_query_fn=lambda keys: (
                    Query.match(gu).where(gu.user_id.in_(keys)).return_(user_id=gu.user_id)
                ),
                join_column="user_id",
            )
            print("Async Hybrid Join Result:")
            print(df)
            print()


def main() -> None:
    print("=== Voyager OGM: AsyncSession & HybridSession ===\n")
    asyncio.run(demo_async_session())
    demo_hybrid_session()
    asyncio.run(demo_async_hybrid_session())
    print("[SUCCESS] AsyncSession & HybridSession demonstration completed successfully.")


if __name__ == "__main__":
    main()
