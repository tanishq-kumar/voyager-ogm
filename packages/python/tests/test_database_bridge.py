"""Unit and integration tests for the Database Bridging Layer and driver adapters."""

from __future__ import annotations

from typing import Any

try:
    import duckdb
except ImportError:
    duckdb = None

import polars as pl
import pytest
from voyager_ogm import (
    AsyncMockBridge,
    AsyncNeo4jBoltBridge,
    AsyncPostgresBridge,
    AsyncSession,
    DuckDbBridge,
    Field,
    MockBridge,
    Neo4jBoltBridge,
    Node,
    PostgresBridge,
    Query,
    Session,
    create_bridge,
    node,
    register_bridge,
    reset_alias_counters,
)


@node
class Person(Node):
    id: int = Field(primary_key=True)
    name: str
    age: int


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


def test_mock_bridge_query_recording():
    """Test MockBridge records executed statements and parameters."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher")

    p = Person()
    q = Query.match(p).filter(p.age > 21)
    res = session.execute(q)

    assert res == []
    assert len(bridge.executed_queries) == 1
    stmt, params = bridge.executed_queries[0]
    assert stmt == "MATCH (_person_0:Person) WHERE _person_0.age > $p0"
    assert params == {"p0": 21}


def test_mock_bridge_canned_results_and_polars():
    """Test MockBridge returning canned dicts and Polars DataFrames."""
    bridge = MockBridge()
    canned_data = [{"id": 1, "name": "Alice", "age": 30}, {"id": 2, "name": "Bob", "age": 35}]
    bridge.queue_result(canned_data)

    session = Session(bridge=bridge, dialect="cypher")
    records = session.execute("MATCH (p:Person) RETURN p")
    assert records == canned_data

    # Queue Polars DataFrame
    canned_df = pl.DataFrame(canned_data)
    bridge.queue_result(canned_df)

    df_res = session.execute_to_polars("MATCH (p:Person) RETURN p")
    assert isinstance(df_res, pl.DataFrame)
    assert len(df_res) == 2
    assert df_res["name"].to_list() == ["Alice", "Bob"]


def test_mock_bridge_bulk_run():
    """Test session.run_bulk executing bulk ingestion plan across MockBridge."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher")

    data = [{"id": i, "name": f"User_{i}", "age": 20 + i} for i in range(100)]
    plan = session.bulk_create(Person, data, batch_size=25)

    result = session.run_bulk(plan)

    assert result.total_batches == 4
    assert result.total_records == 100
    assert len(bridge.executed_queries) == 4
    assert result.statement == (
        "UNWIND $batch AS row CREATE (_person_0:Person) "
        "SET _person_0.id = row.id, _person_0.name = row.name, _person_0.age = row.age"
    )


@pytest.mark.asyncio
async def test_async_session_and_async_mock_bridge():
    """Test AsyncSession and AsyncMockBridge async/await query execution."""
    bridge = AsyncMockBridge()
    bridge.queue_result([{"id": 10, "name": "Charlie", "age": 40}])

    session = AsyncSession(bridge=bridge, dialect="cypher")
    p = Person()
    records = await session.execute(Query.match(p).filter(p.name == "Charlie"))

    assert len(records) == 1
    assert records[0]["name"] == "Charlie"
    assert len(bridge.executed_queries) == 1

    # Test async bulk ingestion
    data = [{"id": 1, "name": "A", "age": 20}, {"id": 2, "name": "B", "age": 21}]
    plan = session.bulk_create(Person, data, batch_size=1)

    bulk_result = await session.run_bulk(plan)
    assert bulk_result.total_batches == 2
    assert bulk_result.total_records == 2
    assert len(bridge.executed_queries) == 3


