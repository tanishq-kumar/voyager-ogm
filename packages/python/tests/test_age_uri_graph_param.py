"""Regression tests for ``?graph=`` / ``?graph_name=`` selectors on PostgreSQL and AGE URIs.

``create_bridge()`` consumes those parameters to choose an Apache AGE graph and must
remove them before handing the URI to psycopg, which rejects unknown libpq keywords.
The live tests select a non-default graph only through the URI and prove the write
landed in that graph (and not in ``age_graph``).
"""

from __future__ import annotations

import contextlib
import socket
import uuid
from typing import TYPE_CHECKING, Any
from unittest.mock import MagicMock

import pytest
from voyager_ogm import Field, NativeClient, Node, Query, Session, VoyagerError, create_bridge

if TYPE_CHECKING:
    from collections.abc import Iterator

AGE_HOST = "127.0.0.1"
AGE_PORT = 5455
AGE_DATABASE_URI = f"postgresql://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}/voyager_graph"


class MockCursor:
    """Mock psycopg cursor returning a configurable ``fetchone`` result."""

    def __init__(self, fetchone_return: Any = None) -> None:
        self.executed: list[tuple[str, Any]] = []
        self._fetchone_return = fetchone_return
        self.description = [("col",)]

    def __enter__(self) -> MockCursor:
        return self

    def __exit__(self, *args: Any) -> None:
        pass

    def execute(self, stmt: str, params: Any = None) -> None:
        self.executed.append((stmt, params))

    def fetchone(self) -> Any:
        return self._fetchone_return

    def fetchall(self) -> list[Any]:
        return []


class MockConnection:
    """Mock psycopg connection that records AGE setup statements."""

    __module__ = "psycopg.connection"
    __qualname__ = "Connection"

    def __init__(self, fetchone_return: Any = None) -> None:
        self.cursor_inst = MockCursor(fetchone_return=fetchone_return)
        self.closed = False
        self._voyager_age_initialized = False

    def cursor(self) -> MockCursor:
        return self.cursor_inst

    def close(self) -> None:
        self.closed = True


def _mock_psycopg_connect(monkeypatch: pytest.MonkeyPatch) -> MagicMock:
    """Replaces ``psycopg.connect`` with a mock returning a recording connection."""
    pytest.importorskip("psycopg")  # not installed on Python 3.14 (see pyproject.toml markers)
    import psycopg

    mock_connect = MagicMock()
    mock_connect.return_value = MockConnection()
    monkeypatch.setattr(psycopg, "connect", mock_connect)
    return mock_connect


# ---------------------------------------------------------------------------
# Offline: the connection string handed to psycopg
# ---------------------------------------------------------------------------


def test_postgres_uri_graph_param_is_stripped(monkeypatch: pytest.MonkeyPatch) -> None:
    """A PostgreSQL URI sends no graph selector to psycopg and keeps other parameters."""
    mock_connect = _mock_psycopg_connect(monkeypatch)

    bridge = create_bridge(f"{AGE_DATABASE_URI}?graph=age_graph&sslmode=disable&connect_timeout=5")

    mock_connect.assert_called_once()
    uri = mock_connect.call_args.args[0]
    assert uri == (
        f"postgresql://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}"
        "/voyager_graph?sslmode=disable&connect_timeout=5"
    )
    assert "graph" not in uri.split("?", 1)[1]
    assert getattr(bridge, "graph_name", None) == "age_graph"


def test_age_uri_graph_name_param_is_stripped(monkeypatch: pytest.MonkeyPatch) -> None:
    """An age:// URI is rewritten to postgresql:// without graph_name and keeps the rest."""
    mock_connect = _mock_psycopg_connect(monkeypatch)

    bridge = create_bridge(
        f"age://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}"
        "/voyager_graph?graph_name=tenant_graph&connect_timeout=5"
    )

    mock_connect.assert_called_once()
    uri = mock_connect.call_args.args[0]
    assert uri.startswith("postgresql://")
    assert uri == (
        f"postgresql://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}"
        "/voyager_graph?connect_timeout=5"
    )
    assert "graph_name" not in uri
    assert getattr(bridge, "graph_name", None) == "tenant_graph"


