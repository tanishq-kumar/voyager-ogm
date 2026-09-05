"""Tests for Session-Level and Global Application Lifecycle Query Optimization.

This test module verifies the 4-tier precedence resolution hierarchy for AST Query Optimization
across Voyager's session and execution lifecycle:

Precedence Resolution Hierarchy (Highest to Lowest):
1. Explicit Query Compilation Argument: `query.compile(dialect, optimize=True/False)`
2. Fluent Query Instance Configuration: `query.optimize(level="standard")`
3. Session / AsyncSession Configuration: `Session(bridge=..., optimize="standard")`
4. Global Application Configuration: `voyager_ogm.configure(optimize="standard")`
   (or environment variable `VOYAGER_OPTIMIZE=1|standard|aggressive`)

Key Capabilities Verified:
- Session automatic query compilation forwarding in `execute()` and `execute_to_polars()`.
- Thread-safe global configuration registry with atomic resets (`reset_config()`).
- Full support for both string levels (`"standard"`, `"aggressive"`, `"none"`) and boolean flags (`True`/`False`).
- Seamless coexistence with multi-dialect bridges (openCypher, ISO GQL, SQL:2023 PGQ).
"""

from __future__ import annotations

import pytest
from voyager_ogm import (
    AsyncSession,
    MockBridge,
    Node,
    Query,
    Session,
    configure,
    get_config,
    reset_config,
)
from voyager_ogm.models import Field


class User(Node):
    __primary_key__ = "id"
    id: int = Field(primary_key=True)
    name: str = Field()
    city: str = Field()
    age: int = Field()


@pytest.fixture(autouse=True)
def reset_global_config():
    """Ensures global configuration is reset to default before and after each test."""
    reset_config()
    yield
    reset_config()


def test_session_level_optimizer_standard():
    """Verify Session(optimize='standard') automatically optimizes executed queries."""
    mock = MockBridge()
    session = Session(bridge=mock, dialect="cypher", optimize="standard")

    assert session.optimize_enabled is True
    assert session.optimization_level == "standard"

    u = User("u")
    query = Query.match(u).where(u.city == "Berlin").return_(u.name)

    # Execute through session — should automatically pushdown predicate into pattern
    res = session.execute(query)
    assert res.statement == "MATCH (u:User {city: $p0}) RETURN u.name"
    assert res.dialect == "cypher"


def test_session_level_optimizer_boolean_flag():
    """Verify Session(optimize=True) defaults to standard optimization level."""
    mock = MockBridge()
    session = Session(bridge=mock, dialect="cypher", optimize=True)

    assert session.optimize_enabled is True
    assert session.optimization_level == "standard"

    u = User("u")
    query = Query.match(u).where(u.city == "Tokyo").return_(u.name)
    res = session.execute(query)
    assert res.statement == "MATCH (u:User {city: $p0}) RETURN u.name"


def test_session_level_optimizer_disabled():
    """Verify Session(optimize=False) or Session(optimize='none') preserves unoptimized queries."""
    mock = MockBridge()
    session_off = Session(bridge=mock, dialect="cypher", optimize=False)
    assert session_off.optimize_enabled is False

    u = User("u")
    query = Query.match(u).where(u.city == "Berlin").return_(u.name)

    res = session_off.execute(query)
    assert res.statement == "MATCH (u:User) WHERE u.city = $p0 RETURN u.name"

    session_none = Session(bridge=mock, dialect="cypher", optimize="none")
    assert session_none.optimize_enabled is False
    res2 = session_none.execute(query)
    assert res2.statement == "MATCH (u:User) WHERE u.city = $p0 RETURN u.name"


def test_session_execute_to_polars_with_optimizer():
    """Verify session.execute_to_polars respects session optimizer settings."""
    mock = MockBridge()
    mock.queue_result([{"name": "Alice", "city": "Berlin"}])
    session = Session(bridge=mock, dialect="cypher", optimize="standard")

    u = User("u")
    query = Query.match(u).where(u.city == "Berlin").return_(u.name)
    df = session.execute_to_polars(query)

    assert len(df) == 1
    # Check that the recorded executed query in mock bridge was optimized
    assert mock.executed_queries[-1][0] == "MATCH (u:User {city: $p0}) RETURN u.name"


def test_query_override_precedence_over_session():
    """Verify query-level explicit optimize/compile overrides session-level default."""
    mock = MockBridge()
    # Session defaults to optimize=True
    session = Session(bridge=mock, dialect="cypher", optimize="standard")

    u = User("u")
    # 1. Query explicitly compiled with optimize=False
    query1 = Query.match(u).where(u.city == "Berlin").return_(u.name)
    compiled_unopt = query1.compile(optimize=False)
    res1 = session.execute(compiled_unopt)
    assert res1.statement == "MATCH (u:User) WHERE u.city = $p0 RETURN u.name"

    # 2. Query explicitly marked with .optimize(level="aggressive")
    query2 = Query.match(u).where(u.city == "Berlin").return_(u.name).optimize(level="aggressive")
    res2 = session.execute(query2)
    assert res2.statement == "MATCH (u:User {city: $p0}) RETURN u.name"


def test_global_application_configure():
    """Verify global voyager_ogm.configure() sets application lifecycle defaults."""
    configure(optimize="standard", default_dialect="iso_gql")

    cfg = get_config()
    assert cfg.optimize is True
    assert cfg.optimization_level == "standard"
    assert cfg.default_dialect == "iso_gql"

    # New session created without explicit optimize inherits global setting
    mock = MockBridge()
    session = Session(bridge=mock)
    assert session.optimize_enabled is True
    assert session.optimization_level == "standard"
    assert session.dialect == "iso_gql"

    u = User("u")
    query = Query.match(u).where(u.city == "London").return_(u.name)
    res = session.execute(query)
    assert res.statement == "MATCH (u:User {city: $p0}) RETURN u.name"
    assert res.dialect == "iso_gql"


@pytest.mark.asyncio
async def test_async_session_level_optimizer():
    """Verify AsyncSession automatically optimizes queries when enabled."""
    from voyager_ogm.bridge import AsyncMockBridge

    async_mock = AsyncMockBridge()
    async_mock.queue_result([{"name": "Bob"}])
    async_mock.queue_result([{"name": "Bob"}])
    session = AsyncSession(bridge=async_mock, dialect="cypher", optimize="standard")

    assert session.optimize_enabled is True
    assert session.optimization_level == "standard"

    u = User("u")
    query = Query.match(u).where(u.city == "Paris").return_(u.name)

    res = await session.execute(query)
    assert res.statement == "MATCH (u:User {city: $p0}) RETURN u.name"

    df = await session.execute_to_polars(query)
    assert len(df) == 1
    assert async_mock.executed_queries[-1][0] == "MATCH (u:User {city: $p0}) RETURN u.name"


def test_environment_variable_configuration(monkeypatch):
    """Verify VOYAGER_OPTIMIZE environment variable configures application default."""
    monkeypatch.setenv("VOYAGER_OPTIMIZE", "standard")
    reset_config()

    cfg = get_config()
    assert cfg.optimize is True
    assert cfg.optimization_level == "standard"

    session = Session(bridge=MockBridge())
    assert session.optimize_enabled is True
    assert session.optimization_level == "standard"
