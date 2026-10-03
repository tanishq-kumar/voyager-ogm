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


def test_native_circuit_router_lifecycle():
    """Verify NativeCircuitRouter transitions through Closed -> Open -> HalfOpen -> Closed."""
    from voyager_ogm import NativeCircuitRouter

    router = NativeCircuitRouter(failure_threshold=2, cooldown_seconds=0.03, success_threshold=1)
    assert router.state == "closed"
    assert router.route() == "native"
    assert router.should_route_native()
    assert not router.should_probe()
    assert router.consecutive_failures == 0
    assert router.failure_threshold == 2
    assert router.cooldown_seconds == pytest.approx(0.03)
    assert "NativeCircuitRouter" in repr(router)

    # 1. Semantic error does not increment failure count
    router.record_failure(is_transient=False)
    assert router.consecutive_failures == 0
    assert router.state == "closed"

    # 2. First transient failure: below threshold 2
    router.record_failure(is_transient=True)
    assert router.consecutive_failures == 1
    assert router.state == "closed"
    assert router.route() == "native"

    # 3. Second transient failure: trips to Open
    router.record_failure(is_transient=True)
    assert router.consecutive_failures == 2
    assert router.state == "open"
    assert router.route() == "fallback"
    assert not router.should_route_native()
    assert not router.should_probe()

    # 4. Wait for cooldown to expire
    time.sleep(0.05)

    # 5. Automatically transitions to HalfOpen for trial probe
    assert router.state == "half_open"
    assert router.route() == "probe"
    assert router.should_probe()
    assert router.should_route_native()

    # 6. Successful probe restores Closed state
    router.record_success()
    assert router.state == "closed"
    assert router.route() == "native"
    assert router.consecutive_failures == 0


def test_native_circuit_router_half_open_failure_reopens():
    """Verify a probe failure in HalfOpen immediately returns the circuit to Open."""
    from voyager_ogm import NativeCircuitRouter

    router = NativeCircuitRouter(failure_threshold=1, cooldown_seconds=0.02)
    router.record_failure(is_transient=True)
    assert router.state == "open"

    time.sleep(0.04)
    assert router.state == "half_open"
    assert router.route() == "probe"

    # Probe fails
    router.record_failure(is_transient=True)
    assert router.state == "open"
    assert router.route() == "fallback"


def test_native_circuit_router_manual_trip_and_reset():
    """Verify manual trip() and reset() methods."""
    from voyager_ogm import NativeCircuitRouter

    router = NativeCircuitRouter()
    assert router.state == "closed"

    router.trip()
    assert router.state == "open"
    assert router.route() == "fallback"

    router.reset()
    assert router.state == "closed"
    assert router.route() == "native"
    assert router.consecutive_failures == 0


def test_native_circuit_router_error_classification_static():
    """Verify static classification helpers on NativeCircuitRouter."""
    from voyager_ogm import NativeCircuitRouter

    assert NativeCircuitRouter.is_semantic_error("SyntaxError: near SELECT")
    assert NativeCircuitRouter.is_semantic_error("ConstraintValidationFailed: key exists")
    assert not NativeCircuitRouter.is_semantic_error("Connection reset by peer")
    assert not NativeCircuitRouter.is_semantic_error("Timed out")

    assert NativeCircuitRouter.classify_error("syntax error") == "semantic"
    assert NativeCircuitRouter.classify_error("Connection refused") == "transient"


def test_session_configurable_failure_threshold_sync():
    """Verify Session respects circuit_failure_threshold before tripping backend."""
    native_mock = MockNativeClientWithErrors(
        ConnectionResetError("Socket broken: connection reset by peer")
    )
    # Threshold = 3: requires 3 transient failures before tripping to bridge
    session = Session(
        bridge=native_mock,
        backend="auto",
        circuit_cooldown_seconds=0.05,
        circuit_failure_threshold=3,
    )
    session._is_explicit_mock = True
    assert session.backend == "native"
    assert session.circuit_router.failure_threshold == 3

    # Attempt 1: fails, but below threshold -> falls back for this query, but backend remains native candidate
    session.execute("MATCH (n) RETURN n")
    assert session.circuit_router.consecutive_failures == 1
    assert session.backend == "native"

    # Attempt 2: second failure
    session.execute("MATCH (n) RETURN n")
    assert session.circuit_router.consecutive_failures == 2
    assert session.backend == "native"

    # Attempt 3: third failure -> reaches threshold 3, trips circuit to Open!
    session.execute("MATCH (n) RETURN n")
    assert session.circuit_router.consecutive_failures == 3
    assert session.backend == "bridge"

    # Heal native mock and wait for cooldown
    native_mock.error_to_raise = None
    time.sleep(0.08)

    # Next query executes probe and recovers
    session.execute("MATCH (n) RETURN n")
    assert session.backend == "native"
    assert session.circuit_router.consecutive_failures == 0


@pytest.mark.asyncio
async def test_session_configurable_failure_threshold_async():
    """Verify AsyncSession respects circuit_failure_threshold before tripping backend."""
    import asyncio

    native_mock = MockNativeClientWithErrors(
        ConnectionResetError("Socket broken: connection reset by peer")
    )
    session = AsyncSession(
        bridge=native_mock,
        backend="auto",
        circuit_cooldown_seconds=0.05,
        circuit_failure_threshold=2,
    )
    session._is_explicit_mock = True
    assert session.backend == "native"
    assert session.circuit_router.failure_threshold == 2

    # Attempt 1
    await session.execute("MATCH (n) RETURN n")
    assert session.circuit_router.consecutive_failures == 1
    assert session.backend == "native"

    # Attempt 2 -> trips to bridge
    await session.execute("MATCH (n) RETURN n")
    assert session.circuit_router.consecutive_failures == 2
    assert session.backend == "bridge"

    # Heal and recover
    native_mock.error_to_raise = None
    await asyncio.sleep(0.08)

    await session.execute("MATCH (n) RETURN n")
    assert session.backend == "native"
    assert session.circuit_router.consecutive_failures == 0


def test_live_tcp_circuit_failover_on_unreachable_endpoint():
    """Verify Session trips circuit when NativeClient encounters real OS socket connection failure."""
    import socket

    # Bind and immediately close a socket to find an unused local port
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        dead_port = s.getsockname()[1]

    dead_uri = f"bolt://127.0.0.1:{dead_port}?connect_timeout=1"
    # When NativeClient tries to connect to dead_uri, the OS kernel rejects the connection
    session = Session(bridge=dead_uri, backend="auto", circuit_cooldown_seconds=0.05)
    # Enable explicit mock bridge fallback for testing the graceful downgrade
    session._is_explicit_mock = True
    assert session.backend == "native"

    # Query fails on native TCP transport and seamlessly falls back to bridge
    res = session.execute("MATCH (n) RETURN n")
    assert res == []

    # Circuit breaker has tripped to Open! Backend is now bridge.
    assert session.backend == "bridge"
    assert session.circuit_router.state == "open"
    assert session.circuit_router.consecutive_failures == 1