def test_duckdb_bridge_live_execution():
    """Test DuckDbBridge executing queries on a live in-memory DuckDB connection."""
    if duckdb is None:
        pytest.skip("duckdb is not installed in this environment")
    con = duckdb.connect(":memory:")
    con.execute("CREATE TABLE person (id INTEGER, name VARCHAR, age INTEGER);")
    con.execute("INSERT INTO person VALUES (1, 'Alice', 28), (2, 'Bob', 32);")

    bridge = DuckDbBridge(con)
    session = Session(bridge=bridge, dialect="sql_pgq")

    # Direct query execution
    records = session.execute("SELECT * FROM person ORDER BY id;")
    assert len(records) == 2
    assert records[0]["name"] == "Alice"
    assert records[1]["name"] == "Bob"

    # Native Polars extraction
    df = session.execute_to_polars("SELECT * FROM person WHERE age > 30;")
    assert isinstance(df, pl.DataFrame)
    assert len(df) == 1
    assert df["name"][0] == "Bob"


@pytest.mark.asyncio
async def test_async_duckdb_bridge_live_execution():
    """Test AsyncDuckDbBridge non-blocking async execution over DuckDB."""
    if duckdb is None:
        pytest.skip("duckdb is not installed in this environment")
    con = duckdb.connect(":memory:")
    con.execute("CREATE TABLE users (id INTEGER, username VARCHAR);")
    con.execute("INSERT INTO users VALUES (101, 'admin'), (102, 'guest');")

    session = AsyncSession(bridge=con, dialect="sql_pgq")
    records = await session.execute("SELECT * FROM users WHERE id = 101;")

    assert len(records) == 1
    assert records[0]["username"] == "admin"

    df = await session.execute_to_polars("SELECT * FROM users ORDER BY id;")
    assert len(df) == 2


def test_neo4j_sync_bolt_bridge_simulation():
    """Test Neo4jBoltBridge wraps a mock Neo4j sync driver."""

    class MockNeo4jRecord:
        def __init__(self, data: dict[str, Any]):
            self._data = data

        def data(self):
            return self._data

    class MockNeo4jSession:
        def __init__(self):
            self.calls = []

        def __enter__(self):
            return self

        def __exit__(self, *args):
            pass

        def run(self, stmt, params):
            self.calls.append((stmt, params))
            return [MockNeo4jRecord({"n": {"name": "Neo", "age": 30}})]

    class MockNeo4jDriver:
        def __init__(self):
            self.session_inst = MockNeo4jSession()

        def session(self, **kwargs):
            return self.session_inst

    mock_driver = MockNeo4jDriver()
    bridge = Neo4jBoltBridge(mock_driver)
    session = Session(bridge=bridge, dialect="cypher")

    results = session.execute("MATCH (n:Person) RETURN n", {"limit": 10})
    assert len(results) == 1
    assert results[0]["n"]["name"] == "Neo"
    assert len(mock_driver.session_inst.calls) == 1
    assert mock_driver.session_inst.calls[0] == ("MATCH (n:Person) RETURN n", {"limit": 10})


@pytest.mark.asyncio
async def test_neo4j_async_bolt_bridge_simulation():
    """Test AsyncNeo4jBoltBridge wraps a mock Neo4j async driver."""

    class MockAsyncResult:
        def __init__(self, data: list[dict[str, Any]]):
            self._data = data

        async def data(self):
            return self._data

    class MockAsyncNeo4jSession:
        def __init__(self):
            self.calls = []

        async def __aenter__(self):
            return self

        async def __aexit__(self, *args):
            pass

        async def run(self, stmt, params):
            self.calls.append((stmt, params))
            return MockAsyncResult([{"p": {"name": "Trinity"}}])

    class MockAsyncNeo4jDriver:
        def __init__(self):
            self.session_inst = MockAsyncNeo4jSession()

        def session(self, **kwargs):
            return self.session_inst

    async_driver = MockAsyncNeo4jDriver()
    bridge = AsyncNeo4jBoltBridge(async_driver)
    session = AsyncSession(bridge=bridge, dialect="cypher")

    records = await session.execute("MATCH (p:Person) RETURN p")
    assert len(records) == 1
    assert records[0]["p"]["name"] == "Trinity"
    assert len(async_driver.session_inst.calls) == 1