def test_async_bridge_uri_graph_param_is_stripped(monkeypatch: pytest.MonkeyPatch) -> None:
    """The async bridge path also removes graph/graph_name before psycopg.connect."""
    mock_connect = _mock_psycopg_connect(monkeypatch)

    bridge = create_bridge(f"{AGE_DATABASE_URI}?graph=async_graph&connect_timeout=5", is_async=True)

    mock_connect.assert_called_once()
    uri = mock_connect.call_args.args[0]
    assert uri == (
        f"postgresql://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}"
        "/voyager_graph?connect_timeout=5"
    )
    assert getattr(bridge, "graph_name", None) == "async_graph"


def test_explicit_graph_name_argument_wins_over_uri(monkeypatch: pytest.MonkeyPatch) -> None:
    """An explicit graph_name argument takes precedence over the URI parameter."""
    mock_connect = _mock_psycopg_connect(monkeypatch)

    bridge = create_bridge(
        f"{AGE_DATABASE_URI}?graph=uri_graph&connect_timeout=5",
        graph_name="explicit_graph",
    )

    uri = mock_connect.call_args.args[0]
    assert "graph=" not in uri and "graph_name=" not in uri
    assert getattr(bridge, "graph_name", None) == "explicit_graph"


def test_graph_uri_param_wins_over_graph_name_uri_param(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Existing precedence is kept: ?graph= is preferred over ?graph_name=."""
    mock_connect = _mock_psycopg_connect(monkeypatch)

    bridge = create_bridge(f"{AGE_DATABASE_URI}?graph=primary_graph&graph_name=secondary_graph")

    uri = mock_connect.call_args.args[0]
    assert "?" not in uri
    assert getattr(bridge, "graph_name", None) == "primary_graph"


def test_uri_without_graph_param_keeps_graph_name_none(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Without a graph selector the bridge falls back to its default graph name."""
    mock_connect = _mock_psycopg_connect(monkeypatch)

    bridge = create_bridge(f"{AGE_DATABASE_URI}?connect_timeout=5")

    uri = mock_connect.call_args.args[0]
    assert uri == (
        f"postgresql://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}"
        "/voyager_graph?connect_timeout=5"
    )
    assert getattr(bridge, "graph_name", None) is None


# ---------------------------------------------------------------------------
# Offline: VoyagerError.skipped is a declared, always-present attribute
# ---------------------------------------------------------------------------


def test_voyager_error_skipped_is_declared_and_mutable() -> None:
    """``err.skipped`` is accessible without getattr and defaults to an empty list."""
    err: VoyagerError = VoyagerError("enterprise-only constraints", code="SCHEMA_ERROR")
    assert err.skipped == []
    err.skipped = ["CREATE CONSTRAINT ... IS NOT NULL"]
    assert err.skipped == ["CREATE CONSTRAINT ... IS NOT NULL"]


# ---------------------------------------------------------------------------
# Live: graph selection only through the URI
# ---------------------------------------------------------------------------


def _is_age_reachable(host: str = AGE_HOST, port: int = AGE_PORT) -> bool:
    """Checks whether the Apache AGE PostgreSQL port is reachable."""
    try:
        with socket.create_connection((host, port), timeout=1.0):
            return True
    except OSError:
        return False


@contextlib.contextmanager
def _temporary_age_graph(prefix: str) -> Iterator[tuple[Any, str]]:
    """Creates a uniquely named graph and drops it only when this test created it."""
    psycopg = pytest.importorskip("psycopg")
    try:
        conn = psycopg.connect(AGE_DATABASE_URI, autocommit=True, connect_timeout=2)
    except Exception as exc:  # pragma: no cover - environment dependent
        pytest.skip(f"Apache AGE container not reachable on port {AGE_PORT}: {exc}")

    graph = f"{prefix}_{uuid.uuid4().hex[:12]}"
    created = False
    try:
        with conn.cursor() as cur:
            cur.execute("LOAD 'age';")
            cur.execute('SET search_path = ag_catalog, "$user", public;')
            cur.execute("SELECT count(*) FROM ag_catalog.ag_graph WHERE name = %s;", (graph,))
            if cur.fetchone()[0] == 0:
                cur.execute(f"SELECT ag_catalog.create_graph('{graph}');")
                created = True
        yield conn, graph
    finally:
        try:
            if created:
                with conn.cursor() as cur:
                    cur.execute(f"SELECT ag_catalog.drop_graph('{graph}', true);")
            conn.close()
        except Exception:  # pragma: no cover - best-effort cleanup
            pass


class UriGraphProbe(Node):
    """Node used to prove which Apache AGE graph received a write."""

    __label__ = "UriGraphProbe"
    id: int = Field(primary_key=True)
    name: str


def _graph_node_count(conn: Any, graph: str, marker: str) -> int:
    """Counts probe nodes with ``marker`` in ``graph`` through a raw AGE query."""
    with conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) FROM ag_catalog.cypher("
            f"'{graph}', $$ MATCH (n:UriGraphProbe) WHERE n.name = '{marker}' RETURN n $$"
            ") AS (v agtype);"
        )
        return int(cur.fetchone()[0])


