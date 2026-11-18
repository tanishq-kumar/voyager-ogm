"""Unit tests for create_bridge auth, URI parsing, credential masking, and AGE connection setup."""

from __future__ import annotations

from typing import Any
from unittest.mock import MagicMock

import pytest
from voyager_ogm.bridge import (
    PostgresBridge,
    create_bridge,
    mask_uri_password,
    setup_postgres_age,
)


class MockCursor:
    """Mock database cursor tracking executed statements and fetch results."""

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
    """Mock PostgreSQL connection."""

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


# ---------------------------------------------------------------------------
# 1. Bolt Credentials & Auth
# ---------------------------------------------------------------------------


def test_bolt_uri_credentials(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test extracting username and password from Bolt URI into driver auth kwargs."""
    mock_driver = MagicMock()
    mock_drv_inst = MagicMock()
    mock_driver.return_value = mock_drv_inst

    import neo4j

    monkeypatch.setattr(neo4j.GraphDatabase, "driver", mock_driver)

    bridge = create_bridge("bolt://neo4j:voyagerpass123@127.0.0.1:7687")
    assert bridge is not None

    mock_driver.assert_called_once()
    args, kwargs = mock_driver.call_args
    assert args[0] == "bolt://127.0.0.1:7687"
    assert kwargs.get("auth") == ("neo4j", "voyagerpass123")


def test_bolt_uri_query_not_forwarded_to_driver(monkeypatch: pytest.MonkeyPatch) -> None:
    """Query parameters are parsed into driver kwargs and stripped from the bolt:// URI.

    The neo4j direct driver rejects query parameters on bolt:// URIs.
    """
    mock_driver = MagicMock()
    monkeypatch.setattr("neo4j.GraphDatabase.driver", mock_driver)

    create_bridge("bolt://neo4j:pw@127.0.0.1:7687?max_connection_pool_size=5")

    args, kwargs = mock_driver.call_args
    assert args[0] == "bolt://127.0.0.1:7687"
    assert kwargs.get("auth") == ("neo4j", "pw")


def test_bolt_url_encoded_password(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test URL-encoded characters in username and password are correctly decoded."""
    mock_driver = MagicMock()
    monkeypatch.setattr("neo4j.GraphDatabase.driver", mock_driver)

    create_bridge("bolt://user%3Aone:p%40ss%23word%25@127.0.0.1:7687/db?connect_timeout=15")

    mock_driver.assert_called_once()
    args, kwargs = mock_driver.call_args
    assert args[0] == "bolt://127.0.0.1:7687/db"
    assert kwargs.get("auth") == ("user:one", "p@ss#word%")
    assert kwargs.get("connection_timeout") == 15.0


def test_bolt_explicit_auth_overriding_uri(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test explicit auth= keyword argument overrides credentials embedded in the URI."""
    mock_driver = MagicMock()
    monkeypatch.setattr("neo4j.GraphDatabase.driver", mock_driver)

    create_bridge(
        "bolt://uri_user:uri_pass@127.0.0.1:7687",
        auth=("explicit_user", "explicit_pass"),
    )

    mock_driver.assert_called_once()
    args, kwargs = mock_driver.call_args
    assert args[0] == "bolt://127.0.0.1:7687"
    assert kwargs.get("auth") == ("explicit_user", "explicit_pass")


def test_bolt_token_passthrough(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test arbitrary non-tuple auth tokens are passed through unchanged to GraphDatabase.driver."""
    mock_driver = MagicMock()
    monkeypatch.setattr("neo4j.GraphDatabase.driver", mock_driver)

    class CustomAuthToken:
        pass

    token = CustomAuthToken()
    create_bridge("bolt://127.0.0.1:7687", auth=token)

    mock_driver.assert_called_once()
    _, kwargs = mock_driver.call_args
    assert kwargs.get("auth") is token


# ---------------------------------------------------------------------------
# 2. Postgres & AGE Auth
# ---------------------------------------------------------------------------


def test_postgres_tuple_auth(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test Postgres/AGE accepts a (user, password) tuple and forwards user/password connect kwargs."""
    mock_connect = MagicMock()
    mock_conn = MockConnection()
    mock_connect.return_value = mock_conn

    import psycopg

    monkeypatch.setattr(psycopg, "connect", mock_connect)

    create_bridge(
        "postgresql://127.0.0.1:5455/voyager_graph",
        auth=("postgres", "voyagerpass123"),
    )

    mock_connect.assert_called_once()
    args, kwargs = mock_connect.call_args
    assert args[0] == "postgresql://127.0.0.1:5455/voyager_graph"
    assert kwargs.get("user") == "postgres"
    assert kwargs.get("password") == "voyagerpass123"
    assert kwargs.get("autocommit") is True


def test_postgres_invalid_auth_type_raises(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test non-tuple auth parameter raises TypeError for PostgreSQL / AGE."""
    with pytest.raises(
        TypeError, match="Postgres/AGE auth must be a \\(username, password\\) tuple"
    ):
        create_bridge("postgresql://127.0.0.1:5455/voyager_graph", auth="bearer_token_string")

    with pytest.raises(
        TypeError, match="Postgres/AGE auth must be a \\(username, password\\) tuple"
    ):
        create_bridge("age://127.0.0.1:5455/voyager_graph", auth=("single_element",))


# ---------------------------------------------------------------------------
# 3. AGE Scheme Rewriting & TLS
# ---------------------------------------------------------------------------


def test_age_scheme_rewrite(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test age:// URI is rewritten to postgresql:// before connecting."""
    mock_connect = MagicMock()
    mock_conn = MockConnection()
    mock_connect.return_value = mock_conn

    monkeypatch.setattr("psycopg.connect", mock_connect)

    create_bridge("age://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph")

    mock_connect.assert_called_once()
    args, _ = mock_connect.call_args
    assert args[0].startswith("postgresql://")
    assert not args[0].startswith("age://")


def test_age_s_sslmode_rewrite(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test age+s:// URI is rewritten to postgresql:// and sslmode=require is passed."""
    mock_connect = MagicMock()
    mock_conn = MockConnection()
    mock_connect.return_value = mock_conn

    monkeypatch.setattr("psycopg.connect", mock_connect)

    create_bridge("age+s://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph")

    mock_connect.assert_called_once()
    args, kwargs = mock_connect.call_args
    assert args[0].startswith("postgresql://")
    assert kwargs.get("sslmode") == "require" or "sslmode=require" in args[0]


# ---------------------------------------------------------------------------
# 4. Password Masking
# ---------------------------------------------------------------------------


def test_mask_uri_password_helper() -> None:
    """Test password masking helper preserves URI structure while hiding secrets."""
    assert (
        mask_uri_password("bolt://neo4j:voyagerpass123@127.0.0.1:7687")
        == "bolt://neo4j:***@127.0.0.1:7687"
    )
    assert (
        mask_uri_password("postgresql://postgres:secret%40pass@localhost:5432/mydb?sslmode=require")
        == "postgresql://postgres:***@localhost:5432/mydb?sslmode=require"
    )
    assert mask_uri_password("bolt://127.0.0.1:7687") == "bolt://127.0.0.1:7687"
    assert mask_uri_password("bolt://neo4j@127.0.0.1:7687") == "bolt://neo4j@127.0.0.1:7687"
    assert mask_uri_password("invalid_plain_string") == "invalid_plain_string"


def test_password_masked_in_create_bridge_error() -> None:
    """Test that connection error messages containing URIs mask the password."""
    uri_with_pass = "unsupported_xyz://admin:super_secret_pw@db.internal:9999/catalog"
    with pytest.raises(ValueError) as exc_info:
        create_bridge(uri_with_pass)

    err_msg = str(exc_info.value)
    assert "super_secret_pw" not in err_msg
    assert "admin:***@db.internal:9999" in err_msg


# ---------------------------------------------------------------------------
# 5. AGE Setup Statements: Issued vs Not Issued
# ---------------------------------------------------------------------------


def test_age_setup_statements_issued_for_age_scheme(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test age:// always unconditionally executes LOAD 'age' and search_path setup."""
    mock_conn = MockConnection()
    monkeypatch.setattr("psycopg.connect", lambda *args, **kwargs: mock_conn)

    create_bridge("age://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph")

    executed_stmts = [stmt for stmt, _ in mock_conn.cursor_inst.executed]
    assert any("LOAD 'age'" in stmt and "ag_catalog" in stmt for stmt in executed_stmts)


def test_age_setup_statements_not_issued_for_plain_postgres(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Test postgresql:// checks pg_extension and does NOT issue AGE setup when age extension is absent."""
    # fetchone returns None -> age extension is not installed
    mock_conn = MockConnection(fetchone_return=None)
    monkeypatch.setattr("psycopg.connect", lambda *args, **kwargs: mock_conn)

    create_bridge("postgresql://postgres:voyagerpass123@127.0.0.1:5432/plain_pg")

    executed_stmts = [stmt for stmt, _ in mock_conn.cursor_inst.executed]
    assert any("pg_extension" in stmt for stmt in executed_stmts)
    assert not any("LOAD 'age'" in stmt for stmt in executed_stmts)


def test_age_setup_statements_issued_for_postgres_with_age_extension(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Test postgresql:// detects age in pg_extension and runs LOAD 'age' setup."""
    # fetchone returns (1,) -> age extension IS installed
    mock_conn = MockConnection(fetchone_return=(1,))
    monkeypatch.setattr("psycopg.connect", lambda *args, **kwargs: mock_conn)

    create_bridge("postgresql://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph")

    executed_stmts = [stmt for stmt, _ in mock_conn.cursor_inst.executed]
    assert any("pg_extension" in stmt for stmt in executed_stmts)
    assert any("LOAD 'age'" in stmt and "ag_catalog" in stmt for stmt in executed_stmts)


def test_postgres_bridge_user_supplied_connection_auto_detects_age() -> None:
    """Test user-supplied connection passed to PostgresBridge auto-detects AGE and sets search_path."""
    mock_conn_with_age = MockConnection(fetchone_return=(1,))
    bridge = PostgresBridge(mock_conn_with_age)
    assert bridge.connection is mock_conn_with_age
    executed_stmts = [stmt for stmt, _ in mock_conn_with_age.cursor_inst.executed]
    assert any("LOAD 'age'" in stmt for stmt in executed_stmts)

    mock_conn_plain = MockConnection(fetchone_return=None)
    bridge_plain = PostgresBridge(mock_conn_plain)
    assert bridge_plain.connection is mock_conn_plain
    executed_stmts_plain = [stmt for stmt, _ in mock_conn_plain.cursor_inst.executed]
    assert not any("LOAD 'age'" in stmt for stmt in executed_stmts_plain)


# ---------------------------------------------------------------------------
# 6. FalkorDB / Redis Credentials
# ---------------------------------------------------------------------------


def test_falkordb_tuple_auth(monkeypatch: pytest.MonkeyPatch) -> None:
    """Test FalkorDB accepts a (username, password) tuple."""
    mock_from_url = MagicMock()
    mock_falkor_inst = MagicMock()
    mock_from_url.return_value = mock_falkor_inst

    from falkordb import FalkorDB

    monkeypatch.setattr(FalkorDB, "from_url", mock_from_url)

    create_bridge("redis://127.0.0.1:6379", auth=("falkor_user", "falkor_pass"))

    mock_from_url.assert_called_once()
    _, kwargs = mock_from_url.call_args
    assert kwargs.get("username") == "falkor_user"
    assert kwargs.get("password") == "falkor_pass"


def test_falkordb_invalid_auth_type_raises() -> None:
    """Test invalid auth type raises TypeError for FalkorDB."""
    with pytest.raises(
        TypeError, match="FalkorDB/Redis auth must be a \\(username, password\\) tuple"
    ):
        create_bridge("redis://127.0.0.1:6379", auth="token_not_tuple")


def test_setup_postgres_age_helper_direct() -> None:
    """Test direct invocation of setup_postgres_age helper with force and auto-detect."""
    conn_force = MockConnection()
    setup_postgres_age(conn_force, force=True)
    assert any("LOAD 'age'" in stmt for stmt, _ in conn_force.cursor_inst.executed)
    assert getattr(conn_force, "_voyager_age_initialized", False) is True

    # Idempotent call does not re-execute
    exec_len = len(conn_force.cursor_inst.executed)
    setup_postgres_age(conn_force, force=True)
    assert len(conn_force.cursor_inst.executed) == exec_len
