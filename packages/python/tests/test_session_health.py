"""Tests for declarative liveness probe and health checks (session.ping, session.health)."""

from __future__ import annotations

import time
from typing import Any

import pytest
from voyager_ogm import (
    AsyncMockBridge,
    AsyncSession,
    MockBridge,
    Session,
)


class MockNativeClientForHealth:
    """Mock NativeClient supporting configurable execution, ping, and errors."""

    def __init__(
        self,
        error_to_raise: Exception | None = None,
        ping_returns: bool = True,
        ping_error: Exception | None = None,
    ) -> None:
        self.error_to_raise = error_to_raise
        self.ping_returns = ping_returns
        self.ping_error = ping_error
        self.ping_sync_calls = 0
        self.ping_async_calls = 0

    def execute_sync(self, query: str, params: dict[str, Any] | None = None):
        if self.error_to_raise:
            raise self.error_to_raise
        return type("Res", (), {"stream": None, "summary": None})()

    async def execute(self, query: str, params: dict[str, Any] | None = None):
        if self.error_to_raise:
            raise self.error_to_raise
        return type("Res", (), {"stream": None, "summary": None})()

    def ping_sync(self) -> bool:
        self.ping_sync_calls += 1
        if self.ping_error:
            raise self.ping_error
        return self.ping_returns

    async def ping(self) -> bool:
        self.ping_async_calls += 1
        if self.ping_error:
            raise self.ping_error
        return self.ping_returns

    def close(self):
        pass


class FailingBridge(MockBridge):
    """Mock bridge that fails ping."""

    def __init__(self, ping_returns: bool = False, ping_error: Exception | None = None) -> None:
        super().__init__()
        self.ping_returns = ping_returns
        self.ping_error = ping_error

    def ping(self) -> bool:
        if self.ping_error:
            raise self.ping_error
        return self.ping_returns


class AsyncFailingBridge(AsyncMockBridge):
    """Async mock bridge that fails ping."""

    def __init__(self, ping_returns: bool = False, ping_error: Exception | None = None) -> None:
        super().__init__()
        self.ping_returns = ping_returns
        self.ping_error = ping_error

    async def ping(self) -> bool:
        if self.ping_error:
            raise self.ping_error
        return self.ping_returns


def test_sync_session_health_healthy_mock():
    """Verify default Session with MockBridge returns healthy diagnostics."""
    session = Session(bridge=MockBridge(), dialect="cypher")

    assert session.ping() is True

    h = session.health()
    assert h["status"] == "healthy"
    assert h["backend"] == "bridge"
    assert h["circuit_state"] == "closed"
    assert h["consecutive_failures"] == 0
    assert h["remaining_cooldown_ms"] == 0
    assert h["dialect"] == "cypher"
    assert isinstance(h["latency_ms"], float)
    assert h["latency_ms"] >= 0.0


@pytest.mark.asyncio
async def test_async_session_health_healthy_mock():
    """Verify AsyncSession with AsyncMockBridge returns healthy diagnostics."""
    session = AsyncSession(bridge=AsyncMockBridge(), dialect="cypher")

    assert await session.ping() is True

    h = await session.health()
    assert h["status"] == "healthy"
    assert h["backend"] == "bridge"
    assert h["circuit_state"] == "closed"
    assert h["consecutive_failures"] == 0
    assert h["remaining_cooldown_ms"] == 0
    assert h["dialect"] == "cypher"
    assert isinstance(h["latency_ms"], float)
    assert h["latency_ms"] >= 0.0


def test_sync_session_health_native_healthy():
    """Verify Session with healthy NativeClient reports native backend and healthy status."""
    client = MockNativeClientForHealth()
    session = Session(bridge=client, backend="auto")

    assert session.backend == "native"
    assert session.ping() is True
    assert client.ping_sync_calls == 1

    h = session.health()
    assert h["status"] == "healthy"
    assert h["backend"] == "native"
    assert h["circuit_state"] == "closed"
    assert h["consecutive_failures"] == 0
    assert h["remaining_cooldown_ms"] == 0
    assert h["dialect"] == "cypher"
    assert h["latency_ms"] >= 0.0


@pytest.mark.asyncio
async def test_async_session_health_native_healthy():
    """Verify AsyncSession with healthy NativeClient reports native backend and healthy status."""
    client = MockNativeClientForHealth()
    session = AsyncSession(bridge=client, backend="auto")

    assert session.backend == "native"
    assert await session.ping() is True
    assert client.ping_async_calls == 1

    h = await session.health()
    assert h["status"] == "healthy"
    assert h["backend"] == "native"
    assert h["circuit_state"] == "closed"
    assert h["consecutive_failures"] == 0
    assert h["remaining_cooldown_ms"] == 0
    assert h["dialect"] == "cypher"
    assert h["latency_ms"] >= 0.0


def test_sync_session_health_degraded_circuit_open():
    """Verify tripped circuit breaker reports degraded status and remaining cooldown."""
    client = MockNativeClientForHealth()
    session = Session(bridge=client, backend="auto")
    assert session.backend == "native"

    # Trip circuit breaker
    session.circuit_router.trip()
    assert session.backend == "bridge"

    h = session.health()
    assert h["status"] == "degraded"
    assert h["backend"] == "bridge"
    assert h["circuit_state"] == "open"
    assert h["remaining_cooldown_ms"] > 0
    # Fallback mock bridge is still healthy, so ping succeeds
    assert session.ping() is True


@pytest.mark.asyncio
async def test_async_session_health_degraded_circuit_open():
    """Verify AsyncSession reports degraded status when circuit breaker is tripped."""
    client = MockNativeClientForHealth()
    session = AsyncSession(bridge=client, backend="auto")
    assert session.backend == "native"

    session.circuit_router.trip()
    assert session.backend == "bridge"

    h = await session.health()
    assert h["status"] == "degraded"
    assert h["backend"] == "bridge"
    assert h["circuit_state"] == "open"
    assert h["remaining_cooldown_ms"] > 0
    assert await session.ping() is True


