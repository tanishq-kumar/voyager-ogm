"""FFI boundary hardening, Arrow C-Stream lifecycle, and dual-backend integration tests."""

from __future__ import annotations

import datetime
import uuid
from decimal import Decimal

import polars as pl
import pyarrow as pa
import pytest
from voyager_ogm import (
    AsyncSession,
    NativeClient,
    Query,
    Session,
    col,
    generate_synthetic_stream,
    lit,
)
from voyager_ogm._voyager_rs import NativeQueryBuilder, compile_query_from_spec

FALKORDB_URI = "redis://127.0.0.1:6379?connect_timeout=2"
NEO4J_URI = "bolt://neo4j:voyagerpass123@127.0.0.1:7687?connect_timeout=2"


def _is_falkordb_online() -> bool:
    try:
        c = NativeClient(FALKORDB_URI, min_idle=1, max_size=2)
        online = c.ping_sync()
        c.close()
        return online
    except Exception:
        return False


def _is_neo4j_online() -> bool:
    try:
        c = NativeClient(NEO4J_URI, min_idle=1, max_size=2)
        online = c.ping_sync()
        c.close()
        return online
    except Exception:
        return False


FALKORDB_ONLINE = _is_falkordb_online()
NEO4J_ONLINE = _is_neo4j_online()


# ---------------------------------------------------------------------------
# 1. AST Recursion Depth Guard (MAX_AST_DEPTH = 256)
# ---------------------------------------------------------------------------


def test_ast_max_depth_guard_raises_value_error():
    """Verify that deeply nested AST expressions (>256) trigger a controlled PyValueError."""
    nested_spec: tuple = ("lit", 1)
    for _ in range(260):
        nested_spec = ("bin", "+", nested_spec, ("lit", 1))

    query_spec = {
        "matches": [
            {
                "paths": [[("node", "n", ["Person"], None)]],
                "where": [nested_spec],
            }
        ],
        "returns": [[nested_spec, None]],
    }

    with pytest.raises(ValueError, match="Maximum AST expression recursion depth exceeded"):
        compile_query_from_spec(query_spec, "cypher")


def test_ast_within_depth_limit_succeeds():
    """Verify that nested AST expressions within the depth limit (<256) compile properly."""
    nested_spec: tuple = ("lit", 1)
    for _ in range(50):
        nested_spec = ("bin", "+", nested_spec, ("lit", 1))

    query_spec = {
        "matches": [
            {
                "paths": [[("node", "n", ["Person"], None)]],
                "where": [nested_spec],
            }
        ],
    }

    compiled = compile_query_from_spec(query_spec, "cypher")
    assert "MATCH (n:Person)" in compiled["statement"]
    assert "WHERE" in compiled["statement"]


# ---------------------------------------------------------------------------
# 2. Dynamic Parameter Expansion in FFI Boundary
# ---------------------------------------------------------------------------


def test_py_to_literal_parameter_expansion():
    """Verify conversion of dict, datetime, date, UUID, and Decimal into native literals."""
    test_uuid = uuid.uuid4()
    test_now = datetime.datetime(2026, 9, 10, 12, 0, 0, tzinfo=datetime.UTC)
    test_date = datetime.date(2026, 9, 10)
    test_decimal = Decimal("123.456")
    test_dict = {"key": "val", "count": 42}

    qb = NativeQueryBuilder()
    qb.match()
    qb.node("n", ["Person"])
    qb.where_expr(("lit", test_dict))
    qb.where_expr(("lit", test_uuid))
    qb.where_expr(("lit", test_now))
    qb.where_expr(("lit", test_date))
    qb.where_expr(("lit", test_decimal))

    compiled = qb.compile(dialect="cypher")
    stmt = compiled["statement"]
    assert "WHERE" in stmt
    assert "Person" in stmt


