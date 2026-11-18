"""Live regression tests for real database transactions (#177).

Covers Neo4j, Memgraph and Apache AGE through both the Python driver bridge and
the native Rust client. Tests skip automatically when containers are absent.
"""

from __future__ import annotations

import re
import socket
from typing import Any

import pytest
from voyager_ogm import Session

pytestmark = pytest.mark.live

try:
    from neo4j import GraphDatabase

    NEO4J_AVAILABLE = True
except ImportError:
    GraphDatabase = None  # type: ignore[assignment]
    NEO4J_AVAILABLE = False

try:
    import psycopg

    PSYCOPG_AVAILABLE = True
except ImportError:
    psycopg = None  # type: ignore[assignment]
    PSYCOPG_AVAILABLE = False


NEO4J_URI = "bolt://127.0.0.1:7687"
NEO4J_AUTH = ("neo4j", "voyagerpass123")
MEMGRAPH_URI = "bolt://127.0.0.1:7688"
AGE_DSN = (
    "host=127.0.0.1 port=5455 user=postgres password=voyagerpass123 "
    "dbname=voyager_graph connect_timeout=2"
)
AGE_GRAPH = "voyager_tx_graph"


def _port_open(host: str, port: int, timeout: float = 0.5) -> bool:
    """Returns True when a TCP port accepts connections."""
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def _count_value(value: Any) -> int:
    """Normalizes an integer or Apache AGE agtype count to a Python int."""
    if isinstance(value, int):
        return value
    if isinstance(value, dict):
        value = next(iter(value.values()), 0)
    match = re.search(r"-?\d+", str(value))
    return int(match.group(0)) if match else 0


class _Harness:
    """Per-engine statements and cleanup helper."""

    def __init__(self, session: Session, engine: str) -> None:
        self.session = session
        self.engine = engine

    def create(self, name: str) -> str:
        """Returns the CREATE statement for one :TxUser node."""
        if self.engine == "age":
            return (
                f"SELECT * FROM ag_catalog.cypher('{AGE_GRAPH}', $$ "
                f"CREATE (:TxUser {{name: '{name}'}}) $$) AS (res agtype);"
            )
        return f"CREATE (:TxUser {{name: '{name}'}})"

    def count(self) -> int:
        """Returns the number of :TxUser nodes visible on the session."""
        if self.engine == "age":
            statement = (
                f"SELECT * FROM ag_catalog.cypher('{AGE_GRAPH}', $$ "
                "MATCH (n:TxUser) RETURN count(n) $$) AS (count agtype);"
            )
        else:
            statement = "MATCH (n:TxUser) RETURN count(n) AS count"
        records = self.session.execute(statement)
        if not records:
            return 0
        record = records[0]
        if isinstance(record, dict):
            return _count_value(next(iter(record.values()), 0))
        return _count_value(record)

    def cleanup(self) -> None:
        """Deletes all :TxUser nodes for this engine."""
        if self.engine == "age":
            self.session.execute(
                f"SELECT * FROM ag_catalog.cypher('{AGE_GRAPH}', $$ "
                "MATCH (n:TxUser) DETACH DELETE n $$) AS (res agtype);"
            )
        else:
            self.session.execute("MATCH (n:TxUser) DETACH DELETE n")


def _ensure_age_graph() -> None:
    """Creates the AGE extension and test graph when they do not exist."""
    assert psycopg is not None
    conn = psycopg.connect(AGE_DSN, autocommit=True)
    try:
        with conn.cursor() as cur:
            cur.execute("CREATE EXTENSION IF NOT EXISTS age;")
            cur.execute("LOAD 'age';")
            cur.execute('SET search_path = ag_catalog, "$user", public;')
            cur.execute("SELECT count(*) FROM ag_graph WHERE name = %s;", (AGE_GRAPH,))
            if _count_value(cur.fetchone()[0]) == 0:
                cur.execute("SELECT create_graph(%s);", (AGE_GRAPH,))
    finally:
        conn.close()


def _make_session(engine: str, path: str) -> Session:
    """Builds a live session for the engine and execution path."""
    if path == "bridge":
        if engine == "neo4j":
            assert GraphDatabase is not None
            driver = GraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH, connection_timeout=2.0)
            return Session(bridge=driver, dialect="cypher")
        if engine == "memgraph":
            assert GraphDatabase is not None
            driver = GraphDatabase.driver(MEMGRAPH_URI, auth=("", ""), connection_timeout=2.0)
            return Session(bridge=driver, dialect="cypher")
        assert psycopg is not None
        conn = psycopg.connect(AGE_DSN, autocommit=True)
        return Session(bridge=conn, dialect="cypher")

    if engine == "neo4j":
        uri = "bolt://neo4j:voyagerpass123@127.0.0.1:7687?connect_timeout=2"
    elif engine == "memgraph":
        uri = "bolt://127.0.0.1:7688?connect_timeout=2"
    else:
        uri = "age://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph?connect_timeout=2"
    return Session(bridge=uri, dialect="cypher", backend="native")


def _failing_statement(engine: str) -> str:
    """Returns a statement guaranteed to fail inside a transaction."""
    if engine == "age":
        return "SELECT 1/0;"
    # Syntax error on Neo4j and Memgraph.
    return "MATCH (n RETURN n"