def test_sync_session_health_recovery_after_reset():
    """Verify session.reset_backend() restores healthy state after degradation."""
    client = MockNativeClientForHealth()
    session = Session(bridge=client, backend="auto")
    session.circuit_router.trip()

    degraded_h = session.health()
    assert degraded_h["status"] == "degraded"
    assert degraded_h["circuit_state"] == "open"

    # Reset backend
    session.reset_backend()
    assert session.backend == "native"

    recovered_h = session.health()
    assert recovered_h["status"] == "healthy"
    assert recovered_h["backend"] == "native"
    assert recovered_h["circuit_state"] == "closed"
    assert recovered_h["remaining_cooldown_ms"] == 0


def test_sync_session_health_failing_when_ping_fails():
    """Verify session.health() reports failing when ping probe returns False or raises."""
    failing_bridge = FailingBridge(ping_returns=False)
    session = Session(bridge=failing_bridge)

    assert session.ping() is False

    h = session.health()
    assert h["status"] == "failing"
    assert h["backend"] == "bridge"
    assert isinstance(h["latency_ms"], float)
    assert h["latency_ms"] >= 0.0

    # Also test when ping raises an unhandled transport error
    error_bridge = FailingBridge(ping_error=ConnectionResetError("Socket broken"))
    session_error = Session(bridge=error_bridge)

    assert session_error.ping() is False
    h_error = session_error.health()
    assert h_error["status"] == "failing"


@pytest.mark.asyncio
async def test_async_session_health_failing_when_ping_fails():
    """Verify async session.health() reports failing when ping probe fails."""
    async_failing_bridge = AsyncFailingBridge(ping_returns=False)
    session = AsyncSession(bridge=async_failing_bridge)

    assert await session.ping() is False

    h = await session.health()
    assert h["status"] == "failing"
    assert h["backend"] == "bridge"
    assert isinstance(h["latency_ms"], float)
    assert h["latency_ms"] >= 0.0

    # Also test with ping error
    async_error_bridge = AsyncFailingBridge(ping_error=TimeoutError("Probe timed out"))
    session_error = AsyncSession(bridge=async_error_bridge)

    assert await session_error.ping() is False
    h_error = await session_error.health()
    assert h_error["status"] == "failing"


def test_sync_session_health_native_ping_failure():
    """Verify native client ping failure reports failing status when native backend is active."""
    client = MockNativeClientForHealth(ping_returns=False)
    session = Session(bridge=client, backend="auto")
    assert session.backend == "native"

    assert session.ping() is False
    h = session.health()
    assert h["status"] == "failing"
    assert h["backend"] == "native"


def test_sync_session_health_requested_native_circuit_tripped():
    """Verify session with requested_backend='native' reports degraded when tripped."""
    client = MockNativeClientForHealth()
    session = Session(bridge=client, backend="native")
    assert session.backend == "native"

    session.circuit_router.trip()
    assert session.backend == "bridge"

    h = session.health()
    assert h["status"] == "degraded"
    assert h["backend"] == "bridge"
    assert h["circuit_state"] == "open"


def test_session_health_half_open_circuit_state():
    """Verify session.health() reports degraded during half_open circuit state."""
    client = MockNativeClientForHealth()
    # Configure very short cooldown of 0.02s
    session = Session(bridge=client, backend="auto", circuit_cooldown_seconds=0.02)
    session.circuit_router.record_failure(is_transient=True)
    assert session.circuit_router.state == "open"

    time.sleep(0.05)
    assert session.circuit_router.state == "half_open"

    h = session.health()
    assert h["status"] == "degraded"
    assert h["circuit_state"] == "half_open"


def test_session_health_dialect_preservation():
    """Verify session.health() reports the configured query dialect."""
    for dialect_name in ("cypher", "sql_pgq", "test-dialect", "duckdb"):
        s = Session(bridge=MockBridge(), dialect=dialect_name)
        h = s.health()
        assert h["dialect"] == dialect_name


try:
    import duckdb

    DUCKDB_AVAILABLE = True
except ImportError:
    DUCKDB_AVAILABLE = False


@pytest.mark.skipif(not DUCKDB_AVAILABLE, reason="DuckDB is not installed")
def test_live_duckdb_session_ping_and_health():
    """Verify Session.ping() and Session.health() against live embedded DuckDB engine."""
    con = duckdb.connect()
    try:
        session = Session(bridge=con, dialect="duckdb")
        assert session.ping() is True

        h = session.health()
        assert h["status"] == "healthy"
        assert h["backend"] == "bridge"
        assert h["circuit_state"] == "closed"
        assert h["consecutive_failures"] == 0
        assert h["remaining_cooldown_ms"] == 0
        assert h["dialect"] == "duckdb"
        assert isinstance(h["latency_ms"], float)
        assert h["latency_ms"] >= 0.0
    finally:
        con.close()

    # Once closed, ping returns False and health reports failing
    assert session.ping() is False
    h_closed = session.health()
    assert h_closed["status"] == "failing"
    assert h_closed["backend"] == "bridge"


def test_session_health_typed_dict_contract():
    """Verify session.health() conforms to SessionHealth TypedDict schema."""
    from voyager_ogm import SessionHealth

    s = Session(bridge=MockBridge(), dialect="cypher")
    h = s.health()
    assert set(SessionHealth.__annotations__.keys()) == set(h.keys())
    assert h["status"] in ("healthy", "degraded", "failing")
    assert h["backend"] in ("native", "bridge")
    assert h["circuit_state"] in ("closed", "open", "half_open")