def test_query_expressions_with_expanded_types():
    """Verify that Query DSL serializes date, UUID, Decimal, and dict seamlessly."""
    u = uuid.uuid4()
    d = datetime.date(2026, 1, 1)
    dec = Decimal("99.95")

    q = (
        Query()
        .match()
        .node("n", labels=["Person"])
        .where(col("n.uuid") == lit(u))
        .where(col("n.created") >= lit(d))
        .where(col("n.amount") == lit(dec))
        .return_("n")
    )
    compiled = q.compile("cypher")
    assert "$p0" in compiled.statement
    assert "$p1" in compiled.statement
    assert "$p2" in compiled.statement
    assert compiled.parameters["p0"] == str(u)
    assert compiled.parameters["p1"] == "2026-01-01"
    assert abs(compiled.parameters["p2"] - 99.95) < 1e-6


# ---------------------------------------------------------------------------
# 3. Arrow C Stream Single-Pass Lifecycle Validation
# ---------------------------------------------------------------------------


def test_arrow_stream_single_pass_consumption():
    """Verify that ArrowStream obeys the Arrow C Data Interface single-pass constraint."""
    stream = generate_synthetic_stream(10)
    assert not stream.is_consumed

    # First consumption via Polars DataFrame
    df = pl.DataFrame(stream)
    assert df.height == 10
    assert stream.is_consumed

    # Second consumption must raise RuntimeError
    with pytest.raises(RuntimeError, match="ArrowArrayStream has already been consumed"):
        pl.DataFrame(stream)


def test_arrow_stream_to_dicts_does_not_consume():
    """Verify that calling to_dicts() extracts Python dicts without consuming the C stream."""
    stream = generate_synthetic_stream(5)
    assert not stream.is_consumed

    # to_dicts() does not consume the underlying C stream
    records = stream.to_dicts()
    assert len(records) == 5
    assert not stream.is_consumed

    # Afterwards, the C stream can still be consumed by PyArrow or Polars
    table = pa.RecordBatchReader.from_stream(stream).read_all()
    assert table.num_rows == 5
    assert stream.is_consumed


# ---------------------------------------------------------------------------
# 4. NativeClient Sync & Async Execution
# ---------------------------------------------------------------------------


def test_native_client_invalid_uri():
    """Verify that constructing NativeClient with an invalid URI raises a ValueError."""
    with pytest.raises(ValueError, match="Unsupported database URI scheme"):
        NativeClient("unknown://host:1234")


@pytest.mark.skipif(not FALKORDB_ONLINE, reason="Live FalkorDB container not available")
def test_native_client_falkordb_sync_and_async():
    """Verify synchronous and asynchronous query execution on FalkorDB via NativeClient."""
    import asyncio

    client = NativeClient(FALKORDB_URI, min_idle=1, max_size=5)
    assert client.uri == FALKORDB_URI
    assert "Redis" in client.protocol
    assert client.ping_sync() is True

    # Sync execution
    res_sync = client.execute_sync("RETURN 101 AS sync_val", {})
    assert res_sync.num_rows == 1
    assert res_sync.records == [{"sync_val": 101}]
    assert res_sync.columns == ["sync_val"]
    assert res_sync.execution_time_ms >= 0

    # Async execution
    async def run_async():
        res_async = await client.execute("RETURN 202 AS async_val", {})
        assert res_async.num_rows == 1
        assert res_async.records == [{"async_val": 202}]
        assert await client.ping() is True

    asyncio.run(run_async())
    client.close()


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j container not available")
def test_native_client_neo4j_sync_and_async():
    """Verify synchronous and asynchronous query execution on Neo4j via NativeClient."""
    import asyncio

    client = NativeClient(NEO4J_URI, min_idle=1, max_size=5)
    assert client.uri == NEO4J_URI
    assert client.protocol == "Bolt"
    assert client.ping_sync() is True

    # Sync execution
    res_sync = client.execute_sync("RETURN 303 AS sync_val", {})
    assert res_sync.num_rows == 1
    assert res_sync.records == [{"sync_val": 303}]

    # Async execution
    async def run_async():
        res_async = await client.execute("RETURN 404 AS async_val", {})
        assert res_async.num_rows == 1
        assert res_async.records == [{"async_val": 404}]
        assert await client.ping() is True

    asyncio.run(run_async())
    client.close()


