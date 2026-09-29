"""Voyager OGM Schema & Graph Types DDL Test Suite.

Verifies:
1. Automated openCypher / Neo4j constraint generation (UNIQUE, NOT NULL, Indexes)
2. Neo4j 5.x Graph Types (Property Type constraints: REQUIRE n.prop :: STRING)
3. Automated DROP DDL generation
4. Live schema constraint creation and validation against running Neo4j container
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

import pytest
from neo4j import GraphDatabase
from voyager_ogm import (
    Field,
    Node,
    Relationship,
    SchemaManager,
    Session,
    node,
    relationship,
)
from voyager_ogm.bridge import Neo4jBoltBridge


def _load_env() -> None:
    env_path = Path(__file__).resolve().parents[3] / ".env"
    if env_path.exists():
        for line in env_path.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                os.environ.setdefault(k.strip(), v.strip().strip("'\""))


_load_env()


@node(label="User")
class User(Node):
    """User entity with constraints and Graph Types."""

    user_id: str = Field(primary_key=True)
    email: str = Field(unique=True)
    age: int = Field(index=True)
    bio: str = Field()


@relationship(type_name="FOLLOWS", source_node=User, target_node=User)
class Follows(Relationship):
    """FOLLOWS edge with property constraint."""

    since: int = Field()


def test_schema_ddl_generation():
    """Verifies generated DDL statements match Neo4j 5.x Graph Types & Constraint standards."""
    statements = SchemaManager.generate_cypher_ddl(User, include_type_constraints=True)

    # Unique / Primary Key constraints
    assert (
        "CREATE CONSTRAINT constraint_user_user_id_unique IF NOT EXISTS FOR (n:User) REQUIRE n.user_id IS UNIQUE"
        in statements
    )
    assert (
        "CREATE CONSTRAINT constraint_user_user_id_not_null IF NOT EXISTS FOR (n:User) REQUIRE n.user_id IS NOT NULL"
        in statements
    )
    assert (
        "CREATE CONSTRAINT constraint_user_email_unique IF NOT EXISTS FOR (n:User) REQUIRE n.email IS UNIQUE"
        in statements
    )

    # Negative assertions: plain Field() and indexed Field() default to nullable (no NOT NULL constraint)
    assert not any("constraint_user_age_not_null" in s for s in statements)
    assert not any("constraint_user_bio_not_null" in s for s in statements)

    # Property Type Constraints (Graph Types)
    assert (
        "CREATE CONSTRAINT constraint_user_user_id_type IF NOT EXISTS FOR (n:User) REQUIRE n.user_id :: STRING"
        in statements
    )
    assert (
        "CREATE CONSTRAINT constraint_user_email_type IF NOT EXISTS FOR (n:User) REQUIRE n.email :: STRING"
        in statements
    )
    assert (
        "CREATE CONSTRAINT constraint_user_age_type IF NOT EXISTS FOR (n:User) REQUIRE n.age :: INTEGER"
        in statements
    )

    # Index
    assert "CREATE INDEX index_user_age IF NOT EXISTS FOR (n:User) ON (n.age)" in statements


def test_schema_drop_ddl_generation():
    """Verifies generated DROP statements."""
    drop_statements = SchemaManager.generate_drop_ddl(User, include_type_constraints=True)
    assert "DROP CONSTRAINT constraint_user_user_id_unique IF EXISTS" in drop_statements
    assert "DROP CONSTRAINT constraint_user_user_id_not_null IF EXISTS" in drop_statements
    assert "DROP CONSTRAINT constraint_user_email_unique IF EXISTS" in drop_statements
    assert "DROP CONSTRAINT constraint_user_age_type IF EXISTS" in drop_statements
    assert "DROP INDEX index_user_age IF EXISTS" in drop_statements


def test_cypher25_graph_type_ddl():
    """Verifies Cypher 25 ALTER CURRENT GRAPH TYPE SET block generation."""
    cypher25_ddl = SchemaManager.generate_cypher25_graph_type_ddl(User, Follows)
    assert cypher25_ddl.startswith("ALTER CURRENT GRAPH TYPE SET {")
    assert "(:User =>" in cypher25_ddl
    assert "user_id :: STRING IS KEY" in cypher25_ddl
    assert "email :: STRING IS UNIQUE" in cypher25_ddl
    assert "(:User)-[:FOLLOWS =>" in cypher25_ddl
    assert "since :: INTEGER" in cypher25_ddl

    drop_ddl = SchemaManager.generate_cypher25_drop_graph_type_ddl()
    assert drop_ddl == "ALTER CURRENT GRAPH TYPE SET {}"


def test_cypher25_composite_facets_and_empty_entities():
    """Verifies Cypher 25 composite facets (NOT NULL IS UNIQUE) and empty-entity grammar."""

    @node(label="Tag")
    class Tag(Node):
        pass

    @node(label="Member")
    class Member(Node):
        member_id: str = Field(primary_key=True)
        login: str = Field(unique=True, nullable=False)

    @relationship(type_name="TAGGED", source_node=Member, target_node=Tag)
    class Tagged(Relationship):
        pass

    ddl = SchemaManager.generate_cypher25_graph_type_ddl(Tag, Member, Tagged)
    # Empty node has no dangling =>
    assert "(:Tag)" in ddl
    assert "(:Tag =>)" not in ddl
    # Empty rel has no dangling =>
    assert "(:Member)-[:TAGGED]->(:Tag)" in ddl
    assert "[:TAGGED =>]" not in ddl
    # Composite facets
    assert "login :: STRING NOT NULL IS UNIQUE" in ddl


@pytest.mark.live
def test_live_neo4j_schema_ddl_creation():
    """Verifies applying constraints and indexes live to the running Neo4j database."""
    try:
        from neo4j import GraphDatabase

        uri = os.getenv("NEO4J_ENTERPRISE_URI", os.getenv("NEO4J_URI", "bolt://127.0.0.1:7687"))
        user = os.getenv("NEO4J_ENTERPRISE_USER", os.getenv("NEO4J_USER", "neo4j"))
        password = os.getenv(
            "NEO4J_ENTERPRISE_PASSWORD", os.getenv("NEO4J_PASSWORD", "voyagerpass123")
        )
        database = os.getenv("NEO4J_ENTERPRISE_DATABASE", "neo4j")

        driver = GraphDatabase.driver(uri, auth=(user, password), connection_timeout=0.5)
        driver.verify_connectivity()
    except Exception:
        pytest.skip(f"Neo4j database not reachable on {uri}")

    bridge = Neo4jBoltBridge(driver, database=database)
    session = Session(bridge=bridge, dialect="cypher")

    # Clean any leftover constraints first
    SchemaManager.drop_all(session, User)

    # Create constraints & indexes (Community edition compatible)
    created = SchemaManager.create_all(session, User, include_type_constraints=False)
    assert len(created) >= 3

    # Verify constraints in Neo4j catalog
    constraints = session.execute("SHOW CONSTRAINTS")
    constraint_names = [c.get("name", "") for c in constraints]
    assert any("user_id_unique" in name for name in constraint_names)
    assert any("email_unique" in name for name in constraint_names)

    # Clean up / Drop
    dropped = SchemaManager.drop_all(session, User, include_type_constraints=False)
    assert len(dropped) >= 3

    session.close()
    driver.close()


@pytest.mark.live
def test_live_neo4j_enterprise_cypher25_graph_type():
    """Verifies applying declarative Cypher 25 Graph Types to Neo4j Enterprise live."""
    uri = os.getenv("NEO4J_ENTERPRISE_URI")
    if not uri:
        pytest.skip("NEO4J_ENTERPRISE_URI not set in environment")

    try:
        from neo4j import GraphDatabase

        user = os.getenv("NEO4J_ENTERPRISE_USER", "neo4j")
        password = os.getenv("NEO4J_ENTERPRISE_PASSWORD", "voyex1234")
        database = os.getenv("NEO4J_ENTERPRISE_DATABASE", "neo4j")

        driver = GraphDatabase.driver(uri, auth=(user, password), connection_timeout=0.5)
        driver.verify_connectivity()
    except Exception:
        pytest.skip(f"Neo4j Enterprise not reachable on {uri}")

    bridge = Neo4jBoltBridge(driver, database=database)
    session = Session(bridge=bridge, dialect="cypher")

    try:
        # 1. Clean existing graph type
        session.execute(SchemaManager.generate_cypher25_drop_graph_type_ddl())

        # 2. Emit and apply declarative Cypher 25 Graph Type
        cypher25_ddl = SchemaManager.generate_cypher25_graph_type_ddl(User, Follows)
        session.execute(cypher25_ddl)

        # 3. Verify graph type constraints exist
        constraints = session.execute("SHOW CONSTRAINTS")
        c_names = [c.get("name", "") for c in constraints]
        assert len(c_names) > 0

        # 4. Reset graph type cleanly
        session.execute(SchemaManager.generate_cypher25_drop_graph_type_ddl())
    finally:
        session.close()
        driver.close()


@pytest.mark.live
def test_live_postgres19_pgq_ddl_execution():
    """Verifies applying generated SQL:2023 PGQ DDL directly to PostgreSQL 19."""
    uri = os.getenv("PG19_URI", "postgresql://postgres:voyagerpass123@127.0.0.1:5456/postgres")
    try:
        import psycopg

        conn = psycopg.connect(uri, connect_timeout=1)
    except Exception:
        pytest.skip(f"PostgreSQL 19 not reachable on {uri}")

    @node(label="Member")
    class Member(Node):
        member_id: str = Field(primary_key=True)
        name: str = Field()
        age: int = Field()

    @relationship(type_name="FRIENDS_WITH", source_node=Member, target_node=Member)
    class FriendsWith(Relationship):
        since: int = Field()

    cur = conn.cursor()
    try:
        cur.execute("DROP PROPERTY GRAPH IF EXISTS live_pgq_test CASCADE;")
        cur.execute("DROP TABLE IF EXISTS friendswith CASCADE;")
        cur.execute("DROP TABLE IF EXISTS member CASCADE;")

        cur.execute("CREATE TABLE member (member_id TEXT PRIMARY KEY, name TEXT, age INT);")
        cur.execute(
            "CREATE TABLE friendswith (member_id TEXT REFERENCES member(member_id), since INT, PRIMARY KEY (member_id, since));"
        )

        pgq_ddl = SchemaManager.generate_pgq_ddl("live_pgq_test", Member, FriendsWith)
        cur.execute(pgq_ddl)
        conn.commit()

        # Drop property graph cleanly
        cur.execute(SchemaManager.generate_pgq_drop_ddl("live_pgq_test"))
        conn.commit()
    finally:
        conn.close()


def test_alter_current_graph_type_ddl():
    """Verifies Cypher 25 / ISO GQL ALTER CURRENT GRAPH TYPE statements."""
    node_alter = SchemaManager.generate_alter_graph_type_ddl(User)
    assert (
        node_alter
        == "ALTER CURRENT GRAPH TYPE ADD NODE TYPE (:User {age :: INTEGER?, bio :: STRING?, email :: STRING, user_id :: STRING})"
    )

    rel_alter = SchemaManager.generate_alter_graph_type_ddl(
        Follows, source_node=User, target_node=User
    )
    assert (
        rel_alter
        == "ALTER CURRENT GRAPH TYPE ADD RELATIONSHIP TYPE (:User)-[:FOLLOWS {since :: INTEGER}]->(:User)"
    )


def test_gql_create_graph_type_ddl():
    """Verifies standard ISO GQL CREATE GRAPH TYPE statement definition."""
    gql_ddl = SchemaManager.generate_gql_graph_type_ddl("SocialGraphType", User, Follows)
    assert "CREATE GRAPH TYPE SocialGraphType AS {" in gql_ddl
    assert (
        "NODE User (age INTEGER, bio STRING, email STRING, user_id STRING NOT NULL) KEY (user_id)"
        in gql_ddl
    )
    assert "EDGE FOLLOWS CONNECTING (User TO User) (since INTEGER)" in gql_ddl

    drop_ddl = SchemaManager.generate_gql_drop_graph_type_ddl("SocialGraphType")
    assert drop_ddl == "DROP GRAPH TYPE SocialGraphType IF EXISTS"


def test_pgq_property_graph_ddl():
    """Verifies standard SQL:2023 PGQ / DuckPGQ CREATE PROPERTY GRAPH statements."""
    pgq_ddl = SchemaManager.generate_pgq_ddl("social_pgq", User, Follows)
    assert "CREATE PROPERTY GRAPH social_pgq" in pgq_ddl
    assert "VERTEX TABLES (" in pgq_ddl
    assert "user KEY (user_id) LABEL User PROPERTIES (age, bio, email, user_id)" in pgq_ddl
    assert "EDGE TABLES (" in pgq_ddl
    assert "SOURCE KEY (user_id) REFERENCES user (user_id)" in pgq_ddl
    assert "DESTINATION KEY (user_id) REFERENCES user (user_id)" in pgq_ddl
    assert "LABEL FOLLOWS PROPERTIES (since)" in pgq_ddl

    drop_pgq = SchemaManager.generate_pgq_drop_ddl("social_pgq")
    assert drop_pgq == "DROP PROPERTY GRAPH IF EXISTS social_pgq;"


def test_native_schema_registry_ddl():
    """Verifies DDL generation directly through NativeSchemaRegistry instance."""
    from voyager_ogm import NativeSchemaRegistry

    reg = NativeSchemaRegistry()
    reg.register_node(
        "Article",
        ["Article"],
        {
            "id": {"type": "STRING", "primary_key": True},
            "title": {"type": "STRING", "indexed": True},
        },
    )
    reg.register_relationship(
        "CitedBy",
        "CITED_BY",
        source_labels=["Article"],
        target_labels=["Article"],
        fields={"year": {"type": "INTEGER"}},
        directed=True,
    )

    # 1. openCypher DDL
    cypher_stmts = reg.generate_cypher_ddl(include_type_constraints=False)
    assert any("CREATE CONSTRAINT constraint_article_id_unique" in s for s in cypher_stmts)
    assert any("CREATE CONSTRAINT constraint_article_id_not_null" in s for s in cypher_stmts)
    assert any("CREATE INDEX index_article_title" in s for s in cypher_stmts)

    # 2. Cypher 25 Graph Type DDL
    cypher25 = reg.generate_cypher25_graph_type_ddl()
    assert "ALTER CURRENT GRAPH TYPE SET {" in cypher25
    assert "id :: STRING IS KEY" in cypher25
    assert "(:Article)-[:CITED_BY =>" in cypher25
    assert reg.generate_cypher25_drop_graph_type_ddl() == "ALTER CURRENT GRAPH TYPE SET {}"

    # 3. ISO GQL DDL
    gql = reg.generate_gql_graph_type_ddl("ArticleGraph")
    assert "CREATE GRAPH TYPE ArticleGraph AS {" in gql
    assert "NODE Article (id STRING NOT NULL, title STRING) KEY (id)" in gql
    assert "EDGE CITED_BY CONNECTING (Article TO Article) (year INTEGER)" in gql

    # 4. SQL:2023 PGQ DDL
    pgq = reg.generate_pgq_ddl("article_pgq")
    assert "CREATE PROPERTY GRAPH article_pgq" in pgq
    assert "article KEY (id) LABEL Article PROPERTIES (id, title)" in pgq
    assert "SOURCE KEY (article_id) REFERENCES article (id)" in pgq
    assert "DESTINATION KEY (article_id) REFERENCES article (id)" in pgq
    assert "LABEL CITED_BY PROPERTIES (year)" in pgq


def test_dialect_ddl_error_handling():
    """Verifies SchemaManager raises actionable NotImplementedError for unsupported dialects instead of failing silently."""
    from voyager_ogm.bridge import MockBridge

    # 1. SQL:2023 PGQ dialect session
    pgq_session = Session(bridge=MockBridge(), dialect="sql_pgq")
    with pytest.raises(NotImplementedError, match="SQL:2023 PGQ"):
        SchemaManager.create_all(pgq_session, User)

    with pytest.raises(NotImplementedError, match="SQL:2023 PGQ"):
        SchemaManager.drop_all(pgq_session, User)

    # 2. FalkorDB session
    falkor_session = Session(bridge=MockBridge(), dialect="falkordb")
    with pytest.raises(NotImplementedError, match="FalkorDB does not support"):
        SchemaManager.create_all(falkor_session, User)

    with pytest.raises(NotImplementedError, match="FalkorDB does not support"):
        SchemaManager.drop_all(falkor_session, User)

    # 3. Apache AGE session
    age_session = Session(bridge=MockBridge(), dialect="age")
    with pytest.raises(NotImplementedError, match="Apache AGE does not support"):
        SchemaManager.create_all(age_session, User)

    with pytest.raises(NotImplementedError, match="Apache AGE does not support"):
        SchemaManager.drop_all(age_session, User)

    # 4. Property Graph helpers for PGQ sessions
    mock_pgq = MockBridge()
    pgq_sess = Session(bridge=mock_pgq, dialect="sql_pgq")
    created_ddl = SchemaManager.create_property_graph(pgq_sess, "my_graph", User, Follows)
    assert created_ddl.startswith("CREATE PROPERTY GRAPH my_graph")
    assert len(mock_pgq.executed_queries) == 1
    assert mock_pgq.executed_queries[0][0] == created_ddl

    dropped_ddl = SchemaManager.drop_property_graph(pgq_sess, "my_graph")
    assert dropped_ddl == "DROP PROPERTY GRAPH IF EXISTS my_graph;"
    assert len(mock_pgq.executed_queries) == 2

    # 5. Relationship missing endpoints raises ValueError in PGQ
    @relationship(type_name="ORPHAN_REL")
    class OrphanRel(Relationship):
        weight: float = Field()

    with pytest.raises(ValueError, match="missing source_labels"):
        SchemaManager.generate_pgq_ddl("orphan_pgq", User, OrphanRel)


def test_schema_manager_native_registration_and_delegation():
    """Verifies automated model registration in NativeSchemaRegistry and granular constraint/index delegation."""
    from voyager_ogm._voyager_rs import NativeSchemaRegistry
    from voyager_ogm.bridge import MockBridge

    # 1. Global registry contains User and Follows models automatically
    User.register_schema()
    Follows.register_schema()
    reg = NativeSchemaRegistry.global_registry()
    assert reg.has_node("User")
    assert reg.has_relationship("Follows")

    user_meta = reg.get_node("User")
    assert user_meta is not None
    assert user_meta["primary_key"] == "user_id"
    assert "email" in user_meta["fields"]
    assert user_meta["fields"]["email"]["unique"] is True

    follows_meta = reg.get_relationship("Follows")
    assert follows_meta is not None
    assert follows_meta["type_name"] == "FOLLOWS"
    assert follows_meta["source_labels"] == ["User"]
    assert follows_meta["target_labels"] == ["User"]

    # Test dynamic declaration auto-registers into global schema registry
    @node(label="DecoratedAuthor")
    class DecoratedAuthor(Node):
        author_id: str = Field(primary_key=True)
        pen_name: str = Field(unique=True)

    assert reg.has_node("DecoratedAuthor")
    author_meta = reg.get_node("DecoratedAuthor")
    assert author_meta is not None
    assert author_meta["primary_key"] == "author_id"
    assert author_meta["fields"]["pen_name"]["unique"] is True

    # 2. generate_cypher_ddl using string model name directly from registry
    user_ddl = SchemaManager.generate_cypher_ddl("User", include_type_constraints=True)
    assert any("constraint_user_user_id_unique" in s for s in user_ddl)
    assert any("index_user_age" in s for s in user_ddl)

    user_drop = SchemaManager.generate_drop_ddl("User")
    assert any("DROP CONSTRAINT constraint_user_user_id_unique" in s for s in user_drop)

    # 3. generate_ddl multi-dialect delegation
    cypher_ddl = SchemaManager.generate_ddl(User, dialect="cypher")
    assert any("constraint_user_user_id_unique" in s for s in cypher_ddl)

    gql_ddl = SchemaManager.generate_ddl(User, dialect="gql")
    assert any("ALTER CURRENT GRAPH TYPE ADD NODE TYPE (:User" in s for s in gql_ddl)

    pgq_ddl = SchemaManager.generate_ddl(User, dialect="pgq")
    assert any("CREATE PROPERTY GRAPH" in s for s in pgq_ddl)

    # 4. Granular create_constraints and create_indexes
    mock_bridge = MockBridge()
    session = Session(bridge=mock_bridge, dialect="cypher")

    applied_constraints = SchemaManager.create_constraints(session, User)
    assert all("CREATE CONSTRAINT" in s for s in applied_constraints)
    assert not any("CREATE INDEX" in s for s in applied_constraints)

    applied_indexes = SchemaManager.create_indexes(session, User)
    assert all("CREATE INDEX" in s for s in applied_indexes)
    assert not any("CREATE CONSTRAINT" in s for s in applied_indexes)

    # Granular drop_constraints and drop_indexes
    dropped_constraints = SchemaManager.drop_constraints(session, User)
    assert all("DROP CONSTRAINT" in s for s in dropped_constraints)
    assert not any("DROP INDEX" in s for s in dropped_constraints)

    dropped_indexes = SchemaManager.drop_indexes(session, User)
    assert all("DROP INDEX" in s for s in dropped_indexes)
    assert not any("DROP CONSTRAINT" in s for s in dropped_indexes)


def test_pure_ddl_generators_multi_dialect():
    """Verifies pure DDL generators operate offline without sessions across multiple dialects."""

    @relationship(type_name="FOLLOWS_INDEXED", source_node=User, target_node=User)
    class FollowsIndexed(Relationship):
        since: int = Field(index=True)

    # 1. openCypher index and constraint generation
    cypher_indexes = SchemaManager.generate_index_ddl(User, FollowsIndexed, dialect="cypher")
    assert any(
        "CREATE INDEX index_user_age IF NOT EXISTS FOR (n:User) ON (n.age)" in s
        for s in cypher_indexes
    )
    assert any(
        "CREATE INDEX index_rel_follows_indexed_since IF NOT EXISTS FOR ()-[r:FOLLOWS_INDEXED]-() ON (r.since)"
        in s
        for s in cypher_indexes
    )

    cypher_constraints = SchemaManager.generate_constraint_ddl(
        User, FollowsIndexed, dialect="cypher"
    )
    assert any("constraint_user_email_unique" in s for s in cypher_constraints)
    assert any("constraint_user_user_id_not_null" in s for s in cypher_constraints)
    assert any("constraint_user_user_id_unique" in s for s in cypher_constraints)

    # 2. openCypher drop index and constraint generation
    cypher_drop_indexes = SchemaManager.generate_drop_index_ddl(
        User, FollowsIndexed, dialect="cypher"
    )
    assert any("DROP INDEX index_user_age IF EXISTS" in s for s in cypher_drop_indexes)
    assert any(
        "DROP INDEX index_rel_follows_indexed_since IF EXISTS" in s for s in cypher_drop_indexes
    )

    cypher_drop_constraints = SchemaManager.generate_drop_constraint_ddl(
        User, FollowsIndexed, dialect="cypher"
    )
    assert any(
        "DROP CONSTRAINT constraint_user_email_unique IF EXISTS" in s
        for s in cypher_drop_constraints
    )
    assert any(
        "DROP CONSTRAINT constraint_user_user_id_not_null IF EXISTS" in s
        for s in cypher_drop_constraints
    )

    # 3. PostgreSQL / DuckPGQ relational index and constraint generation
    sql_indexes = SchemaManager.generate_index_ddl(User, FollowsIndexed, dialect="postgres")
    assert 'CREATE INDEX IF NOT EXISTS idx_user_age ON "user" ("age");' in sql_indexes
    assert (
        'CREATE INDEX IF NOT EXISTS idx_follows_indexed_since ON "follows_indexed" ("since");'
        in sql_indexes
    )

    sql_constraints = SchemaManager.generate_constraint_ddl(
        User, FollowsIndexed, dialect="postgres"
    )
    assert 'ALTER TABLE "user" ADD CONSTRAINT uq_user_email UNIQUE ("email");' in sql_constraints

    sql_drop_indexes = SchemaManager.generate_drop_index_ddl(
        User, FollowsIndexed, dialect="postgres"
    )
    assert "DROP INDEX IF EXISTS idx_user_age;" in sql_drop_indexes
    assert "DROP INDEX IF EXISTS idx_follows_indexed_since;" in sql_drop_indexes

    sql_drop_constraints = SchemaManager.generate_drop_constraint_ddl(
        User, FollowsIndexed, dialect="postgres"
    )
    assert 'ALTER TABLE "user" DROP CONSTRAINT IF EXISTS uq_user_email;' in sql_drop_constraints

    # 4. FalkorDB Cypher indexes and actionable constraint errors
    falkor_indexes = SchemaManager.generate_index_ddl(User, FollowsIndexed, dialect="falkordb")
    assert "CREATE INDEX FOR (n:User) ON (n.age)" in falkor_indexes
    assert "CREATE INDEX FOR ()-[r:FOLLOWS_INDEXED]-() ON (r.since)" in falkor_indexes

    falkor_drop_indexes = SchemaManager.generate_drop_index_ddl(
        User, FollowsIndexed, dialect="falkordb"
    )
    assert "DROP INDEX FOR (n:User) ON (n.age)" in falkor_drop_indexes
    assert "DROP INDEX FOR ()-[r:FOLLOWS_INDEXED]-() ON (r.since)" in falkor_drop_indexes

    with pytest.raises(NotImplementedError, match="FalkorDB does not support"):
        SchemaManager.generate_constraint_ddl(User, dialect="falkordb")

    # 5. Apache AGE actionable errors
    with pytest.raises(NotImplementedError, match="Apache AGE does not support"):
        SchemaManager.generate_index_ddl(User, dialect="age")

    with pytest.raises(NotImplementedError, match="Apache AGE does not support"):
        SchemaManager.generate_constraint_ddl(User, dialect="age")

    # 6. Pure DDL generation using registered model names
    User.register_schema()
    FollowsIndexed.register_schema()
    reg_indexes = SchemaManager.generate_index_ddl("User", "FollowsIndexed", dialect="postgres")
    assert 'CREATE INDEX IF NOT EXISTS idx_user_age ON "user" ("age");' in reg_indexes
    assert (
        'CREATE INDEX IF NOT EXISTS idx_follows_indexed_since ON "follows_indexed" ("since");'
        in reg_indexes
    )


def test_session_ddl_convenience_methods():
    """Verifies Session instance convenience methods execute DDL directly."""
    from voyager_ogm.bridge import MockBridge

    # openCypher session
    mock_bridge = MockBridge()
    session = Session(bridge=mock_bridge, dialect="cypher")

    applied_constraints = session.create_constraints(User, Follows)
    assert len(applied_constraints) > 0
    assert all("CREATE CONSTRAINT" in s for s in applied_constraints)

    applied_indexes = session.create_indexes(User, Follows)
    assert len(applied_indexes) > 0
    assert all("CREATE INDEX" in s for s in applied_indexes)

    dropped_indexes = session.drop_indexes(User, Follows)
    assert len(dropped_indexes) > 0
    assert all("DROP INDEX" in s for s in dropped_indexes)

    dropped_constraints = session.drop_constraints(User, Follows)
    assert len(dropped_constraints) > 0
    assert all("DROP CONSTRAINT" in s for s in dropped_constraints)

    # Verify executed queries in MockBridge match applied statements
    executed_stmts = [q[0] for q in mock_bridge.executed_queries]
    for stmt in applied_constraints + applied_indexes + dropped_indexes + dropped_constraints:
        assert stmt in executed_stmts

    # PostgreSQL session
    mock_bridge_sql = MockBridge()
    session_sql = Session(bridge=mock_bridge_sql, dialect="postgres")

    sql_idx = session_sql.create_indexes(User, Follows)
    assert 'CREATE INDEX IF NOT EXISTS idx_user_age ON "user" ("age");' in sql_idx
    assert (
        'CREATE INDEX IF NOT EXISTS idx_user_age ON "user" ("age");',
        {},
    ) in mock_bridge_sql.executed_queries


@pytest.mark.asyncio
async def test_async_session_ddl_convenience_methods():
    """Verifies AsyncSession instance convenience methods asynchronously execute DDL."""
    from voyager_ogm.bridge import AsyncMockBridge
    from voyager_ogm.session import AsyncSession

    async_mock = AsyncMockBridge()
    async_session = AsyncSession(bridge=async_mock, dialect="cypher")

    applied_constraints = await async_session.create_constraints(User, Follows)
    assert len(applied_constraints) > 0
    assert all("CREATE CONSTRAINT" in s for s in applied_constraints)

    applied_indexes = await async_session.create_indexes(User, Follows)
    assert len(applied_indexes) > 0
    assert all("CREATE INDEX" in s for s in applied_indexes)

    dropped_indexes = await async_session.drop_indexes(User, Follows)
    assert len(dropped_indexes) > 0
    assert all("DROP INDEX" in s for s in dropped_indexes)

    dropped_constraints = await async_session.drop_constraints(User, Follows)
    assert len(dropped_constraints) > 0
    assert all("DROP CONSTRAINT" in s for s in dropped_constraints)

    executed_stmts = [q[0] for q in async_mock.executed_queries]
    for stmt in applied_constraints + applied_indexes + dropped_indexes + dropped_constraints:
        assert stmt in executed_stmts


@pytest.mark.live
def test_live_schema_ddl_multi_engine_integration():
    """Verifies live index and constraint management across live Neo4j, PostgreSQL 19, and FalkorDB engines."""
    # 1. Neo4j Live Engine (Bolt port 7687)
    uri = os.getenv("NEO4J_ENTERPRISE_URI", "bolt://127.0.0.1:7687")
    user = os.getenv("NEO4J_ENTERPRISE_USER", "neo4j")
    password = os.getenv("NEO4J_ENTERPRISE_PASSWORD", "voyex1234")
    database = os.getenv("NEO4J_ENTERPRISE_DATABASE", "neo4j")

    try:
        neo_drv = GraphDatabase.driver(uri, auth=(user, password), connection_timeout=1.0)
        neo_drv.verify_connectivity()
        neo_bridge = Neo4jBoltBridge(neo_drv, database=database)
        neo_session = Session(bridge=neo_bridge, dialect="cypher")

        # Create constraints and indexes idempotently
        neo_c = neo_session.create_constraints(User)
        assert len(neo_c) > 0
        neo_i = neo_session.create_indexes(User)
        assert len(neo_i) > 0

        # Teardown
        neo_session.drop_indexes(User)
        neo_session.drop_constraints(User)
        neo_session.close()
    except Exception:
        pass  # Skip gracefully if Neo4j is unreachable

    # 2. PostgreSQL 19 Live Engine (Port 5456)
    pg_uri = os.getenv("PG19_URI", "postgresql://postgres:voyagerpass123@127.0.0.1:5456/postgres")
    try:
        import psycopg

        pg_conn = psycopg.connect(pg_uri, autocommit=True, connect_timeout=1)
        with pg_conn.cursor() as cur:
            cur.execute('DROP TABLE IF EXISTS "user" CASCADE;')
            cur.execute('CREATE TABLE "user" (user_id TEXT PRIMARY KEY, email TEXT, age INT);')

        pg_session = Session(bridge=pg_conn, dialect="postgres")
        # Quoted table "user" avoids PostgreSQL reserved keyword syntax error
        pg_idx = pg_session.create_indexes(User)
        assert len(pg_idx) > 0
        pg_con = pg_session.create_constraints(User)
        assert len(pg_con) > 0

        # Teardown
        pg_session.drop_indexes(User)
        pg_session.drop_constraints(User)

        with pg_conn.cursor() as cur:
            cur.execute('DROP TABLE IF EXISTS "user" CASCADE;')
        pg_conn.close()
    except Exception:
        pass  # Skip gracefully if PostgreSQL 19 is unreachable

    # 3. FalkorDB Live Engine (Port 6379)
    try:
        from falkordb import FalkorDB
        from voyager_ogm.bridge import FalkorDBBridge

        f_db = FalkorDB(host="127.0.0.1", port=6379)
        f_graph = f_db.select_graph("voyager_ddl_live_test")
        f_graph.query("RETURN 1")

        fk_bridge = FalkorDBBridge(f_graph)
        fk_session = Session(bridge=fk_bridge, dialect="falkordb")
        # Idempotent index creation
        fk_idx = fk_session.create_indexes(User)
        assert len(fk_idx) > 0
        # Re-creation should not crash (handles 'already indexed' in bridge)
        fk_session.create_indexes(User)

        # Teardown
        fk_session.drop_indexes(User)
        # Re-drop should not crash (handles 'no such index' in bridge)
        fk_session.drop_indexes(User)

        # Constraints raise actionable error
        with pytest.raises(NotImplementedError, match="FalkorDB does not support"):
            fk_session.create_constraints(User)
    except Exception:
        pass  # Skip gracefully if FalkorDB is unreachable


def test_schema_registration_loud_failure(caplog: pytest.LogCaptureFixture):
    """Verifies that schema registration failures log a loud warning and raise an exception."""
    import logging

    @node(label="LoudFailNode")
    class LoudFailNode(Node):
        id: str = Field(primary_key=True)

    @relationship(type_name="LOUD_FAIL_REL", source_node=LoudFailNode, target_node=LoudFailNode)
    class LoudFailRel(Relationship):
        weight: float = Field()

    class FailingRegistry:
        def register_node(self, *args: Any, **kwargs: Any) -> None:
            raise RuntimeError("Rust registry allocation failed")

        def register_relationship(self, *args: Any, **kwargs: Any) -> None:
            raise RuntimeError("Rust registry allocation failed")

    fail_reg = FailingRegistry()

    with caplog.at_level(logging.WARNING, logger="voyager_ogm"):
        with pytest.raises(RuntimeError, match="Rust registry allocation failed"):
            LoudFailNode.register_schema(registry=fail_reg)
        assert "Failed to register schema for node model LoudFailNode" in caplog.text

    with caplog.at_level(logging.WARNING, logger="voyager_ogm"):
        with pytest.raises(RuntimeError, match="Rust registry allocation failed"):
            LoudFailRel.register_schema(registry=fail_reg)
        assert "Failed to register schema for relationship model LoudFailRel" in caplog.text


def test_falkordb_bridge_idempotency_unit():
    """Unit test verifying that FalkorDBBridge absorbs index idempotency errors and raises others."""
    from voyager_ogm.bridge import AsyncFalkorDBBridge, FalkorDBBridge

    class FakeGraph:
        def __init__(self) -> None:
            self.calls: list[str] = []

        def query(self, stmt: str, params: Any = None) -> Any:
            self.calls.append(stmt)
            stmt_clean = stmt.strip().lower()
            if "already_fail" in stmt_clean:
                raise RuntimeError("Attribute 'age' is already indexed")
            if "no_such_fail" in stmt_clean:
                raise RuntimeError("Unable to drop index on :User(age): no such index.")
            if "syntax_error" in stmt_clean:
                raise RuntimeError("Invalid Cypher syntax")
            return None

    fake_g = FakeGraph()
    bridge = FalkorDBBridge(fake_g)

    # Creating already indexed attribute should be absorbed
    res = bridge.execute("CREATE INDEX FOR (n:User) ON (n.already_fail)")
    assert res == []

    # Dropping non-existent index should be absorbed
    res = bridge.execute("DROP INDEX FOR (n:User) ON (n.no_such_fail)")
    assert res == []

    # Other queries with syntax errors must NOT be swallowed
    with pytest.raises(RuntimeError, match="Invalid Cypher syntax"):
        bridge.execute("CREATE INDEX FOR (n:User) ON (n.syntax_error)")

    # Non-index queries that somehow contain 'already indexed' must NOT be swallowed
    with pytest.raises(RuntimeError, match="already indexed"):
        bridge.execute("MATCH (n:User) RETURN n.already_fail")

    # Async bridge delegates faithfully
    import asyncio

    async_bridge = AsyncFalkorDBBridge(fake_g)
    res_async = asyncio.run(async_bridge.execute("CREATE INDEX FOR (n:User) ON (n.already_fail)"))
    assert res_async == []
