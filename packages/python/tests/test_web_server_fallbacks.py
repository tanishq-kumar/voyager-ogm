"""Tests for async web server failure modes, error classification, and fallback mechanisms."""

from __future__ import annotations

import importlib.util
import logging
import os
import time
from typing import Any

import pytest
import voyager_ogm._voyager_rs as _voyager_rs
from voyager_ogm import (
    AsyncMockBridge,
    AsyncSession,
    MockBridge,
    Session,
)
from voyager_ogm.bridge import create_bridge
from voyager_ogm.session import _is_query_semantic_error, _SessionBase


def test_is_query_semantic_error_classification():
    """Verify semantic/syntax errors are distinguished from transport/socket failures."""
    # Semantic / syntax errors -> True
    assert _is_query_semantic_error(
        ValueError("Neo.ClientError.Statement.SyntaxError: Invalid token")
    )
    assert _is_query_semantic_error(
        RuntimeError("Neo.ClientError.Schema.ConstraintValidationFailed: Already exists")
    )
    assert _is_query_semantic_error(Exception("syntax error at or near 'WHERE'"))
    assert _is_query_semantic_error(Exception("duplicate key value violates unique constraint"))
    assert _is_query_semantic_error(Exception("column c.name does not exist"))

    # Transport / network failures -> False
    assert not _is_query_semantic_error(ConnectionResetError("Connection reset by peer"))
    assert not _is_query_semantic_error(TimeoutError("Timed out waiting for connection"))
    assert not _is_query_semantic_error(RuntimeError("PoolExhausted: No connections available"))
    assert not _is_query_semantic_error(Exception("Broken pipe"))
    assert not _is_query_semantic_error(Exception("Failed to connect to host: Connection refused"))


class MockNativeClientWithErrors:
    """Mock NativeClient that raises controllable errors for fallback testing."""

    def __init__(self, error_to_raise: Exception | None = None) -> None:
        self.error_to_raise = error_to_raise

    def execute_sync(self, query: str, params: dict[str, Any] | None = None):
        if self.error_to_raise:
            raise self.error_to_raise
        return type("Res", (), {"stream": None, "summary": None})()

    async def execute(self, query: str, params: dict[str, Any] | None = None):
        if self.error_to_raise:
            raise self.error_to_raise
        return type("Res", (), {"stream": None, "summary": None})()

    def ping_sync(self):
        return True

    async def ping(self):
        return True

    def close(self):
        pass


def test_query_semantic_error_does_not_degrade_sync_session():
    """Syntax or constraint errors must raise directly and NOT permanently degrade Session backend."""
    native_mock = MockNativeClientWithErrors(
        ValueError("Neo.ClientError.Statement.SyntaxError: Invalid query token")
    )
    session = Session(bridge=native_mock, backend="auto")
    assert session.backend == "native"

    with pytest.raises(ValueError, match="SyntaxError"):
        session.execute("MATCH (n INVALID SYNTAX)")

    # Crucial assertion: Backend MUST remain 'native' rather than permanently falling back!
    assert session.backend == "native"


@pytest.mark.asyncio
async def test_query_semantic_error_does_not_degrade_async_session():
    """Syntax or constraint errors must raise directly and NOT permanently degrade AsyncSession backend."""
    native_mock = MockNativeClientWithErrors(
        ValueError("Neo.ClientError.Statement.SyntaxError: Invalid query token")
    )
    session = AsyncSession(bridge=native_mock, backend="auto")
    assert session.backend == "native"

    with pytest.raises(ValueError, match="SyntaxError"):
        await session.execute("MATCH (n INVALID SYNTAX)")

    # Crucial assertion: Backend MUST remain 'native'
    assert session.backend == "native"


def test_no_silent_mock_bridge_trap_on_real_uri():
    """If native client fails with transport error on a real URI, do NOT silently fall back to empty mock data."""
    # When bridge is a real URI and native fails with a network error,
    # falling back to AsyncMockBridge/MockBridge would deceive the user with empty canned responses.
    native_mock = MockNativeClientWithErrors(
        ConnectionResetError("Socket broken: connection reset by peer")
    )
    session = Session(bridge=native_mock, backend="auto")
    # Manually configure an internal mock bridge to simulate no real driver installed
    session._is_explicit_mock = False
    session._bridge = MockBridge()

    with pytest.raises(RuntimeError, match="No official database driver available for fallback"):
        session.execute("MATCH (n) RETURN n")


@pytest.mark.asyncio
async def test_no_silent_mock_bridge_trap_on_real_uri_async():
    """AsyncSession must raise RuntimeError rather than silently returning mock data on real URI failure."""
    native_mock = MockNativeClientWithErrors(
        ConnectionResetError("Socket broken: connection reset by peer")
    )
    session = AsyncSession(bridge=native_mock, backend="auto")
    session._is_explicit_mock = False
    session._bridge = AsyncMockBridge()

    with pytest.raises(RuntimeError, match="No official database driver available for fallback"):
        await session.execute("MATCH (n) RETURN n")


def test_explicit_mock_uri_allows_mock_fallback():
    """Explicit mock:// or MockBridge sessions should allow mock execution without error."""
    session = Session(bridge=MockBridge(), backend="auto")
    assert session.backend == "bridge"
    res = session.execute("MATCH (n) RETURN n")
    assert res == []


