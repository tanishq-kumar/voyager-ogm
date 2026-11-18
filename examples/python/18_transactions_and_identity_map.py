"""Voyager OGM: In-Memory Unit of Work Transactions, Savepoints & The Identity Map.

Teaches:
    - Managing in-memory Unit of Work transactional boundaries with `session.transaction()`
    - Transaction states: ACTIVE, COMMITTED, ROLLED_BACK (in-memory AST & entity mutations)
    - Fine-grained recovery using nested savepoints (`tx.savepoint()`)
    - Opt-in Identity Map pattern (`Session(enable_identity_map=True)`)
    - In-memory entity deduplication and retrieval via `session.get()`
    - Vectorized batch flush (`session.flush()`) for dirty entities

Prerequisites:
    - Engine: None (offline / in-memory using MockBridge)
    - Extras: None

Run with:
    `uv run python examples/python/18_transactions_and_identity_map.py`
"""

from __future__ import annotations

from voyager_ogm import Field, MockBridge, Node, Session, node


@node(label="Customer")
class Customer(Node):
    id: str = Field(primary_key=True)
    name: str = Field()
    balance: float = Field(default=0.0)
    tier: str = Field(default="Standard")


def main() -> None:
    print("=== Voyager OGM: Transactions & Identity Map ===\n")

    mock_bridge = MockBridge()
    session = Session(bridge=mock_bridge, dialect="cypher", enable_identity_map=True)

    # -------------------------------------------------------------------------
    # 1. Transaction Lifecycle: Auto-Commit & Rollback
    # -------------------------------------------------------------------------
    print("--- [1] Clean Transaction Auto-Commit ---")
    with session.transaction() as tx:
        print(f"Transaction ID: {tx.id}, State: {tx.state}, Active: {tx.is_active}")
        # Perform transactional operations...
    print(f"After clean block exit -> State: {tx.state}, Active: {tx.is_active}\n")

    print("--- [2] Transaction Auto-Rollback on Exception ---")
    try:
        with session.transaction() as tx_fail:
            print(f"Transaction ID: {tx_fail.id}, State: {tx_fail.state}")
            raise RuntimeError("Simulated transient network failure during processing")
    except RuntimeError as err:
        print(f"Caught expected error: {err}")
    print(f"After exception -> State: {tx_fail.state}, Active: {tx_fail.is_active}\n")

    # -------------------------------------------------------------------------
    # 2. Nested Savepoint Support
    # -------------------------------------------------------------------------
    print("--- [3] Two-Layer Savepoint Rollback ---")
    with session.transaction() as tx_main:
        print(f"Main transaction {tx_main.id} started.")

        # Isolated sub-operation protected by a savepoint
        try:
            with tx_main.savepoint("sub_payment_checkpoint"):
                print("  Inside savepoint 'sub_payment_checkpoint'")
                raise ValueError("Payment provider rejected card")
        except ValueError as err:
            print(f"  Handled sub-operation exception: {err}")
            print(f"  Savepoint rolled back, but main tx remains: {tx_main.state}")

        # Main transaction proceeds successfully
        assert tx_main.is_active
    print(f"Main transaction committed successfully: {tx_main.state}\n")

    # -------------------------------------------------------------------------
    # 3. Identity Map Registration & In-Memory Deduplication
    # -------------------------------------------------------------------------
    print("--- [4] Identity Map & Entity Deduplication ---")
    alice = Customer(id="c_101", name="Alice", balance=1500.0, tier="Gold")
    session.register(alice)

    # Subsequent get() returns the exact same in-memory Python object
    cached_alice = session.get(Customer, "c_101")
    assert cached_alice is alice
    print(
        f"Retrieved Customer from Identity Map: {cached_alice.get('name')} (id: {cached_alice.get('id')})"
    )
    print(
        f"In-memory object identity confirmed: (alice is cached_alice) == {alice is cached_alice}\n"
    )

    # -------------------------------------------------------------------------
    # 4. Dirty Property Tracking & Vectorized Batch Flush
    # -------------------------------------------------------------------------
    print("--- [5] Vectorized Batch Flush ---")
    bob = Customer(id="c_102", name="Bob", balance=500.0, tier="Silver")
    session.register(bob)

    # Mutate tracked instances in memory
    alice.balance = 2000.0
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

    print("\n[SUCCESS] Transactions & Identity Map demonstration completed successfully.")


if __name__ == "__main__":
    main()
