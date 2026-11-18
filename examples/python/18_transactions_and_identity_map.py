"""Voyager OGM: Real Transactions, In-Memory Unit of Work & The Identity Map.

Teaches:
    - Real database transactions with `session.transaction()` (BEGIN/COMMIT/ROLLBACK)
      on any supported engine, including embedded DuckDB/DuckPGQ and Relate
    - In-memory Unit of Work boundaries with `session.unit_of_work()`
    - Unit of Work states: ACTIVE, COMMITTED, ROLLED_BACK (in-memory entity state)
    - Rollback restoring identity-mapped entity values to the begin snapshot
    - Fine-grained recovery using nested savepoints (`uow.savepoint()`)
    - Opt-in Identity Map pattern (`Session(enable_identity_map=True)`)
    - In-memory entity deduplication and retrieval via `session.get()`
    - Vectorized batch flush (`session.flush()`) for dirty entities

Prerequisites:
    - Engine: None (offline / in-memory using MockBridge)
    - Live transaction section: Neo4j on 127.0.0.1:7687 (skipped when unreachable)
    - Extras: `neo4j` for the live section

Run with:
    `uv run python examples/python/18_transactions_and_identity_map.py`
"""

from __future__ import annotations

import os
import socket

from voyager_ogm import Field, MockBridge, Node, Session, node


@node(label="Customer")
class Customer(Node):
    id: str = Field(primary_key=True)
    name: str = Field()
    balance: float = Field(default=0.0)
    tier: str = Field(default="Standard")


def _port_open(host: str, port: int, timeout: float = 0.5) -> bool:
    """Returns True when a TCP port accepts connections."""
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def run_live_transaction() -> None:
    """Demonstrates a real database transaction against Neo4j when available.

    Skips silently when Neo4j is unreachable or the neo4j package is missing.
    """
    host = os.getenv("NEO4J_HOST", "127.0.0.1")
    port = int(os.getenv("NEO4J_PORT", "7687"))
    if not _port_open(host, port):
        print("--- [7] Real Database Transaction (skipped: no Neo4j on port 7687) ---\n")
        return
    try:
        from neo4j import GraphDatabase
    except ImportError:
        print("--- [7] Real Database Transaction (skipped: neo4j package not installed) ---\n")
        return

    uri = os.getenv("NEO4J_URI", f"bolt://{host}:{port}")
    password = os.getenv("NEO4J_PASSWORD", "voyagerpass123")
    driver = GraphDatabase.driver(uri, auth=("neo4j", password))
    session = Session(bridge=driver, dialect="cypher")
    try:
        driver.verify_connectivity()
        session.execute("MATCH (n:TxDemo) DETACH DELETE n")

        print("--- [7] Real Database Transaction: Rollback ---")
        try:
            with session.transaction() as tx:
                print(f"Transaction state inside block: {tx.state}")
                session.execute("CREATE (:TxDemo {name: 'RolledBack'})")
                raise RuntimeError("Simulated failure after the write")
        except RuntimeError as err:
            print(f"Caught expected error: {err}")
            print(f"Transaction state after rollback: {tx.state}")

        count = session.execute("MATCH (n:TxDemo) RETURN count(n) AS count")[0]["count"]
        print(f"Rows after rollback: {count}")
        assert count == 0

        print("\n--- [8] Real Database Transaction: Commit ---")
        with session.transaction() as tx:
            session.execute("CREATE (:TxDemo {name: 'Committed'})")
        count = session.execute("MATCH (n:TxDemo) RETURN count(n) AS count")[0]["count"]
        print(f"Transaction state after commit: {tx.state}")
        print(f"Rows after commit: {count}")
        assert count == 1
        session.execute("MATCH (n:TxDemo) DETACH DELETE n")
        print("Live transaction demonstration completed.\n")
    finally:
        session.close()