def test_dynamic_bridge_registration():
    """Test registering a custom third-party driver adapter into Voyager's bridge registry."""

    class CustomDatabaseClient:
        def __init__(self):
            self.history = []

        def query(self, sql):
            self.history.append(sql)
            return [{"custom_key": "custom_val"}]

    class CustomClientBridge:
        def __init__(self, client: CustomDatabaseClient):
            self.client = client

        def execute(self, statement: str, parameters: dict[str, Any] | None = None):
            return self.client.query(statement)

        def execute_to_polars(self, statement: str, parameters: dict[str, Any] | None = None):
            return pl.DataFrame(self.execute(statement, parameters))

        def execute_bulk(self, plan_or_statement, batches=None):
            pass

        def close(self):
            pass

    # Register the custom bridge
    register_bridge(CustomDatabaseClient, CustomClientBridge, is_async=False)

    client = CustomDatabaseClient()
    session = Session(bridge=client)

    res = session.execute("CUSTOM GRAPH QUERY")
    assert res == [{"custom_key": "custom_val"}]
    assert client.history == ["CUSTOM GRAPH QUERY"]


def test_postgres_sync_bridge_simulation():
    """Test PostgresBridge wraps a mock psycopg/PostgreSQL connection."""

    class MockPgCursor:
        def __init__(self):
            self.executed = []
            self.description = [("id",), ("name",), ("city",)]

        def __enter__(self):
            return self

        def __exit__(self, *args):
            pass

        def execute(self, stmt, params=None):
            self.executed.append((stmt, params))

        def fetchall(self):
            return [(1, "Alice", "London"), (2, "Bob", "Berlin")]

    class MockPgConnection:
        __module__ = "psycopg.connection"
        __qualname__ = "Connection"

        def __init__(self):
            self.cursor_inst = MockPgCursor()
            self.closed = False

        def cursor(self):
            return self.cursor_inst

        def close(self):
            self.closed = True

    mock_conn = MockPgConnection()
    bridge = PostgresBridge(mock_conn)
    session = Session(bridge=bridge, dialect="sql_pgq")

    # 1. Test GRAPH_TABLE query with parameters (parameter inlining)
    pgq_stmt = (
        "SELECT * FROM GRAPH_TABLE (g MATCH (p IS Person) WHERE p.city = $p0 COLUMNS (p.name))"
    )
    records = session.execute(pgq_stmt, {"p0": "London"})
    assert len(records) == 2
    assert records[0]["name"] == "Alice"
    assert len(mock_conn.cursor_inst.executed) == 1
    executed_stmt, executed_params = mock_conn.cursor_inst.executed[0]
    assert "$p0" not in executed_stmt
    assert "'London'" in executed_stmt
    assert executed_params == ()

    # 2. Test standard SQL query with parameters ($p0 -> %s translation)
    sql_stmt = "SELECT * FROM person WHERE city = $p0 AND age >= $p1"
    df = session.execute_to_polars(sql_stmt, {"p0": "London", "p1": 25})
    assert isinstance(df, pl.DataFrame)
    assert len(df) == 2
    executed_stmt2, executed_params2 = mock_conn.cursor_inst.executed[1]
    assert executed_stmt2 == "SELECT * FROM person WHERE city = %s AND age >= %s"
    assert executed_params2 == ["London", 25]


@pytest.mark.asyncio
async def test_postgres_async_bridge_simulation():
    """Test AsyncPostgresBridge wraps a mock psycopg/PostgreSQL connection asynchronously."""

    class MockPgCursor:
        def __init__(self):
            self.description = [("val",)]

        def __enter__(self):
            return self

        def __exit__(self, *args):
            pass

        def execute(self, stmt, params=None):
            pass

        def fetchall(self):
            return [(42,)]

    class MockPgConnection:
        __module__ = "psycopg.connection"
        __qualname__ = "Connection"

        def cursor(self):
            return MockPgCursor()

    mock_conn = MockPgConnection()
    bridge = AsyncPostgresBridge(mock_conn)
    session = AsyncSession(bridge=bridge, dialect="sql_pgq")

    records = await session.execute("SELECT 42 AS val")
    assert len(records) == 1
    assert records[0]["val"] == 42

    df = await session.execute_to_polars("SELECT 42 AS val")
    assert isinstance(df, pl.DataFrame)
    assert df["val"][0] == 42


