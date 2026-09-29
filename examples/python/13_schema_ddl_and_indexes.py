"""Voyager OGM Example 13: Declarative Schema, Constraints & Index Management.

Demonstrates how Voyager OGM manages constraints and indexes across all supported
graph and relational engines (Neo4j, Memgraph, FalkorDB, Apache AGE, DuckPGQ, Postgres 19+).

Run with:
    uv run python examples/python/13_schema_ddl_and_indexes.py
"""

from __future__ import annotations

from voyager_ogm import (
    Field,
    Node,
    Relationship,
    SchemaManager,
    Session,
    node,
    relationship,
)

# ---------------------------------------------------------------------------
# 1. Declarative Graph Entity Definitions with Constraints & Indexes
# ---------------------------------------------------------------------------


@node(label="User")
class User(Node):
    """User node entity showcasing primary keys, uniqueness, and secondary indexes."""

    user_id: str = Field(primary_key=True)
    email: str = Field(unique=True)
    age: int = Field(index=True)
    bio: str = Field(index=True)
    city: str = Field(default="London", index=True)
    is_active: bool = True


@relationship(type_name="FOLLOWS", source_node=User, target_node=User)
class Follows(Relationship):
    """FOLLOWS edge entity with indexed temporal properties."""

    since: int = Field(index=True)
    weight: float = 1.0


# ---------------------------------------------------------------------------
# 2. Main Demonstration
# ---------------------------------------------------------------------------


def main() -> None:
    print("=" * 76)
    print(" Voyager OGM: Declarative Schema, Constraints & Index Management")
    print("=" * 76)
    print()

    # -----------------------------------------------------------------------
    # Part 1: openCypher (Neo4j 5.x / Memgraph / FalkorDB)
    # -----------------------------------------------------------------------
    print("[1] openCypher DDL (Neo4j 5.x / Memgraph):")
    print("-" * 50)
    for model in (User, Follows):
        cypher_ddl = SchemaManager.generate_cypher_ddl(model, include_type_constraints=True)
        for stmt in cypher_ddl:
            print(f"  -> {stmt}")
    print()

    # -----------------------------------------------------------------------
    # Part 2: Neo4j Cypher 25 Declarative Graph Types (ALTER CURRENT GRAPH TYPE SET)
    # -----------------------------------------------------------------------
    print("[2] Neo4j Cypher 25 Graph Types DDL (Single Contract):")
    print("-" * 50)
    cypher25_ddl = SchemaManager.generate_cypher25_graph_type_ddl(User, Follows)
    print(cypher25_ddl)
    print()

    # -----------------------------------------------------------------------
    # Part 3: ISO/IEC 39075 GQL Standard Graph Types
    # -----------------------------------------------------------------------
    print("[3] ISO GQL Standard Graph Type DDL:")
    print("-" * 50)
    gql_ddl = SchemaManager.generate_gql_graph_type_ddl("SocialNetworkType", User, Follows)
    print(gql_ddl)
    print()

    # -----------------------------------------------------------------------
    # Part 4: SQL:2023 PGQ (PostgreSQL 19+ / DuckDB DuckPGQ)
    # -----------------------------------------------------------------------
    print("[4] SQL:2023 PGQ Property Graph Catalog DDL:")
    print("-" * 50)
    pgq_ddl = SchemaManager.generate_pgq_ddl("social_graph", User, Follows)
    print(pgq_ddl)
    print()

    # -----------------------------------------------------------------------
    # Part 5: Pure DDL Generation (Option 1 - Zero-Session Offline Migrations & CI)
    # -----------------------------------------------------------------------
    print("[5] Pure DDL Generation (No Session required - for Alembic/Migrations/CI):")
    print("-" * 50)
    print("  openCypher Indexes:")
    for stmt in SchemaManager.generate_index_ddl(User, Follows, dialect="cypher"):
        print(f"    -> {stmt}")

    print("  PostgreSQL / DuckPGQ Relational Indexes:")
    for stmt in SchemaManager.generate_index_ddl(User, Follows, dialect="postgres"):
        print(f"    -> {stmt}")

    print("  openCypher Constraints:")
    for stmt in SchemaManager.generate_constraint_ddl(User, Follows, dialect="cypher"):
        print(f"    -> {stmt}")
    print()

    # -----------------------------------------------------------------------
    # Part 6: Session Convenience Execution (Option 2 - Direct session.* methods)
    # -----------------------------------------------------------------------
    print(
        "[6] Session Convenience Execution Lifecycle (session.create_indexes / create_constraints):"
    )
    print("-" * 50)
    from voyager_ogm.bridge import MockBridge

    session = Session(MockBridge())

    print("  1. session.create_constraints(User, Follows):")
    executed_constraints = session.create_constraints(User, Follows)
    for stmt in executed_constraints:
        print(f"    [EXECUTED] {stmt}")

    print("  2. session.create_indexes(User, Follows):")
    executed_indexes = session.create_indexes(User, Follows)
    for stmt in executed_indexes:
        print(f"    [EXECUTED] {stmt}")

    print("  3. session.drop_indexes(User, Follows):")
    dropped_indexes = session.drop_indexes(User, Follows)
    for stmt in dropped_indexes:
        print(f"    [DROPPED]  {stmt}")

    print("  4. session.drop_constraints(User, Follows):")
    dropped_constraints = session.drop_constraints(User, Follows)
    for stmt in dropped_constraints:
        print(f"    [DROPPED]  {stmt}")
    print()

    # -----------------------------------------------------------------------
    # Part 7: Backwards-Compatible SchemaManager Delegation
    # -----------------------------------------------------------------------
    print(
        "[7] Backwards-Compatible SchemaManager Delegation (SchemaManager.create_indexes(session, ...)):"
    )
    print("-" * 50)
    delegated = SchemaManager.create_indexes(session, User, Follows)
    print(f"  Successfully applied {len(delegated)} indexes via SchemaManager delegation.")
    print()

    print("=" * 76)
    print(" Demonstration completed successfully!")
    print("=" * 76)


if __name__ == "__main__":
    main()
