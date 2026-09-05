"""Example 06: SQLAlchemy Native Property Graph & Hybrid Integration (Task 4.1).

Demonstrates the 4 Pillars of Voyager's SQLAlchemy 2.0 Integration:
1. Auto-deriving PropertyGraph schemas & DDL from SQLAlchemy MetaData.
2. First-class `graph_table()` FromClause compiling into SQL:2023 GRAPH_TABLE expressions.
3. Live multi-hop recursive graph traversal (`as_cte()`) executed directly on SQLite.
4. Declarative `graph_relationship()` ORM model descriptors.
5. Dual-engine hybrid querying with zero-copy Polars DataFrame extraction.
"""

from __future__ import annotations

import sys

import polars as pl
from sqlalchemy import Column, ForeignKey, Integer, String, create_engine, select
from sqlalchemy.orm import DeclarativeBase, sessionmaker
from voyager_ogm import (
    Field,
    HybridQuery,
    HybridSession,
    MockBridge,
    Node,
    PropertyGraph,
    Query,
    Relationship,
    Session,
    as_cte,
    graph_relationship,
    graph_table,
    node,
    relationship,
)

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

pl.Config.set_ascii_tables(True)


# ---------------------------------------------------------------------------
# 1. Relational Schema: SQLAlchemy ORM Models
# ---------------------------------------------------------------------------


class Base(DeclarativeBase):
    pass


class SQLUser(Base):
    __tablename__ = "users"
    id = Column(Integer, primary_key=True)
    username = Column(String(50), nullable=False)
    department = Column(String(50), nullable=False)
    tier = Column(String(20), default="Standard")


class SQLFollows(Base):
    __tablename__ = "follows"
    follower_id = Column(Integer, ForeignKey("users.id"), primary_key=True)
    followed_id = Column(Integer, ForeignKey("users.id"), primary_key=True)
    since = Column(Integer, default=2024)


# ---------------------------------------------------------------------------
# 2. Graph Schema: Voyager OGM
# ---------------------------------------------------------------------------


@node(label="GraphUser")
class GraphUser(Node):
    user_id: int = Field(primary_key=True)
    username: str = Field()


@relationship(type_name="FOLLOWS")
class FollowsRel(Relationship):
    since: int = Field()