def test_bridge_registry_and_uri_auto_resolution():
    """Test that create_bridge auto-detects driver types and resolves URI schemes."""
    # 1. Null / Mock fallbacks
    assert isinstance(create_bridge(None), MockBridge)
    assert isinstance(create_bridge(None, is_async=True), AsyncMockBridge)
    assert isinstance(create_bridge(MockBridge()), MockBridge)
    assert isinstance(create_bridge(AsyncMockBridge(), is_async=True), AsyncMockBridge)
    assert isinstance(create_bridge("mock://test"), MockBridge)
    assert isinstance(create_bridge("mock://test", is_async=True), AsyncMockBridge)

    # 2. DuckDB connection auto-detection
    class FakeDuckDb:
        __module__ = "duckdb.duckdb"
        __qualname__ = "DuckDBPyConnection"

        def execute(self, *args):
            pass

    duck_inst = FakeDuckDb()
    assert isinstance(create_bridge(duck_inst), DuckDbBridge)

    # 3. PostgreSQL connection auto-detection
    class FakePsycopg:
        __module__ = "psycopg.connection"
        __qualname__ = "Connection"

        def cursor(self):
            pass

    pg_inst = FakePsycopg()
    assert isinstance(create_bridge(pg_inst), PostgresBridge)
    assert isinstance(create_bridge(pg_inst, is_async=True), AsyncPostgresBridge)

    # 4. Neo4j driver auto-detection
    class FakeNeo4jDriver:
        __module__ = "neo4j._sync.driver"
        __qualname__ = "Driver"

        def session(self):
            pass

    neo_inst = FakeNeo4jDriver()
    assert isinstance(create_bridge(neo_inst), Neo4jBoltBridge)


def test_postgres_bridge_execute_bulk_with_bulk_ingestion_plan():
    """Test PostgresBridge.execute_bulk correctly processes batches from BulkIngestionPlan (Issue #72)."""
    from unittest.mock import MagicMock

    from voyager_ogm.ingestion import create_bulk_create_plan

    mock_conn = MagicMock()
    bridge = PostgresBridge(mock_conn)

    data = [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]
    plan = create_bulk_create_plan(Person, data, batch_size=1, dialect="sql_pgq")

    res = bridge.execute_bulk(plan)
    assert res.total_batches == 2
    assert res.total_records == 2
    assert mock_conn.cursor.return_value.__enter__.return_value.execute.call_count == 2


def test_bridge_parameter_formatting_prefix_collision_safety():
    """Test parameter prefix collision safety ($p vs $p0) in DuckDbBridge and PostgresBridge (Issue #73)."""
    from unittest.mock import MagicMock

    mock_conn = MagicMock()
    duck_bridge = DuckDbBridge(mock_conn)
    pg_bridge = PostgresBridge(mock_conn)

    stmt = "SELECT * FROM GRAPH_TABLE(g MATCH (p) WHERE p.name = $p AND p.age > $p0 COLUMNS (p.id))"
    params = {"p": "Alice", "p0": 30}

    formatted_duck, _ = duck_bridge._format_pgq_statement(stmt, params)
    assert "p.name = 'Alice'" in formatted_duck
    assert "p.age > 30" in formatted_duck
    assert "'Alice'0" not in formatted_duck

    formatted_pg, _ = pg_bridge._format_pgq_statement(stmt, params)
    assert "p.name = 'Alice'" in formatted_pg
    assert "p.age > 30" in formatted_pg
    assert "'Alice'0" not in formatted_pg


