"""Unit and integration tests for Task 4.1: SQLAlchemy Hybrid Bridge.

Verifies bidirectional query coordination, transactional integrity, and Polars
DataFrame joining between SQLAlchemy relational models (SQLite, DuckDB, PostgreSQL)
and Voyager graph traversals.
"""

from __future__ import annotations

import polars as pl
import pytest

try:
    from sqlalchemy import Column, Integer, String, create_engine, select
    from sqlalchemy.orm import DeclarativeBase, sessionmaker

    HAS_SQLALCHEMY = True
except ImportError:
    HAS_SQLALCHEMY = False

from voyager_ogm import (
    AsyncHybridSession,
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
    reset_alias_counters,
)

pytestmark = pytest.mark.skipif(not HAS_SQLALCHEMY, reason="SQLAlchemy not installed")


if HAS_SQLALCHEMY:
    from sqlalchemy import ForeignKey

    class Base(DeclarativeBase):
        pass

    class SQLUser(Base):
        __tablename__ = "users"
        id = Column(Integer, primary_key=True, autoincrement=True)
        username = Column(String(50), nullable=False)
        department = Column(String(50), nullable=False)
        tier = Column(String(20), default="Standard")

    class SQLDepartment(Base):
        __tablename__ = "departments"
        id = Column(Integer, primary_key=True)
        name = Column(String(50), nullable=False)
        budget = Column(Integer, nullable=False)

    class SQLFollows(Base):
        __tablename__ = "follows"
        follower_id = Column(Integer, ForeignKey("users.id"), primary_key=True)
        followed_id = Column(Integer, ForeignKey("users.id"), primary_key=True)
        since = Column(Integer, default=2024)


@node(label="GraphUser")
class GraphUser(Node):
    user_id: int = Field(primary_key=True)
    name: str = Field()


@node(label="Server")
class Server(Node):
    server_id: str = Field(primary_key=True)
    hostname: str = Field()
    region: str = Field()


@relationship(type_name="MANAGES")
class Manages(Relationship):
    role: str = Field()


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


@pytest.fixture
def sqlite_engine():
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
                SQLDepartment(id=101, name="Engineering", budget=500000),
                SQLDepartment(id=102, name="HR", budget=150000),
                SQLDepartment(id=103, name="Finance", budget=300000),
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

    return engine


