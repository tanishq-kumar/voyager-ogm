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
    AsyncSession,
    DuckDbBridge,
    Field,
    MockBridge,
    Neo4jBoltBridge,
    Node,
    Query,
    Session,
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