def test_postgres_bridge_multiple_occurrences_parameter_count():
    """Test multiple occurrences of a parameter in standard SQL match ordered_params length (Issue #73)."""
    from unittest.mock import MagicMock

    mock_conn = MagicMock()
    pg_bridge = PostgresBridge(mock_conn)

    stmt = "SELECT * FROM users WHERE age >= $p0 AND max_age <= $p0"
    params = {"p0": 25}

    formatted_stmt, ordered_params = pg_bridge._format_pgq_statement(stmt, params)
    assert formatted_stmt == "SELECT * FROM users WHERE age >= %s AND max_age <= %s"
    assert ordered_params == [25, 25]


def test_postgres_bridge_asyncpg_connection_not_matched():
    """Test asyncpg Connection is not matched as synchronous PostgresBridge (Issue #74)."""
    from voyager_ogm.bridge import _is_postgres_conn

    class FakeAsyncpg:
        __module__ = "asyncpg.connection"
        __qualname__ = "Connection"

        def cursor(self):
            pass

    conn = FakeAsyncpg()
    assert _is_postgres_conn(conn) is False


def test_session_runtime_parameters_merging():
    """Test session.execute and query.execute retain dynamically supplied runtime parameters."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher")

    p = Person()
    q = Query.match(p).filter(p.age > 21)

    # 1. session.execute(query, parameters={...})
    session.execute(q, parameters={"runtime_limit": 10})
    stmt, params = bridge.executed_queries[-1]
    assert stmt == "MATCH (_person_0:Person) WHERE _person_0.age > $p0"
    assert params["p0"] == 21
    assert params["runtime_limit"] == 10

    # 2. query.execute(session, parameters={...})
    q.execute(session, parameters={"page_size": 50})
    stmt, params = bridge.executed_queries[-1]
    assert params["p0"] == 21
    assert params["page_size"] == 50

    # 3. session.execute(compiled_query, parameters={...})
    compiled = q.compile(dialect="cypher")
    session.execute(compiled, parameters={"offset": 100})
    stmt, params = bridge.executed_queries[-1]
    assert params["p0"] == 21
    assert params["offset"] == 100


@pytest.mark.asyncio
async def test_async_session_runtime_parameters_merging():
    """Test AsyncSession.execute retains dynamically supplied runtime parameters."""
    bridge = AsyncMockBridge()
    session = AsyncSession(bridge=bridge, dialect="cypher")

    p = Person()
    q = Query.match(p).filter(p.age > 25)

    await session.execute(q, parameters={"runtime_limit": 5})
    stmt, params = bridge.executed_queries[-1]
    assert stmt == "MATCH (_person_0:Person) WHERE _person_0.age > $p0"
    assert params["p0"] == 25
    assert params["runtime_limit"] == 5

    compiled = q.compile(dialect="cypher")
    await session.execute(compiled, parameters={"offset": 20})
    stmt, params = bridge.executed_queries[-1]
    assert params["p0"] == 25
    assert params["offset"] == 20


def test_session_ping_dialect_awareness():
    """Test Session.ping uses appropriate probing syntax per dialect."""
    from unittest.mock import MagicMock

    class FallbackBridge(MockBridge):
        ping = None  # Explicitly disable ping method to test SQL/Cypher fallback execution

    # Fallback with Cypher uses RETURN 1
    bridge_cypher = FallbackBridge()
    session_cypher = Session(bridge=bridge_cypher, dialect="cypher")
    assert session_cypher.ping() is True
    assert bridge_cypher.executed_queries[-1][0] == "RETURN 1"

    # Fallback with SQL/PGQ uses SELECT 1
    bridge_pgq = FallbackBridge()
    session_pgq = Session(bridge=bridge_pgq, dialect="sql_pgq")
    assert session_pgq.ping() is True
    assert bridge_pgq.executed_queries[-1][0] == "SELECT 1"

    # Apache AGE uses SELECT 1
    bridge_age = FallbackBridge()
    session_age = Session(bridge=bridge_age, dialect="age")
    assert session_age.ping() is True
    assert bridge_age.executed_queries[-1][0] == "SELECT 1"

    # MockBridge ping
    mock_bridge = MockBridge()
    session_mock = Session(bridge=mock_bridge, dialect="cypher")
    assert session_mock.ping() is True

    # DuckDB bridge ping
    mock_duckdb_con = MagicMock()
    duck_bridge = DuckDbBridge(mock_duckdb_con)
    session_duck = Session(bridge=duck_bridge, dialect="sql_pgq")
    assert session_duck.ping() is True
    mock_duckdb_con.execute.assert_called_with("SELECT 1")

    # Postgres bridge ping
    mock_pg_conn = MagicMock()
    pg_bridge = PostgresBridge(mock_pg_conn)
    session_pg = Session(bridge=pg_bridge, dialect="sql_pgq")
    assert session_pg.ping() is True
    mock_pg_conn.cursor.return_value.__enter__.return_value.execute.assert_called_with("SELECT 1")


@pytest.mark.asyncio
async def test_async_session_ping_dialect_awareness():
    """Test AsyncSession.ping uses appropriate probing syntax per dialect."""

    class AsyncFallbackBridge(AsyncMockBridge):
        ping = None  # Explicitly disable ping method to test SQL/Cypher fallback execution

    bridge_cypher = AsyncFallbackBridge()
    session_cypher = AsyncSession(bridge=bridge_cypher, dialect="cypher")
    assert await session_cypher.ping() is True
    assert bridge_cypher.executed_queries[-1][0] == "RETURN 1"

    bridge_pgq = AsyncFallbackBridge()
    session_pgq = AsyncSession(bridge=bridge_pgq, dialect="sql_pgq")
    assert await session_pgq.ping() is True
    assert bridge_pgq.executed_queries[-1][0] == "SELECT 1"

    bridge_age = AsyncFallbackBridge()
    session_age = AsyncSession(bridge=bridge_age, dialect="age")
    assert await session_age.ping() is True
    assert bridge_age.executed_queries[-1][0] == "SELECT 1"


def test_session_parameter_collision_warning():
    """Test that runtime parameter key collisions with compiled parameters emit a UserWarning."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher")

    q = Query.match(Node("Person", name="Alice")).where(Node("Person").age > 25)
    compiled = q.compile(dialect="cypher")
    assert "p0" in compiled.parameters

    with pytest.warns(UserWarning, match=r"Runtime parameter key collision detected: \['p0'\]"):
        session.execute(compiled, parameters={"p0": 999})

    _, params = bridge.executed_queries[-1]
    assert params["p0"] == 999


