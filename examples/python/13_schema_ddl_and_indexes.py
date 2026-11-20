"""Voyager OGM: Declarative Schema, Constraints & Index DDL.

Teaches:
    - Declaring primary keys, unique constraints, and secondary indexes on graph models
    - Multi-dialect DDL generation via `SchemaManager.generate_ddl()`
    - Dialect targets: Neo4j, Memgraph, FalkorDB, Apache AGE, DuckDB, PostgreSQL
    - Drop DDL generation via `SchemaManager.generate_drop_ddl()`
    - Executing DDL offline via `MockBridge` or real engines
    - Recording field and model descriptions as metadata that appears in the
      schema snapshot but never changes DDL or compiled queries

Prerequisites:
    - Engine: None (runs offline / compile-only via MockBridge; optional DuckDB/Postgres if executed live)
    - Extras: None (or `duckdb` / `psycopg` if executing against live engines)

Run with:
    `uv run python examples/python/13_schema_ddl_and_indexes.py`
"""

from __future__ import annotations

import json

from voyager_ogm import (
    Field,
    NativeSchemaRegistry,
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

    __description__ = "A registered account holder in the social network."

    user_id: str = Field(primary_key=True, description="Stable account identifier.")
    email: str = Field(unique=True, description="Primary contact email address.")
    age: int = Field(index=True, description="Age in years.")
    bio: str = Field(index=True, description="Free-text profile biography.")
    city: str = Field(default="London", index=True)
    is_active: bool = True


@relationship(type_name="FOLLOWS", source_node=User, target_node=User)
class Follows(Relationship):
    """FOLLOWS edge entity with indexed temporal properties."""

    __description__ = "A directed follow relationship between two users."

    since: int = Field(index=True, description="Epoch seconds when the follow began.")
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

    session = Session(bridge=MockBridge())

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

    # -----------------------------------------------------------------------
    # Part 8: Description Metadata (schema snapshot only - no DDL change)
    # -----------------------------------------------------------------------
    print("[8] Description Metadata (schema snapshot only):")
    print("-" * 50)
    print("  Field(description=...) and a model-level __description__ are metadata:")
    print("  they appear in the registry snapshot JSON but do not alter DDL or queries.")

    registry = NativeSchemaRegistry()
    User.register_schema(registry)
    Follows.register_schema(registry)
    snapshot = json.loads(registry.to_json())

    for kind in ("nodes", "relationships"):
        for identity, schema in snapshot[kind].items():
            print(f"  {kind[:-1]} {identity}: description={schema.get('description')!r}")
            for field_name, field_schema in schema["fields"].items():
                present = "description" in field_schema
                value = field_schema.get("description")
                print(f"    field {field_name}: description={value!r} (key present: {present})")

    # A model with the same labels and fields, but no descriptions.
    class PlainUser(Node):
        __labels__ = ["User"]

        user_id: str = Field(primary_key=True)
        email: str = Field(unique=True)
        age: int = Field(index=True)
        bio: str = Field(index=True)
        city: str = Field(default="London", index=True)
        is_active: bool = True

    plain_registry = NativeSchemaRegistry()
    PlainUser.register_schema(plain_registry)
    described_ddl = registry.generate_cypher_ddl(model_name="User")
    plain_ddl = plain_registry.generate_cypher_ddl(model_name="PlainUser")
    print(f"  DDL identical to the same model without descriptions: {described_ddl == plain_ddl}")

    # A subclass only carries a description when it declares its own.
    class AuditUser(User):
        """Subclass of User that declares no __description__ of its own."""

        __labels__ = ["AuditUser"]

    audit_registry = NativeSchemaRegistry()
    AuditUser.register_schema(audit_registry)
    audit_schema = audit_registry.get_node("AuditUser") or {}
    print(f"  Subclass AuditUser (no __description__): {audit_schema.get('description')!r}")

    class AdminUser(User):
        """Subclass of User that declares its own description."""

        __description__ = "A privileged account holder."
        __labels__ = ["AdminUser"]

    admin_registry = NativeSchemaRegistry()
    AdminUser.register_schema(admin_registry)
    admin_schema = admin_registry.get_node("AdminUser") or {}
    print(f"  Subclass AdminUser (own __description__): {admin_schema.get('description')!r}")

    print()
    print("  Snapshot JSON (NativeSchemaRegistry.to_json()) for the User node:")
    for line in json.dumps(snapshot["nodes"]["User"], indent=2).splitlines():
        print(f"    {line}")
    print()

    print("=" * 76)
    print(" Demonstration completed successfully!")
    print("=" * 76)


if __name__ == "__main__":
    main()
