"""Voyager OGM: Mutation DSL (CREATE, MERGE, SET, DELETE, REMOVE).

Teaches:
    - Declarative graph mutations using fluent Query builder methods
    - Creating nodes and relationship patterns with `Query.create()`
    - Upsert patterns with `Query.merge()`, `on_create_set()`, and `on_match_set()`
    - Field defaults with `default` / `default_factory`, written only on create
    - Reading in-memory values with `.get()` because attributes build query predicates
    - In-place property mutation with `set()` and `set_()`
    - Safe deletion with `delete()` and cascade removal with `detach_delete()`
    - Property unsetting with `remove()`
    - Model dirty tracking integration with `node.dirty_fields`

Prerequisites:
    - Engine: None (offline / compile-only)
    - Extras: None

Run with:
    `uv run python examples/python/07_mutation_dsl.py`
"""

from __future__ import annotations

from voyager_ogm import Field, Node, Query, Relationship, node, relationship


@node(label="Account")
class Account(Node):
    acc_no: str = Field(primary_key=True)
    holder: str = Field()
    balance: float = Field(default=0.0)
    status: str = Field(default="PENDING")
    tags: list[str] = Field(default_factory=list)


@relationship(type_name="TRANSFERRED_TO")
class TransferredTo(Relationship):
    amount: float = Field()
    timestamp: str = Field(default="2026-01-01T00:00:00Z")


def main() -> None:
    print("=== Voyager OGM: Mutation DSL (CREATE, MERGE, SET, DELETE) ===\n")

    # -------------------------------------------------------------------------
    # 1. CREATE: Single Node & Path Patterns
    # -------------------------------------------------------------------------
    src = Account(alias="src", acc_no="ACC-100", holder="Alice", balance=5000.0)
    dst = Account(alias="dst", acc_no="ACC-200", holder="Bob", balance=1200.0)
    tx = TransferredTo(alias="tx", amount=750.0)

    # Single node creation: every set field becomes a parameterised pattern property
    q_create_node = Query.create(src)
    compiled_create = q_create_node.compile("cypher")
    print("[1] CREATE Single Node (openCypher):")
    print(f"    {compiled_create.statement}\n")

    # Relationship path creation: (src)-[tx]->(dst)
    q_create_path = Query.create(src).to(tx).node(dst)
    compiled_path = q_create_path.compile("cypher")
    print("[2] CREATE Relationship Path (openCypher):")
    print(f"    {compiled_path.statement}\n")

    # -------------------------------------------------------------------------
    # 2. MERGE: Upsert with ON CREATE and ON MATCH
    # -------------------------------------------------------------------------
    # Query.merge(instance) keys the MERGE pattern on the primary key (acc_no).
    # Fields filled from a declared default are written only in ON CREATE SET, so an
    # existing node keeps its stored value; explicitly supplied fields are written in
    # both ON CREATE SET and ON MATCH SET. Chained .on_create_set(...) /
    # .on_match_set(...) calls are appended after them.
    user = Account(alias="a", acc_no="ACC-300", holder="Charlie")
    # Account attributes are query-DSL fields; read in-memory values with .get().
    print(
        f"    Defaults (read with .get): balance={user.get('balance')}, "
        f"status={user.get('status')}, tags={user.get('tags')}"
    )
    q_merge = Query.merge(user).on_match_set(user.status == "ACTIVE")
    compiled_merge = q_merge.compile("cypher")
    print("[3] MERGE Upsert Pattern (openCypher):")
    print(f"    Statement:  {compiled_merge.statement}")
    print(f"    Parameters: {compiled_merge.parameters}\n")

    # -------------------------------------------------------------------------
    # 3. SET: In-Place Property Updates
    # -------------------------------------------------------------------------
    target = Account(alias="tgt")
    q_set = (
        Query.match(target)
        .where(target.acc_no == "ACC-100")
        .set(target.balance == 5750.0, target.status == "VERIFIED")
        .return_(target.acc_no, target.balance, target.status)
    )
    compiled_set = q_set.compile("cypher")
    print("[4] MATCH ... SET In-Place Updates:")
    print(f"    Statement:  {compiled_set.statement}")
    print(f"    Parameters: {compiled_set.parameters}\n")

    # -------------------------------------------------------------------------
    # 4. Model Dirty Tracking & Automatic SET Generation
    # -------------------------------------------------------------------------
    acc_tracked = Account(alias="t", acc_no="ACC-999", holder="Eve", balance=100.0)
    acc_tracked.clear_dirty()
    assert acc_tracked.dirty_fields == {}

    # Mutate in memory
    acc_tracked.balance = 250.0
    acc_tracked.status = "FLAGGED"
    print("[5] Model Dirty Tracking:")
    print(f"    Dirty properties: {acc_tracked.dirty_fields}")

    q_dirty = Query.match(acc_tracked).where(acc_tracked.acc_no == "ACC-999").set(acc_tracked)
    compiled_dirty = q_dirty.compile("cypher")
    print(f"    Generated Statement: {compiled_dirty.statement}\n")

    # -------------------------------------------------------------------------
    # 5. DELETE & DETACH DELETE
    # -------------------------------------------------------------------------
    banned = Account(alias="b")
    q_detach = Query.match(banned).where(banned.status == "BANNED").detach_delete(banned)
    compiled_detach = q_detach.compile("cypher")
    print("[6] DETACH DELETE (Cascade Removal):")
    print(f"    {compiled_detach.statement}\n")

    # -------------------------------------------------------------------------
    # 6. Multi-Dialect ISO GQL Compilation
    # -------------------------------------------------------------------------
    gql_insert = Query.create(src).compile("iso_gql")
    print("[7] ISO/IEC 39075:2024 GQL INSERT:")
    print(f"    {gql_insert.statement}\n")

    print("[SUCCESS] Mutation DSL demonstration completed successfully.")


if __name__ == "__main__":
    main()