@pytest.mark.live
@pytest.mark.parametrize(
    ("scheme", "param"),
    [
        ("postgresql", "graph"),
        ("postgresql", "graph_name"),
        ("age", "graph"),
        ("age", "graph_name"),
    ],
)
def test_live_uri_graph_param_targets_selected_graph(scheme: str, param: str) -> None:
    """A Session built from a URI writes to the graph named only through the URI."""
    if not _is_age_reachable():
        pytest.skip(f"Apache AGE container not reachable on port {AGE_PORT}")

    with _temporary_age_graph("uri_probe") as (admin_conn, graph):
        uri = (
            f"{scheme}://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}"
            f"/voyager_graph?{param}={graph}&connect_timeout=5"
        )
        bridge = create_bridge(uri)
        assert getattr(bridge, "graph_name", None) == graph

        session = Session(bridge=bridge, dialect="age")
        marker = f"probe-{uuid.uuid4().hex}"
        try:
            assert session.graph_name == graph
            probe = UriGraphProbe("p")
            created = session.execute(
                Query.create(UriGraphProbe("p", id=1, name=marker)).return_(probe.name)
            )
            assert len(created) == 1
            assert marker in str(created[0]["name"])

            read = session.execute(
                Query.match(UriGraphProbe("p"))
                .where(UriGraphProbe("p").name == marker)
                .return_(UriGraphProbe("p").name)
            )
            assert len(read) == 1
            assert marker in str(read[0]["name"])

            assert _graph_node_count(admin_conn, graph, marker) == 1
            assert _graph_node_count(admin_conn, "age_graph", marker) == 0
        finally:
            session.close()


@pytest.mark.live
@pytest.mark.parametrize("param", ["graph", "graph_name"])
def test_live_native_client_accepts_age_uri_graph_param(param: str) -> None:
    """The native client connects when ?graph= / ?graph_name= names an AGE graph."""
    if not _is_age_reachable():
        pytest.skip(f"Apache AGE container not reachable on port {AGE_PORT}")

    with _temporary_age_graph("uri_native") as (_, graph):
        uri = (
            f"age://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}"
            f"/voyager_graph?{param}={graph}&connect_timeout=5"
        )
        client = NativeClient(uri, min_idle=1, max_size=2)
        try:
            assert client.ping_sync() is True
        finally:
            client.close()


@pytest.mark.live
def test_live_native_session_routes_to_uri_selected_graph() -> None:
    """Session(uri, backend='native') writes to the graph named only through the URI."""
    if not _is_age_reachable():
        pytest.skip(f"Apache AGE container not reachable on port {AGE_PORT}")

    with _temporary_age_graph("uri_native_session") as (admin_conn, graph):
        uri = (
            f"age://postgres:voyagerpass123@{AGE_HOST}:{AGE_PORT}"
            f"/voyager_graph?graph={graph}&connect_timeout=5"
        )
        session = Session(uri, dialect="age", backend="native")
        marker = f"native-{uuid.uuid4().hex}"
        try:
            assert session.graph_name == graph
            assert session.backend == "native"
            # The native PostgreSQL adapter does not expand the `%s` parameter placeholder
            # emitted by the AGE DSL, so this test issues literal AGE SQL using the graph
            # name the session resolved from the URI.
            session.execute(
                "SELECT * FROM cypher("
                f"'{session.graph_name}', $$ CREATE (n:UriGraphProbe {{id: 2, name: '{marker}'}}) "
                "RETURN n $$) AS (v agtype);"
            )
            assert _graph_node_count(admin_conn, graph, marker) == 1
            assert _graph_node_count(admin_conn, "age_graph", marker) == 0
        finally:
            session.close()
