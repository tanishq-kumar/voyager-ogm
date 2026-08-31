"""Tests for Two-Layer Rollback Transaction and Savepoint Context Managers in Python."""

from __future__ import annotations

import pytest
from voyager_ogm import Transaction


def test_transaction_clean_commit_lifecycle():
    with Transaction() as tx:
        assert tx.is_active
        assert tx.state == "ACTIVE"
        assert tx.id > 0

    # Clean exit should auto-commit
    assert tx.state == "COMMITTED"
    assert not tx.is_active


def test_transaction_exception_auto_rollback():
    tx_ref = None

    with pytest.raises(RuntimeError, match="Simulated database failure"):
        with Transaction() as tx:
            tx_ref = tx
            assert tx.is_active
            raise RuntimeError("Simulated database failure")

    assert tx_ref is not None
    assert tx_ref.state == "ROLLED_BACK"
    assert not tx_ref.is_active


def test_transaction_savepoint_context_manager():
    with Transaction() as tx:
        # Step 1: Initial state
        assert tx.is_active

        # Step 2: Nested savepoint with exception isolated
        with pytest.raises(ValueError, match="Sub-operation error"):
            with tx.savepoint("sp_isolated"):
                raise ValueError("Sub-operation error")

        # Parent transaction remains active after savepoint rollback
        assert tx.is_active
        assert tx.state == "ACTIVE"

    # Parent transaction commits successfully
    assert tx.state == "COMMITTED"


def test_explicit_transaction_savepoint_rollback():
    tx = Transaction(42)
    assert tx.id == 42
    assert tx.is_active

    tx.create_savepoint("checkpoint_a")
    tx.create_savepoint("checkpoint_b")

    # Rollback to checkpoint_a
    tx.rollback_to_savepoint("checkpoint_a")
    assert tx.is_active

    tx.commit()
    assert tx.state == "COMMITTED"


def test_invalid_operations_on_completed_transaction():
    tx = Transaction()
    tx.commit()

    with pytest.raises(ValueError, match="Cannot commit inactive transaction"):
        tx.commit()

    with pytest.raises(ValueError, match="Cannot rollback inactive transaction"):
        tx.rollback()

    with pytest.raises(ValueError, match="Cannot create savepoint on inactive transaction"):
        tx.savepoint("sp_fail")