def test_reset_backend_restores_native_mode():
    """Verify session.reset_backend() allows restoring native execution after transient degradation."""
    native_mock = MockNativeClientWithErrors()
    session = Session(bridge=native_mock, backend="auto")
    assert session.backend == "native"

    # Simulate degradation to bridge
    session._active_backend = "bridge"
    assert session.backend == "bridge"

    # Reset backend
    session.reset_backend()
    assert session.backend == "native"


@pytest.mark.asyncio
async def test_async_reset_backend_restores_native_mode():
    """Verify async_session.reset_backend() allows restoring native execution after transient degradation."""
    native_mock = MockNativeClientWithErrors()
    session = AsyncSession(bridge=native_mock, backend="auto")
    assert session.backend == "native"

    session._active_backend = "bridge"
    assert session.backend == "bridge"

    session.reset_backend()
    assert session.backend == "native"


def test_fork_safety_runtime_pid_tracking():
    """Verify Tokio runtime PID tracking correctly reflects the current process ID."""
    current_pid = os.getpid()
    runtime_pid = _voyager_rs.get_runtime_pid()
    assert runtime_pid == current_pid


def test_create_bridge_auto_instantiation_from_uri():
    """create_bridge should recognize bolt:// scheme and auto-instantiate official driver if present."""
    sync_bridge = create_bridge("bolt://localhost:7687", is_async=False)
    async_bridge = create_bridge("bolt://localhost:7687", is_async=True)

    if importlib.util.find_spec("neo4j") is not None:
        assert type(sync_bridge).__name__ == "Neo4jBoltBridge"
        assert type(async_bridge).__name__ == "AsyncNeo4jBoltBridge"
    else:
        assert isinstance(sync_bridge, MockBridge)
        assert isinstance(async_bridge, AsyncMockBridge)


def test_session_base_inheritance():
    """Verify Session and AsyncSession inherit common properties and methods from _SessionBase."""
    session = Session(bridge=MockBridge())
    async_session = AsyncSession(bridge=AsyncMockBridge())

    assert isinstance(session, _SessionBase)
    assert isinstance(async_session, _SessionBase)

    # Shared properties work identically
    assert session.dialect == async_session.dialect
    assert session.backend == "bridge"
    assert async_session.backend == "bridge"


def test_backend_downgrade_logs_warning_and_circuit_breaker_probe_sync(caplog):
    """Verify native execution failure logs a warning and circuit breaker probes native after cooldown."""
    native_mock = MockNativeClientWithErrors(
        ConnectionResetError("Socket broken: connection reset by peer")
    )
    # Configure short cooldown of 0.02s for testing
    session = Session(bridge=native_mock, backend="auto", circuit_cooldown_seconds=0.02)
    session._is_explicit_mock = True  # Allow mock fallback
    assert session.backend == "native"

    with caplog.at_level(logging.WARNING, logger="voyager_ogm.session"):
        res = session.execute("MATCH (n) RETURN n")

    # 1. Fallback to bridge must succeed
    assert res == []
    assert session.backend == "bridge"

    # 2. Structured warning must be logged
    assert any(
        "Native backend execution failed" in record.message
        and "Falling back to bridge backend" in record.message
        for record in caplog.records
    )

    # 3. Subsequent query during cooldown stays on bridge
    res2 = session.execute("MATCH (n) RETURN n")
    assert res2 == []
    assert session.backend == "bridge"

    # 4. Heal the native client
    native_mock.error_to_raise = None

    # 5. Wait for circuit breaker cooldown to expire
    time.sleep(0.08)

    # 6. Next query probes native and recovers
    with caplog.at_level(logging.INFO, logger="voyager_ogm.session"):
        session.execute("MATCH (n) RETURN n")

    assert session.backend == "native"
    assert any("Native execution backend recovered" in record.message for record in caplog.records)


@pytest.mark.asyncio
async def test_backend_downgrade_logs_warning_and_circuit_breaker_probe_async(caplog):
    """Verify async session native failure logs warning and recovers via circuit breaker probe."""
    import asyncio

    native_mock = MockNativeClientWithErrors(
        ConnectionResetError("Socket broken: connection reset by peer")
    )
    session = AsyncSession(bridge=native_mock, backend="auto", circuit_cooldown_seconds=0.02)
    session._is_explicit_mock = True
    assert session.backend == "native"

    with caplog.at_level(logging.WARNING, logger="voyager_ogm.session"):
        res = await session.execute("MATCH (n) RETURN n")

    assert res == []
    assert session.backend == "bridge"
    assert any("Native backend execution failed" in record.message for record in caplog.records)

    # Cooldown in effect -> stays bridge
    await session.execute("MATCH (n) RETURN n")
    assert session.backend == "bridge"

    # Heal native client
    native_mock.error_to_raise = None
    await asyncio.sleep(0.08)

    # Probes native and recovers
    with caplog.at_level(logging.INFO, logger="voyager_ogm.session"):
        await session.execute("MATCH (n) RETURN n")

    assert session.backend == "native"
    assert any("Native execution backend recovered" in record.message for record in caplog.records)