# ---------------------------------------------------------------------------
# 5. Dual-Backend Integration in Session & AsyncSession
# ---------------------------------------------------------------------------


def test_session_backend_selection():
    """Verify backend selection behavior across native, bridge, and auto."""
    # Mock bridge defaults to bridge backend even with auto
    s_mock = Session("mock://memory", backend="auto")
    assert s_mock.backend == "bridge"
    assert s_mock.native_client is None

    # Invalid string URI with backend="native" raises ValueError
    with pytest.raises(ValueError, match="Native backend requires a valid URI string"):
        Session(None, backend="native")

    with pytest.raises(RuntimeError, match="Unsupported database URI scheme"):
        Session("unsupported://127.0.0.1:9999", backend="native")


@pytest.mark.skipif(not FALKORDB_ONLINE, reason="Live FalkorDB container not available")
def test_session_native_falkordb():
    """Verify Session full query workflow using native backend against FalkorDB."""
    with Session(FALKORDB_URI, backend="native") as s:
        assert s.backend == "native"
        assert s.native_client is not None
        assert s.ping() is True

        res = s.execute("RETURN 42 AS answer, 'voyager' AS framework")
        assert res.first() == {"answer": 42, "framework": "voyager"}
        assert res.statement == "RETURN 42 AS answer, 'voyager' AS framework"

        # Conversion to Polars
        df = res.to_polars()
        assert df.shape == (1, 2)
        assert df["answer"][0] == 42
        assert df["framework"][0] == "voyager"

        # Direct execute_to_polars
        df2 = s.execute_to_polars("RETURN 99 AS n")
        assert df2.shape == (1, 1)
        assert df2["n"][0] == 99

        # Direct execute_to_arrow
        tbl = s.execute_to_arrow("RETURN 123 AS num")
        assert tbl.num_rows == 1
        assert tbl.column("num")[0].as_py() == 123


@pytest.mark.skipif(not FALKORDB_ONLINE, reason="Live FalkorDB container not available")
@pytest.mark.asyncio
async def test_async_session_native_falkordb():
    """Verify AsyncSession full query workflow using native backend against FalkorDB."""
    async with AsyncSession(FALKORDB_URI, backend="native") as s:
        assert s.backend == "native"
        assert s.native_client is not None
        assert await s.ping() is True

        res = await s.execute("RETURN 555 AS lucky")
        assert res.first() == {"lucky": 555}

        # Conversion to Arrow
        tbl = res.to_arrow()
        assert tbl.num_rows == 1
        assert tbl.column("lucky")[0].as_py() == 555

        # Direct execute_to_polars
        df = await s.execute_to_polars("RETURN 777 AS val")
        assert df.shape == (1, 1)
        assert df["val"][0] == 777


def test_execution_result_lazy_stream_materialization():
    """Verify that ExecutionResult wraps an ArrowStream lazily without eager dict materialization."""
    from voyager_ogm.session import ExecutionResult

    stream = generate_synthetic_stream(5_000)
    res = ExecutionResult(stream=stream)

    # Stream must NOT be consumed upon __init__
    assert not stream.is_consumed

    # O(1) len(res) inspection reads stream.num_rows without consuming stream
    assert len(res) == 5_000
    assert not stream.is_consumed

    # .to_polars() consumes the stream directly into Polars DataFrame without allocating dicts
    df = res.to_polars()
    assert df.shape == (5_000, 6)
    assert stream.is_consumed

    # Subsequent access (indexing, .first(), .all(), .to_arrow()) uses cached dataframe without re-consuming
    assert res.first()["id"] == 0
    assert res[1]["id"] == 1
    assert len(res.all()) == 5_000
    tbl = res.to_arrow()
    assert tbl.num_rows == 5_000
    assert len(res) == 5_000