class TestSQLAlchemyHybridBridge:
    def test_hybrid_session_context_and_transactions(self, sqlite_engine):
        """Verifies context manager and commit/rollback semantics on HybridSession."""
        mock_bridge = MockBridge()
        graph_session = Session(bridge=mock_bridge, dialect="cypher")
        session_factory = sessionmaker(bind=sqlite_engine)

        with session_factory() as sa_session:
            with HybridSession(sa_session, graph_session) as hybrid:
                assert hybrid.sa is sa_session
                assert hybrid.graph is graph_session
                # Commit is executed on clean context exit

    def test_relational_to_graph_projection(self, sqlite_engine):
        """Verifies query_graph_from_relational: filtering SQL users, querying graph for servers."""
        mock_bridge = MockBridge()
        # Mock graph results for Alice (id=1) and Bob (id=2)
        mock_bridge.queue_result(
            [
                {"id": 1, "hostname": "prod-east-1.aws", "role": "Owner"},
                {"id": 1, "hostname": "prod-east-2.aws", "role": "Backup"},
                {"id": 2, "hostname": "prod-west-1.aws", "role": "Admin"},
            ]
        )
        graph_session = Session(bridge=mock_bridge, dialect="cypher")

        with sessionmaker(bind=sqlite_engine)() as sa_session:
            hybrid = HybridSession(sa_session, graph_session)

            stmt = select(SQLUser).where(SQLUser.department == "Engineering")

            def build_graph_query(user_ids: list[int]):
                u = GraphUser(alias="u")
                m = Manages(alias="m")
                s = Server(alias="s")
                return (
                    Query.match(u)
                    .to(m)
                    .node(s)
                    .where(u.user_id.in_(user_ids))
                    .return_(id=u.user_id, hostname=s.hostname, role=m.role)
                )

            df = hybrid.query_graph_from_relational(
                sa_statement=stmt,
                relational_key=SQLUser.id,
                graph_query_fn=build_graph_query,
                join_column="id",
            )

            assert isinstance(df, pl.DataFrame)
            assert df.height == 3
            assert "username" in df.columns
            assert "hostname" in df.columns
            assert "department" in df.columns
            assert set(df["username"].to_list()) == {"Alice", "Bob"}
            assert "prod-east-1.aws" in df["hostname"].to_list()

    def test_graph_to_relational_projection(self, sqlite_engine):
        """Verifies query_relational_from_graph: traversing graph first, enriching with SQL details."""
        mock_bridge = MockBridge()
        # Graph returns servers and their manager user_ids
        mock_bridge.queue_result(
            [
                {"user_id": 3, "incident_count": 5, "risk_score": 0.88},
                {"user_id": 4, "incident_count": 12, "risk_score": 0.95},
            ]
        )
        graph_session = Session(bridge=mock_bridge, dialect="cypher")

        with sessionmaker(bind=sqlite_engine)() as sa_session:
            hybrid = HybridSession(sa_session, graph_session)

            u = GraphUser(alias="u")
            g_query = Query.match(u).return_(user_id=u.user_id)

            df = hybrid.query_relational_from_graph(
                graph_query=g_query,
                graph_key="user_id",
                sa_model_or_table=SQLUser,
                sa_key=SQLUser.id,
            )

            assert isinstance(df, pl.DataFrame)
            assert df.height == 2
            assert set(df["username"].to_list()) == {"Charlie", "Diana"}
            assert "risk_score" in df.columns
            assert "department" in df.columns

    def test_fluent_hybrid_query_builder_relational_first(self, sqlite_engine):
        """Verifies execution of fluent HybridQuery DSL with relational_first priority."""
        mock_bridge = MockBridge()
        mock_bridge.queue_result(
            [
                {"user_id": 1, "managed_nodes": 15},
                {"user_id": 2, "managed_nodes": 8},
            ]
        )
        graph_session = Session(bridge=mock_bridge, dialect="cypher")

        with sessionmaker(bind=sqlite_engine)() as sa_session:
            hybrid = HybridSession(sa_session, graph_session)

            u = GraphUser(alias="u")
            hq = (
                HybridQuery()
                .relational(select(SQLUser).where(SQLUser.tier == "Enterprise"), key=SQLUser.id)
                .join_graph(
                    Query.match(u).return_(user_id=u.user_id),
                    on=u.user_id,
                )
                .relational_first()
            )

            df = hybrid.execute_to_polars(hq)
            assert isinstance(df, pl.DataFrame)
            assert df.height == 2
            assert set(df["username"].to_list()) == {"Alice", "Bob"}
            assert "managed_nodes" in df.columns

    def test_fluent_hybrid_query_builder_graph_first(self, sqlite_engine):
        """Verifies execution of fluent HybridQuery DSL with graph_first priority."""
        mock_bridge = MockBridge()
        # Graph yields user_ids 1 and 3
        mock_bridge.queue_result(
            [
                {"user_id": 1, "graph_cluster": "Core"},
                {"user_id": 3, "graph_cluster": "Edge"},
            ]
        )
        graph_session = Session(bridge=mock_bridge, dialect="cypher")

        with sessionmaker(bind=sqlite_engine)() as sa_session:
            hybrid = HybridSession(sa_session, graph_session)

            u = GraphUser(alias="u")
            hq = (
                HybridQuery()
                .relational(select(SQLUser), key=SQLUser.id)
                .join_graph(
                    Query.match(u).return_(user_id=u.user_id),
                    on=u.user_id,
                )
                .graph_first()
            )

            df = hybrid.execute_to_polars(hq)
            assert isinstance(df, pl.DataFrame)
            assert df.height == 2
            assert set(df["username"].to_list()) == {"Alice", "Charlie"}
            assert "graph_cluster" in df.columns

    def test_sync_table_to_graph_bulk(self, sqlite_engine):
        """Verifies sync_table_to_graph syncing relational table records into graph via UNWIND."""
        mock_bridge = MockBridge()
        graph_session = Session(bridge=mock_bridge, dialect="cypher")

        with sessionmaker(bind=sqlite_engine)() as sa_session:
            hybrid = HybridSession(sa_session, graph_session)

            stmt = select(SQLUser).where(SQLUser.tier == "Enterprise")
            synced_count = hybrid.sync_table_to_graph(
                sa_statement=stmt,
                node_model=GraphUser,
                key_mapping={"id": "user_id", "username": "name"},
                batch_size=50,
            )

            assert synced_count == 2
            assert len(mock_bridge.executed_queries) == 1
            stmt_cypher, params = mock_bridge.executed_queries[0]
            assert "UNWIND $batch AS row" in stmt_cypher
            assert "MERGE" in stmt_cypher
            assert len(params["batch"]) == 2
            assert params["batch"][0]["name"] == "Alice"
            assert params["batch"][1]["name"] == "Bob"

    @pytest.mark.asyncio
    async def test_async_hybrid_session(self, sqlite_engine):
        """Verifies AsyncHybridSession non-blocking execution."""
        from voyager_ogm import AsyncMockBridge, AsyncSession

        mock_bridge = AsyncMockBridge()
        mock_bridge.queue_result(
            [
                {"id": 1, "status": "Active"},
                {"id": 2, "status": "Active"},
            ]
        )
        graph_session = AsyncSession(bridge=mock_bridge, dialect="cypher")

        with sessionmaker(bind=sqlite_engine)() as sa_session:
            async with AsyncHybridSession(sa_session, graph_session) as async_hybrid:
                stmt = select(SQLUser).where(SQLUser.department == "Engineering")

                def build_graph(keys):
                    u = GraphUser(alias="u")
                    return Query.match(u).return_(id=u.user_id)

                df = await async_hybrid.query_graph_from_relational(
                    sa_statement=stmt,
                    relational_key=SQLUser.id,
                    graph_query_fn=build_graph,
                    join_column="id",
                )

                assert isinstance(df, pl.DataFrame)
                assert df.height == 2
                assert set(df["username"].to_list()) == {"Alice", "Bob"}
                assert "status" in df.columns

    def test_empty_results_and_edge_cases(self, sqlite_engine):
        """Verifies graceful handling of empty relational or graph results."""
        mock_bridge = MockBridge()
        graph_session = Session(bridge=mock_bridge, dialect="cypher")

        with sessionmaker(bind=sqlite_engine)() as sa_session:
            hybrid = HybridSession(sa_session, graph_session)

            # 1. Empty relational query
            empty_stmt = select(SQLUser).where(SQLUser.username == "NonExistent")
            df = hybrid.query_graph_from_relational(
                sa_statement=empty_stmt,
                relational_key=SQLUser.id,
                graph_query_fn=lambda keys: "MATCH (n) RETURN n",
            )
            assert df.is_empty()

            # 2. Empty graph query
            u = GraphUser(alias="u")
            df_g = hybrid.query_relational_from_graph(
                graph_query=Query.match(u).return_(u.user_id),
                graph_key="user_id",
                sa_model_or_table=SQLUser,
                sa_key=SQLUser.id,
            )
            assert df_g.is_empty()

    def test_duckdb_sqlalchemy_hybrid_pipeline(self):
        """Verifies hybrid pipeline execution with DuckDB relational tables and Voyager."""
        try:
            import duckdb
        except ImportError:
            pytest.skip("duckdb not installed")

        con = duckdb.connect(":memory:")
        con.execute(
            """
            CREATE TABLE employees (id INT, name VARCHAR, dept VARCHAR, salary INT);
            INSERT INTO employees VALUES
                (1, 'Alice', 'AI', 150000),
                (2, 'Bob', 'AI', 140000),
                (3, 'Charlie', 'Web', 110000);
            """
        )

        mock_bridge = MockBridge()
        # Mock graph results for project collaborators
        mock_bridge.queue_result(
            [
                {"id": 1, "collaborator": "Bob", "shared_repos": 8},
                {"id": 1, "collaborator": "Diana", "shared_repos": 4},
                {"id": 2, "collaborator": "Alice", "shared_repos": 8},
            ]
        )
        graph_session = Session(bridge=mock_bridge, dialect="cypher")

        hybrid = HybridSession(con, graph_session)

        # Relational-first pipeline over DuckDB
        df = hybrid.query_graph_from_relational(
            sa_statement="SELECT * FROM employees WHERE dept = 'AI'",
            relational_key="id",
            graph_query_fn=lambda keys: (
                "MATCH (e)-[r:COLLABORATES]->(c) RETURN e.id AS id, c.name AS collaborator"
            ),
            join_column="id",
        )

        assert isinstance(df, pl.DataFrame)
        assert df.height == 3
        assert set(df["name"].to_list()) == {"Alice", "Bob"}
        assert "collaborator" in df.columns
        assert "salary" in df.columns
        con.close()

    def test_property_graph_from_metadata(self):
        """Verifies Pillar 1: Auto-deriving PropertyGraph schema and DDL from SQLAlchemy MetaData."""
        pg = PropertyGraph.from_metadata(Base.metadata, name="enterprise_catalog")
        assert pg.name == "enterprise_catalog"

        v_names = {v.table_name for v in pg.vertex_tables}
        assert "users" in v_names
        assert "departments" in v_names
        assert "follows" in v_names

        # Edge table auto-detection
        edge_tables = {e.table_name: e for e in pg.edge_tables}
        assert "follows" in edge_tables
        follows_edge = edge_tables["follows"]
        assert follows_edge.source_table == "users"
        assert follows_edge.source_key == "follower_id"
        assert follows_edge.destination_table == "users"
        assert follows_edge.destination_key == "followed_id"

        # Generate DDL
        ddl = pg.generate_create_ddl()
        assert "CREATE PROPERTY GRAPH enterprise_catalog" in ddl
        assert "VERTEX TABLES (" in ddl
        assert "users LABEL User" in ddl
        assert "EDGE TABLES (" in ddl
        assert "follows" in ddl
        assert "SOURCE KEY (follower_id) REFERENCES users" in ddl

        # Dynamic Voyager Node / Relationship generation
        node_models, rel_models = pg.to_voyager_models()
        assert "User" in node_models
        assert issubclass(node_models["User"], Node)
        assert "FOLLOWS" in rel_models
        assert issubclass(rel_models["FOLLOWS"], Relationship)

    def test_graph_table_clause_compilation_and_select(self):
        """Verifies Pillar 2: graph_table() FromClause compiles into SQL:2023 GRAPH_TABLE in SQLAlchemy."""
        u = GraphUser(alias="u")
        f = Manages(alias="f")
        friend = GraphUser(alias="friend")

        gt = graph_table(
            graph="enterprise_catalog",
            match=Query.match(u).to(f).node(friend),
            columns=[
                ("user_id", u.user_id),
                ("friend_name", friend.name),
            ],
            alias="gt",
        )

        stmt = (
            select(SQLUser.username, gt.c.friend_name)
            .join(gt, SQLUser.id == gt.c.user_id)
            .where(SQLUser.department == "Engineering")
        )

        compiled_sql = str(stmt.compile(compile_kwargs={"literal_binds": True}))
        assert "SELECT users.username, gt.friend_name" in compiled_sql
        assert "FROM users JOIN GRAPH_TABLE (enterprise_catalog MATCH" in compiled_sql
        assert "COLUMNS (u.user_id AS user_id, friend.name AS friend_name)) AS gt" in compiled_sql
        assert "ON users.id = gt.user_id" in compiled_sql
        assert "WHERE users.department = 'Engineering'" in compiled_sql

    def test_as_cte_recursive_traversal_on_sqlite(self, sqlite_engine):
        """Verifies Pillar 3: as_cte() compiles recursive CTE executed live on SQLite for multi-hop graph paths."""
        session_factory = sessionmaker(bind=sqlite_engine)

        with session_factory() as session:
            # Generate recursive CTE up to 3 hops
            cte = as_cte(
                edge_table=SQLFollows,
                source_col=SQLFollows.follower_id,
                target_col=SQLFollows.followed_id,
                max_hops=3,
                cte_name="friend_path_cte",
            )

            # Query all multi-hop friends reachable from Alice (id=1)
            stmt = (
                select(SQLUser.username, cte.c.depth)
                .join(cte, SQLUser.id == cte.c.target_id)
                .where(cte.c.source_id == 1)
                .order_by(cte.c.depth)
            )

            results = session.execute(stmt).all()
            assert len(results) == 3
            # Alice (1) -> Bob (2, depth 1) -> Charlie (3, depth 2) -> Diana (4, depth 3)
            assert results[0] == ("Bob", 1)
            assert results[1] == ("Charlie", 2)
            assert results[2] == ("Diana", 3)

    def test_graph_relationship_orm_descriptor(self, sqlite_engine):
        """Verifies Pillar 4: Declarative graph_relationship descriptor on SQLAlchemy ORM models."""
        # Attach declarative graph relationship dynamically
        friends_rel = graph_relationship(
            target=SQLUser,
            via_edge_table=SQLFollows,
            source_key=SQLFollows.follower_id,
            target_key=SQLFollows.followed_id,
            max_hops=2,
        )

        session_factory = sessionmaker(bind=sqlite_engine)
        with session_factory() as session:
            alice = session.get(SQLUser, 1)
            assert alice is not None

            # Execute query for Alice's friends within 2 hops (Bob and Charlie)
            stmt = friends_rel.query(alice, target_model=SQLUser).order_by(SQLUser.id)
            reachable_friends = session.execute(stmt).scalars().all()

            assert len(reachable_friends) == 2
            friend_names = [f.username for f in reachable_friends]
            assert friend_names == ["Bob", "Charlie"]