def _require_engine(engine: str) -> None:
    """Skips when the engine container is not reachable."""
    if engine == "neo4j":
        if not NEO4J_AVAILABLE:
            pytest.skip("neo4j driver not installed")
        if not _port_open("127.0.0.1", 7687):
            pytest.skip("Neo4j container not available on port 7687")
    elif engine == "memgraph":
        if not NEO4J_AVAILABLE:
            pytest.skip("neo4j driver not installed")
        if not _port_open("127.0.0.1", 7688):
            pytest.skip("Memgraph container not available on port 7688")
    else:
        if not PSYCOPG_AVAILABLE:
            pytest.skip("psycopg not installed")
        if not _port_open("127.0.0.1", 5455):
            pytest.skip("Apache AGE container not available on port 5455")
        _ensure_age_graph()


@pytest.mark.parametrize("engine", ["neo4j", "memgraph", "age"])
@pytest.mark.parametrize("path", ["bridge", "native"])
def test_live_transaction_rollback_and_commit(engine: str, path: str) -> None:
    """The #177 regression: rollback removes the write, commit persists exactly one."""
    _require_engine(engine)
    session = _make_session(engine, path)
    harness = _Harness(session, engine)
    try:
        harness.cleanup()

        with pytest.raises(RuntimeError, match="boom"):
            with session.transaction():
                session.execute(harness.create("Alice"))
                assert harness.count() == 1, f"{engine}/{path} did not see its own write"
                raise RuntimeError("boom")

        assert harness.count() == 0, f"{engine}/{path} kept the rolled-back :TxUser"

        with session.transaction():
            session.execute(harness.create("Alice"))

        assert harness.count() == 1, f"{engine}/{path} did not commit exactly one :TxUser"
    finally:
        try:
            harness.cleanup()
        finally:
            session.close()


@pytest.mark.parametrize("engine", ["neo4j", "memgraph", "age"])
@pytest.mark.parametrize("path", ["bridge", "native"])
def test_live_failed_statement_clean_exit_raises_and_rolls_back(engine: str, path: str) -> None:
    """A caught statement error makes clean-exit commit raise and lose no data."""
    _require_engine(engine)
    session = _make_session(engine, path)
    harness = _Harness(session, engine)
    try:
        harness.cleanup()

        with pytest.raises(Exception, match="abort"):
            with session.transaction():
                session.execute(harness.create("CleanExitAlice"))
                try:
                    session.execute(_failing_statement(engine))
                except Exception:
                    pass

        assert harness.count() == 0, f"{engine}/{path} committed data after a failed statement"
    finally:
        try:
            harness.cleanup()
        finally:
            session.close()


@pytest.mark.parametrize("engine", ["neo4j", "memgraph", "age"])
@pytest.mark.parametrize("path", ["bridge", "native"])
def test_live_failed_statement_then_write_raises_aborted(engine: str, path: str) -> None:
    """Writes after a failed statement are rejected instead of autocommitting."""
    _require_engine(engine)
    session = _make_session(engine, path)
    harness = _Harness(session, engine)
    try:
        harness.cleanup()

        with pytest.raises(Exception, match="abort"):
            with session.transaction():
                session.execute(harness.create("BeforeFailure"))
                try:
                    session.execute(_failing_statement(engine))
                except Exception:
                    pass
                session.execute(harness.create("AfterFailure"))

        assert harness.count() == 0, f"{engine}/{path} kept a write issued after failure"
        assert session._active_transaction is None
    finally:
        try:
            harness.cleanup()
        finally:
            session.close()


@pytest.mark.parametrize("engine", ["neo4j", "memgraph", "age"])
@pytest.mark.parametrize("path", ["bridge", "native"])
def test_live_failed_statement_original_exception_surfaces(engine: str, path: str) -> None:
    """An uncaught statement error surfaces unchanged and the transaction rolls back."""
    _require_engine(engine)
    session = _make_session(engine, path)
    harness = _Harness(session, engine)
    try:
        harness.cleanup()

        with pytest.raises(Exception) as excinfo:
            with session.transaction():
                session.execute(harness.create("OriginalError"))
                session.execute(_failing_statement(engine))

        assert "aborted" not in str(excinfo.value).lower(), (
            f"{engine}/{path} masked the statement error with a rollback error: {excinfo.value}"
        )
        assert harness.count() == 0, f"{engine}/{path} kept data after an uncaught failure"
    finally:
        try:
            harness.cleanup()
        finally:
            session.close()


@pytest.mark.parametrize("engine", ["neo4j", "memgraph"])
def test_live_bolt_transactions_savepoints_raise(engine: str) -> None:
    """Bolt engines raise a clear error for savepoints."""
    _require_engine(engine)
    session = _make_session(engine, "bridge")
    try:
        with session.transaction() as tx:
            assert not tx.supports_savepoints
            with pytest.raises(RuntimeError, match="savepoint"):
                tx.savepoint("sp_bolt")
    finally:
        session.close()


@pytest.mark.parametrize("path", ["bridge", "native"])
def test_live_age_savepoint_rollback(path: str) -> None:
    """Apache AGE/PostgreSQL savepoints roll back only the nested write."""
    _require_engine("age")
    session = _make_session("age", path)
    harness = _Harness(session, "age")
    try:
        harness.cleanup()

        with session.transaction() as tx:
            session.execute(harness.create("Committed"))
            try:
                with tx.savepoint("sp_tx"):
                    session.execute(harness.create("NestedRollback"))
                    raise RuntimeError("nested failure")
            except RuntimeError:
                pass
            assert tx.is_active
            assert harness.count() == 1

        assert harness.count() == 1
    finally:
        try:
            harness.cleanup()
        finally:
            session.close()