def main() -> None:
    print("=== Voyager OGM: Unit of Work & Identity Map ===\n")

    mock_bridge = MockBridge()
    session = Session(bridge=mock_bridge, dialect="cypher", enable_identity_map=True)

    # -------------------------------------------------------------------------
    # 1. Unit of Work Lifecycle: Auto-Commit & Rollback
    # -------------------------------------------------------------------------
    print("--- [1] Clean Unit of Work Auto-Commit ---")
    with session.unit_of_work() as uow:
        print(f"Unit of Work ID: {uow.id}, State: {uow.state}, Active: {uow.is_active}")
        # Perform transactional operations...
    print(f"After clean block exit -> State: {uow.state}, Active: {uow.is_active}\n")

    print("--- [2] Unit of Work Auto-Rollback on Exception ---")
    try:
        with session.unit_of_work() as uow_fail:
            print(f"Unit of Work ID: {uow_fail.id}, State: {uow_fail.state}")
            raise RuntimeError("Simulated transient network failure during processing")
    except RuntimeError as err:
        print(f"Caught expected error: {err}")
    print(f"After exception -> State: {uow_fail.state}, Active: {uow_fail.is_active}\n")

    # -------------------------------------------------------------------------
    # 2. Identity Map Registration & In-Memory Deduplication
    # -------------------------------------------------------------------------
    print("--- [3] Identity Map & Entity Deduplication ---")
    alice = Customer(id="c_101", name="Alice", balance=1500.0, tier="Gold")
    session.register(alice, is_clean=True)

    # Subsequent get() returns the exact same in-memory Python object
    cached_alice = session.get(Customer, "c_101")
    assert cached_alice is alice
    print(
        f"Retrieved Customer from Identity Map: {cached_alice.get('name')} "
        f"(id: {cached_alice.get('id')})"
    )
    print(
        f"In-memory object identity confirmed: (alice is cached_alice) == {alice is cached_alice}\n"
    )

    # -------------------------------------------------------------------------
    # 3. Rollback Restores In-Memory Entity State
    # -------------------------------------------------------------------------
    print("--- [4] Rollback Restores Entity Values ---")
    print(f"Before unit of work: balance={alice.get('balance')}, tier={alice.get('tier')}")
    try:
        with session.unit_of_work():
            alice.balance = 9999.0
            alice.tier = "Platinum"
            print(f"Inside unit of work: balance={alice.get('balance')}, tier={alice.get('tier')}")
            raise RuntimeError("Simulated failure after mutating Alice")
    except RuntimeError as err:
        print(f"Caught expected error: {err}")

    print(f"After rollback:      balance={alice.get('balance')}, tier={alice.get('tier')}")
    assert alice.get("balance") == 1500.0
    assert alice.get("tier") == "Gold"
    assert alice.dirty_fields == {}
    print("Entity values were restored to the begin snapshot.\n")

    # -------------------------------------------------------------------------
    # 4. Nested Savepoint Support
    # -------------------------------------------------------------------------
    print("--- [5] Two-Layer Savepoint Rollback ---")
    with session.unit_of_work() as uow_main:
        print(f"Main unit of work {uow_main.id} started.")
        alice.balance = 2000.0

        # Isolated sub-operation protected by a savepoint
        try:
            with uow_main.savepoint("sub_payment_checkpoint"):
                print("  Inside savepoint 'sub_payment_checkpoint'")
                alice.balance = 2500.0
                raise ValueError("Payment provider rejected card")
        except ValueError as err:
            print(f"  Handled sub-operation exception: {err}")
            print(f"  Savepoint rolled back, but main uow remains: {uow_main.state}")

        # Savepoint rollback restores the balance captured at savepoint creation
        assert alice.get("balance") == 2000.0
        assert uow_main.is_active
    print(f"Main unit of work committed successfully: {uow_main.state}")
    print(f"Pending dirty state kept for flush: {alice.dirty_fields}\n")

    # -------------------------------------------------------------------------
    # 5. Dirty Property Tracking & Vectorized Batch Flush
    # -------------------------------------------------------------------------
    print("--- [6] Vectorized Batch Flush ---")
    bob = Customer(id="c_102", name="Bob", balance=500.0, tier="Silver")
    session.register(bob, is_clean=True)

    # Mutate tracked instances in memory
    bob.tier = "Gold"

    print(f"Alice dirty properties: {alice.dirty_fields}")
    print(f"Bob dirty properties:   {bob.dirty_fields}")

    # session.flush() compiles all dirty updates into vectorized UNWIND statements per model
    flush_results = session.flush()
    print(f"Flush generated {len(flush_results)} vectorized batch execution result(s):")
    for res in flush_results:
        print(f"  Statement: {res.statement} (records: {res.total_records})")

    # After flush, dirty fields are cleared
    assert alice.dirty_fields == {}
    assert bob.dirty_fields == {}
    print("\nDirty state cleared post-flush.")

    # -------------------------------------------------------------------------
    # 6. Real Database Transactions (live, skipped when no engine is reachable)
    # -------------------------------------------------------------------------
    run_live_transaction()

    print("\n[SUCCESS] Unit of Work & Identity Map demonstration completed successfully.")


if __name__ == "__main__":
    main()