def main() -> None:
    print("=== Voyager OGM: SQLAlchemy Native Property Graph & Hybrid Demo ===\n")

    # 1. Initialize SQLite in-memory database
    engine = create_engine("sqlite:///:memory:")
    Base.metadata.create_all(engine)

    session_factory = sessionmaker(bind=engine)
    with session_factory() as db:
        db.add_all(
            [
                SQLUser(id=1, username="Alice", department="Engineering", tier="Enterprise"),
                SQLUser(id=2, username="Bob", department="Engineering", tier="Enterprise"),
                SQLUser(id=3, username="Charlie", department="HR", tier="Standard"),
                SQLUser(id=4, username="Diana", department="Finance", tier="Standard"),
            ]
        )
        db.add_all(
            [
                SQLFollows(follower_id=1, followed_id=2, since=2020),
                SQLFollows(follower_id=2, followed_id=3, since=2021),
                SQLFollows(follower_id=3, followed_id=4, since=2022),
            ]
        )
        db.commit()

    # -----------------------------------------------------------------------
    # Pillar 1: Auto-Derive PropertyGraph from Base.metadata
    # -----------------------------------------------------------------------
    print("[Pillar 1] Auto-Deriving PropertyGraph Catalog from SQLAlchemy MetaData:")
    pg = PropertyGraph.from_metadata(Base.metadata, name="company_graph")
    print(f"Graph Name: {pg.name}")
    print(f"Vertex Tables: {[v.table_name for v in pg.vertex_tables]}")
    print(f"Edge Tables: {[e.table_name for e in pg.edge_tables]}")
    print("\nGenerated SQL:2023 DDL:")
    print(pg.generate_create_ddl())
    print()

    # -----------------------------------------------------------------------
    # Pillar 2: First-Class `graph_table()` FromClause Compilation
    # -----------------------------------------------------------------------
    print("[Pillar 2] First-Class graph_table() FromClause (SQL:2023 / DuckPGQ):")
    u = GraphUser(alias="u")
    f = FollowsRel(alias="f")
    friend = GraphUser(alias="friend")

    gt = graph_table(
        graph=pg,
        match=Query.match(u).to(f).node(friend),
        columns=[
            ("user_id", u.user_id),
            ("friend_name", friend.username),
        ],
        alias="gt",
    )

    stmt_pgq = (
        select(SQLUser.username, gt.c.friend_name)
        .join(gt, SQLUser.id == gt.c.user_id)
        .where(SQLUser.department == "Engineering")
    )
    print("Compiled SQLAlchemy Statement:")
    print(stmt_pgq.compile(compile_kwargs={"literal_binds": True}))
    print()

    # -----------------------------------------------------------------------
    # Pillar 3: Multi-Hop Recursive CTE (Live Execution on SQLite)
    # -----------------------------------------------------------------------
    print("[Pillar 3] Live Multi-Hop Recursive Graph Traversal on SQLite via as_cte():")
    with session_factory() as session:
        # Transpile 3-hop graph traversal into recursive CTE
        cte = as_cte(
            edge_table=SQLFollows,
            source_col=SQLFollows.follower_id,
            target_col=SQLFollows.followed_id,
            max_hops=3,
            cte_name="friend_graph_cte",
        )

        # Execute single SQL query finding all friends reachable from Alice (id=1)
        stmt_cte = (
            select(SQLUser.username, cte.c.depth)
            .join(cte, SQLUser.id == cte.c.target_id)
            .where(cte.c.source_id == 1)
            .order_by(cte.c.depth)
        )
        reachable = session.execute(stmt_cte).all()
        print(f"Alice's Multi-Hop Network (max 3 hops): {reachable}")
        print()

    # -----------------------------------------------------------------------
    # Pillar 4: Declarative `graph_relationship()` ORM Descriptor
    # -----------------------------------------------------------------------
    print("[Pillar 4] Declarative graph_relationship() Descriptor:")
    friends_rel = graph_relationship(
        target=SQLUser,
        via_edge_table=SQLFollows,
        source_key=SQLFollows.follower_id,
        target_key=SQLFollows.followed_id,
        max_hops=2,
    )
    with session_factory() as session:
        alice = session.get(SQLUser, 1)
        friends_query = friends_rel.query(alice, target_model=SQLUser)
        two_hop_friends = session.execute(friends_query).scalars().all()
        print(f"Alice's 2-hop friends: {[f.username for f in two_hop_friends]}")
        print()

    # -----------------------------------------------------------------------
    # Pillar 5: Dual-Engine HybridSession & Polars DataFrame Export
    # -----------------------------------------------------------------------
    print("[Pillar 5] Dual-Engine HybridSession -> Polars Streaming:")
    mock_bridge = MockBridge()
    mock_bridge.queue_result(
        [
            {"user_id": 1, "centrality_score": 0.94, "cluster": "Core"},
            {"user_id": 2, "centrality_score": 0.81, "cluster": "Core"},
        ]
    )
    graph_session = Session(bridge=mock_bridge, dialect="cypher")

    with session_factory() as sa_session:
        hybrid = HybridSession(sa_session, graph_session)

        hq = (
            HybridQuery()
            .relational(
                select(SQLUser).where(SQLUser.tier == "Enterprise"),
                key=SQLUser.id,
            )
            .join_graph(
                Query.match(u).return_(user_id=u.user_id),
                on=u.user_id,
            )
            .relational_first()
        )
        df = hybrid.execute_to_polars(hq)
        print(df)

    print("\n[PASS] All 4 Pillars of SQLAlchemy Integration completed successfully!")


if __name__ == "__main__":
    main()