@pytest.mark.asyncio
async def test_async_session_parameter_collision_warning():
    """Test AsyncSession parameter key collision emits a UserWarning."""
    bridge = AsyncMockBridge()
    session = AsyncSession(bridge=bridge, dialect="cypher")

    q = Query.match(Node("Person", name="Bob")).where(Node("Person").age > 30)
    compiled = q.compile(dialect="cypher")
    assert "p0" in compiled.parameters

    with pytest.warns(UserWarning, match=r"Runtime parameter key collision detected: \['p0'\]"):
        await session.execute(compiled, parameters={"p0": 42})

    _, params = bridge.executed_queries[-1]
    assert params["p0"] == 42


def test_session_ping_bridge_exception_contract():
    """Test that Session.ping returns False when bridge.ping() raises an exception."""

    class RaisingBridge(MockBridge):
        def ping(self) -> bool:
            raise ConnectionResetError("Connection lost to database")

    bridge = RaisingBridge()
    session = Session(bridge=bridge, dialect="cypher")
    assert session.ping() is False


@pytest.mark.asyncio
async def test_async_session_ping_bridge_exception_contract():
    """Test that AsyncSession.ping returns False when async bridge.ping() raises an exception."""

    class AsyncRaisingBridge(AsyncMockBridge):
        async def ping(self) -> bool:
            raise ConnectionResetError("Async connection lost to database")

    bridge = AsyncRaisingBridge()
    session = AsyncSession(bridge=bridge, dialect="cypher")
    assert await session.ping() is False
