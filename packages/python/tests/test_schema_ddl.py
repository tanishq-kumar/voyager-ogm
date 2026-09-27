"""Voyager OGM Schema & Graph Types DDL Test Suite.

Verifies:
1. Automated openCypher / Neo4j constraint generation (UNIQUE, NOT NULL, Indexes)
2. Neo4j 5.x Graph Types (Property Type constraints: REQUIRE n.prop :: STRING)
3. Automated DROP DDL generation
4. Live schema constraint creation and validation against running Neo4j container
"""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    Node,
    Relationship,
    SchemaManager,
    Session,
    node,
    relationship,
)


@node(label="User")
class User(Node):
    """User entity with constraints and Graph Types."""

    user_id: str = Field(primary_key=True)
    email: str = Field(unique=True)
    age: int = Field(index=True)
    bio: str = Field()


@relationship(type_name="FOLLOWS")
class Follows(Relationship):
    """FOLLOWS edge with property constraint."""

    since: int = Field(unique=True)


def test_schema_ddl_generation():
    """Verifies generated DDL statements match Neo4j 5.x Graph Types & Constraint standards."""
    statements = SchemaManager.generate_cypher_ddl(User, include_type_constraints=True)

    # Unique / Primary Key constraints
    assert (
        "CREATE CONSTRAINT constraint_user_user_id_unique IF NOT EXISTS FOR (n:User) REQUIRE n.user_id IS UNIQUE"
        in statements
    )
    assert (
        "CREATE CONSTRAINT constraint_user_email_unique IF NOT EXISTS FOR (n:User) REQUIRE n.email IS UNIQUE"
        in statements
    )

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
    assert "DROP CONSTRAINT constraint_user_email_unique IF EXISTS" in drop_statements
    assert "DROP CONSTRAINT constraint_user_age_type IF EXISTS" in drop_statements
    assert "DROP INDEX index_user_age IF EXISTS" in drop_statements


@pytest.mark.live
def test_live_neo4j_schema_ddl_creation():
    """Verifies applying constraints and indexes live to the running Neo4j database."""
    try:
        from neo4j import GraphDatabase

        driver = GraphDatabase.driver(
            "bolt://127.0.0.1:7687", auth=("neo4j", "voyagerpass123"), connection_timeout=0.2
        )
        driver.verify_connectivity()
    except Exception:
        pytest.skip("Neo4j database not reachable on bolt://127.0.0.1:7687")

    session = Session(bridge=driver, dialect="cypher")

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
    assert "    NODE User (age INTEGER, bio STRING, email STRING, user_id STRING)" in gql_ddl
    assert "    EDGE FOLLOWS (since INTEGER)" in gql_ddl

    drop_ddl = SchemaManager.generate_gql_drop_graph_type_ddl("SocialGraphType")
    assert drop_ddl == "DROP GRAPH TYPE SocialGraphType IF EXISTS"


def test_pgq_property_graph_ddl():
    """Verifies standard SQL:2023 PGQ / DuckPGQ CREATE PROPERTY GRAPH statements."""
    pgq_ddl = SchemaManager.generate_pgq_ddl("social_pgq", User, Follows)
    assert "CREATE PROPERTY GRAPH social_pgq" in pgq_ddl
    assert "VERTEX TABLES (" in pgq_ddl
    assert "user KEY (user_id) LABEL User" in pgq_ddl
    assert "EDGE TABLES (" in pgq_ddl
    assert "LABEL FOLLOWS" in pgq_ddl

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
    assert any("CREATE INDEX index_article_title" in s for s in cypher_stmts)

    # 2. ISO GQL DDL
    gql = reg.generate_gql_graph_type_ddl("ArticleGraph")
    assert "CREATE GRAPH TYPE ArticleGraph AS {" in gql
    assert "NODE Article (id STRING, title STRING)" in gql
    assert "EDGE CITED_BY (year INTEGER)" in gql

    # 3. SQL:2023 PGQ DDL
    pgq = reg.generate_pgq_ddl("article_pgq")
    assert "CREATE PROPERTY GRAPH article_pgq" in pgq
    assert "article KEY (id) LABEL Article" in pgq
    assert "LABEL CITED_BY" in pgq
