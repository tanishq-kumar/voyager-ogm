"""Tests for the in-memory Unit of Work, savepoints, and session entity rollback."""

from __future__ import annotations

import pytest
from voyager_ogm import (
    AsyncMockBridge,
    AsyncSession,
    Field,
    MockBridge,
    Node,
    Session,
    UnitOfWork,
)


class UowPerson(Node):
    """Test node model tracked by the session identity map."""

    id: str = Field(primary_key=True)
    name: str = Field()
    balance: float = Field(default=0.0)


def _make_session() -> tuple[Session, MockBridge, UowPerson]:
    """Builds an identity-map session with one clean registered entity."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)
    person = UowPerson(id="u1", name="Alice", balance=100.0)
    session.register(person, is_clean=True)
    return session, bridge, person


def test_unit_of_work_clean_commit_lifecycle():
    with UnitOfWork() as uow:
        assert uow.is_active
        assert uow.state == "ACTIVE"
        assert uow.id > 0

    # Clean exit should auto-commit
    assert uow.state == "COMMITTED"
    assert not uow.is_active


def test_unit_of_work_exception_auto_rollback():
    uow_ref = None

    with pytest.raises(RuntimeError, match="Simulated database failure"):
        with UnitOfWork() as uow:
            uow_ref = uow
            assert uow.is_active
            raise RuntimeError("Simulated database failure")

    assert uow_ref is not None
    assert uow_ref.state == "ROLLED_BACK"
    assert not uow_ref.is_active


def test_unit_of_work_savepoint_context_manager():
    with UnitOfWork() as uow:
        # Step 1: Initial state
        assert uow.is_active

        # Step 2: Nested savepoint with exception isolated
        with pytest.raises(ValueError, match="Sub-operation error"):
            with uow.savepoint("sp_isolated"):
                raise ValueError("Sub-operation error")

        # Parent unit of work remains active after savepoint rollback
        assert uow.is_active
        assert uow.state == "ACTIVE"

    # Parent unit of work commits successfully
    assert uow.state == "COMMITTED"


def test_explicit_unit_of_work_savepoint_rollback():
    uow = UnitOfWork(42)
    assert uow.id == 42
    assert uow.is_active

    uow.create_savepoint("checkpoint_a")
    uow.create_savepoint("checkpoint_b")

    # Rollback to checkpoint_a
    uow.rollback_to_savepoint("checkpoint_a")
    assert uow.is_active

    uow.commit()
    assert uow.state == "COMMITTED"


def test_invalid_operations_on_completed_unit_of_work():
    uow = UnitOfWork()
    uow.commit()

    with pytest.raises(ValueError, match="Cannot commit inactive transaction"):
        uow.commit()

    with pytest.raises(ValueError, match="Cannot rollback inactive transaction"):
        uow.rollback()

    with pytest.raises(ValueError, match="Cannot create savepoint on inactive transaction"):
        uow.savepoint("sp_fail")


def test_session_unit_of_work_rollback_restores_modified_entity():
    session, _, person = _make_session()

    uow = session.unit_of_work()
    person.name = "Bob"
    person.balance = 999.0
    assert person.get("name") == "Bob"
    assert person.get("balance") == 999.0

    uow.rollback()

    assert uow.state == "ROLLED_BACK"
    assert person.get("name") == "Alice"
    assert person.get("balance") == 100.0
    assert person.dirty_fields == {}


def test_session_unit_of_work_savepoint_restores_savepoint_state():
    session, _, person = _make_session()

    uow = session.unit_of_work()
    person.name = "Before Savepoint"
    uow.create_savepoint("sp_state")

    person.name = "After Savepoint"
    person.balance = 500.0
    uow.rollback_to_savepoint("sp_state")

    assert person.get("name") == "Before Savepoint"
    assert person.get("balance") == 100.0
    assert person.dirty_fields == {"name": "Before Savepoint"}

    uow.rollback()
    assert person.get("name") == "Alice"
    assert person.dirty_fields == {}


def test_entity_registered_inside_unit_of_work_is_removed_on_rollback():
    session, _, _ = _make_session()
    uow = session.unit_of_work()

    temporary = UowPerson(id="u2", name="Temporary")
    session.register(temporary, is_clean=True)
    assert session.get(UowPerson, "u2") is temporary

    uow.rollback()

    assert session.get(UowPerson, "u2") is None


def test_commit_keeps_dirty_state_for_flush():
    session, bridge, person = _make_session()
    uow = session.unit_of_work()

    person.name = "Bob"
    uow.commit()

    assert uow.state == "COMMITTED"
    assert person.get("name") == "Bob"
    assert person.dirty_fields == {"name": "Bob"}

    results = session.flush()

    assert len(results) == 1
    assert results[0].total_records == 1
    assert person.dirty_fields == {}
    assert any("MERGE" in statement.upper() for statement, _ in bridge.executed_queries)


@pytest.mark.asyncio
async def test_async_session_unit_of_work_rollback_and_commit_flush():
    bridge = AsyncMockBridge()
    session = AsyncSession(bridge=bridge, dialect="cypher", enable_identity_map=True)
    person = UowPerson(id="a1", name="Alice", balance=100.0)
    session.register(person, is_clean=True)

    uow = session.unit_of_work()
    person.name = "Bob"
    uow.rollback()
    assert person.get("name") == "Alice"
    assert person.dirty_fields == {}

    uow = session.unit_of_work()
    person.name = "Bob"
    uow.commit()
    assert person.dirty_fields == {"name": "Bob"}

    results = await session.flush()

    assert len(results) == 1
    assert results[0].total_records == 1
    assert person.dirty_fields == {}


def test_unit_of_work_warns_when_identity_map_disabled():
    session = Session(bridge=MockBridge(), dialect="cypher", enable_identity_map=False)
    with pytest.warns(UserWarning, match="identity map is disabled"):
        session.unit_of_work()
