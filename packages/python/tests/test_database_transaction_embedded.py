"""Real-transaction tests for the embedded DuckDB/DuckPGQ and Relate engines.

Both engines run in-process, so these tests are part of the normal suite and
only skip when the optional driver package is missing.
"""

from __future__ import annotations

from typing import Any

import pytest
from voyager_ogm import AsyncSession, Field, Node, Session, node


@node(label="EmbeddedTxPerson")
class EmbeddedTxPerson(Node):
    """Identity-mapped node used by the embedded transaction tests."""

    id: str = Field(primary_key=True)
    name: str = Field()
    balance: float = Field(default=0.0)


def _duckdb_session() -> tuple[Any, Session]:
    duckdb = pytest.importorskip("duckdb")
    con = duckdb.connect(":memory:")
    con.execute("CREATE TABLE tx_items(id INTEGER, name VARCHAR)")
    return con, Session(bridge=con, dialect="sql_pgq")


def _duckdb_count(session: Session) -> int:
    return int(session.execute("SELECT count(*) AS c FROM tx_items")[0]["c"])


def _relate_session(tmp_path: Any) -> tuple[Any, Session]:
    relate = pytest.importorskip("relate")
    con = relate.connect(str(tmp_path / "transaction.rl"))
    return con, Session(bridge=con, dialect="cypher")


def _relate_values(session: Session) -> list[int]:
    return [
        int(record["i"])
        for record in session.execute("MATCH (n:TxItem) RETURN n.i AS i ORDER BY n.i")
    ]


async def _relate_values_async(session: AsyncSession) -> list[int]:
    return [
        int(record["i"])
        for record in await session.execute("MATCH (n:TxItem) RETURN n.i AS i ORDER BY n.i")
    ]


# ---------------------------------------------------------------------------
# DuckDB / DuckPGQ (sync)
# ---------------------------------------------------------------------------


def test_duckdb_transaction_commit_persists():
    _con, session = _duckdb_session()

    with session.transaction() as tx:
        tx.execute("INSERT INTO tx_items VALUES (1, 'Alice')")
        assert tx.execute("SELECT * FROM tx_items") == [{"id": 1, "name": "Alice"}]

    assert tx.state == "COMMITTED"
    assert _duckdb_count(session) == 1


def test_duckdb_transaction_exception_rolls_back_and_reraises():
    _con, session = _duckdb_session()
    tx_ref = None

    with pytest.raises(RuntimeError, match="boom"):
        with session.transaction() as tx:
            tx_ref = tx
            session.execute("INSERT INTO tx_items VALUES (1, 'Alice')")
            raise RuntimeError("boom")

    assert tx_ref is not None
    assert tx_ref.state == "ROLLED_BACK"
    assert _duckdb_count(session) == 0


def test_duckdb_failed_statement_then_clean_exit_raises_and_rolls_back():
    _con, session = _duckdb_session()

    with pytest.raises(RuntimeError, match="aborted"):
        with session.transaction() as tx:
            tx.execute("INSERT INTO tx_items VALUES (1, 'Alice')")
            with pytest.raises(Exception, match="missing_table"):
                tx.execute("SELECT * FROM missing_table")
            assert tx.state == "FAILED"

    assert _duckdb_count(session) == 0


def test_duckdb_failed_statement_then_write_raises_aborted():
    _con, session = _duckdb_session()

    with pytest.raises(RuntimeError, match="aborted"):
        with session.transaction() as tx:
            tx.execute("INSERT INTO tx_items VALUES (1, 'Alice')")
            with pytest.raises(Exception, match="missing_table"):
                tx.execute("SELECT * FROM missing_table")
            tx.execute("INSERT INTO tx_items VALUES (2, 'Bob')")

    assert _duckdb_count(session) == 0


def test_duckdb_savepoints_raise_and_nested_transaction_raises():
    _con, session = _duckdb_session()

    with session.transaction() as tx:
        assert tx.supports_savepoints is False
        with pytest.raises(RuntimeError, match="DuckDB transactions do not support savepoints"):
            tx.create_savepoint("sp")
        with pytest.raises(RuntimeError, match="DuckDB transactions do not support savepoints"):
            tx.rollback_to_savepoint("sp")
        with pytest.raises(RuntimeError, match="DuckDB transactions do not support savepoints"):
            tx.release_savepoint("sp")
        with pytest.raises(RuntimeError, match="DuckDB transactions do not support savepoints"):
            with tx.savepoint("sp"):
                pass
        with pytest.raises(RuntimeError, match="already active"):
            session.transaction()


def test_duckdb_duckpgq_graph_table_parameters_inside_transaction():
    duckdb = pytest.importorskip("duckdb")
    con = duckdb.connect(":memory:", config={"allow_unsigned_extensions": "true"})
    try:
        con.execute("LOAD duckpgq;")
    except Exception as exc:  # pragma: no cover - depends on offline extension cache
        pytest.skip(f"duckpgq extension could not be loaded offline: {exc}")

    con.execute("CREATE TABLE Person(id BIGINT PRIMARY KEY, name VARCHAR, city VARCHAR)")
    con.execute("INSERT INTO Person VALUES (1, 'Alice', 'London'), (2, 'Bob', 'Berlin')")
    con.execute("CREATE PROPERTY GRAPH tx_graph VERTEX TABLES (Person LABEL Person)")
    session = Session(bridge=con, dialect="sql_pgq")

    with session.transaction() as tx:
        tx.execute("INSERT INTO Person VALUES (3, 'Charlie', 'London')")
        rows = tx.execute(
            "SELECT * FROM GRAPH_TABLE (tx_graph MATCH (p IS Person) "
            "WHERE p.city = $city COLUMNS (p.name))",
            {"city": "London"},
        )

    assert sorted(row["name"] for row in rows) == ["Alice", "Charlie"]
    assert tx.state == "COMMITTED"
    assert len(session.execute("SELECT * FROM Person")) == 3


def test_duckdb_unit_of_work_rollback_restores_entity_and_rows():
    duckdb = pytest.importorskip("duckdb")
    con = duckdb.connect(":memory:")
    con.execute(
        "CREATE TABLE EmbeddedTxPerson(id VARCHAR PRIMARY KEY, name VARCHAR, balance DOUBLE)"
    )
    con.execute("INSERT INTO EmbeddedTxPerson VALUES ('p1', 'Alice', 100.0)")
    session = Session(bridge=con, dialect="sql_pgq", enable_identity_map=True)

    person = EmbeddedTxPerson(id="p1", name="Alice", balance=100.0)
    session.register(person, is_clean=True)

    with pytest.raises(RuntimeError, match="boom"):
        with session.transaction():
            person.name = "Bob"
            person.balance = 999.0
            session.flush()
            assert session.execute("SELECT * FROM EmbeddedTxPerson") == [
                {"id": "p1", "name": "Bob", "balance": 999.0}
            ]
            raise RuntimeError("boom")

    assert person.get("name") == "Alice"
    assert person.get("balance") == 100.0
    assert person.dirty_fields == {}
    assert session.execute("SELECT * FROM EmbeddedTxPerson") == [
        {"id": "p1", "name": "Alice", "balance": 100.0}
    ]


# ---------------------------------------------------------------------------
# DuckDB / DuckPGQ (async)
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_duckdb_async_transaction_commit_rollback_and_failure():
    duckdb = pytest.importorskip("duckdb")
    con = duckdb.connect(":memory:")
    con.execute("CREATE TABLE tx_items(id INTEGER, name VARCHAR)")
    session = AsyncSession(bridge=con, dialect="sql_pgq")

    async with session.transaction() as tx:
        await tx.execute("INSERT INTO tx_items VALUES (1, 'Alice')")
    assert tx.state == "COMMITTED"
    assert int((await session.execute("SELECT count(*) AS c FROM tx_items"))[0]["c"]) == 1

    with pytest.raises(RuntimeError, match="boom"):
        async with session.transaction():
            await session.execute("INSERT INTO tx_items VALUES (2, 'Bob')")
            raise RuntimeError("boom")
    assert int((await session.execute("SELECT count(*) AS c FROM tx_items"))[0]["c"]) == 1

    with pytest.raises(RuntimeError, match="aborted"):
        async with session.transaction() as tx:
            await tx.execute("INSERT INTO tx_items VALUES (3, 'Charlie')")
            with pytest.raises(Exception, match="missing_table"):
                await tx.execute("SELECT * FROM missing_table")
    assert tx.state == "ROLLED_BACK"
    assert int((await session.execute("SELECT count(*) AS c FROM tx_items"))[0]["c"]) == 1

    with pytest.raises(RuntimeError, match="already active"):
        async with session.transaction():
            session.transaction()


# ---------------------------------------------------------------------------
# Relate (sync)
# ---------------------------------------------------------------------------


def test_relate_transaction_commit_persists_and_restores_autocommit(tmp_path):
    con, session = _relate_session(tmp_path)
    assert con.autocommit is True

    with session.transaction() as tx:
        tx.execute("CREATE (:TxItem {i: 1})")
        assert _relate_values(session) == [1]

    assert tx.state == "COMMITTED"
    assert con.autocommit is True
    assert _relate_values(session) == [1]


def test_relate_transaction_exception_rolls_back_and_restores_autocommit(tmp_path):
    con, session = _relate_session(tmp_path)
    tx_ref = None

    with pytest.raises(RuntimeError, match="boom"):
        with session.transaction() as tx:
            tx_ref = tx
            session.execute("CREATE (:TxItem {i: 1})")
            raise RuntimeError("boom")

    assert tx_ref is not None
    assert tx_ref.state == "ROLLED_BACK"
    assert con.autocommit is True
    assert _relate_values(session) == []


def test_relate_failed_statement_then_clean_exit_raises_and_restores_autocommit(tmp_path):
    con, session = _relate_session(tmp_path)

    with pytest.raises(RuntimeError, match="aborted"):
        with session.transaction() as tx:
            tx.execute("CREATE (:TxItem {i: 1})")
            with pytest.raises(Exception, match="/ by zero"):
                tx.execute("RETURN 1/0")
            assert tx.state == "FAILED"

    assert con.autocommit is True
    assert _relate_values(session) == []


def test_relate_failed_statement_then_write_raises_aborted(tmp_path):
    con, session = _relate_session(tmp_path)

    with pytest.raises(RuntimeError, match="aborted"):
        with session.transaction() as tx:
            tx.execute("CREATE (:TxItem {i: 1})")
            with pytest.raises(Exception, match="/ by zero"):
                tx.execute("RETURN 1/0")
            tx.execute("CREATE (:TxItem {i: 2})")

    assert con.autocommit is True
    assert _relate_values(session) == []


def test_relate_savepoints_raise_and_nested_transaction_raises(tmp_path):
    _con, session = _relate_session(tmp_path)

    with session.transaction() as tx:
        assert tx.supports_savepoints is False
        with pytest.raises(RuntimeError, match="Relate transactions do not support savepoints"):
            tx.create_savepoint("sp")
        with pytest.raises(RuntimeError, match="Relate transactions do not support savepoints"):
            tx.rollback_to_savepoint("sp")
        with pytest.raises(RuntimeError, match="Relate transactions do not support savepoints"):
            tx.release_savepoint("sp")
        with pytest.raises(RuntimeError, match="Relate transactions do not support savepoints"):
            with tx.savepoint("sp"):
                pass
        with pytest.raises(RuntimeError, match="already active"):
            session.transaction()


def test_relate_existing_explicit_transaction_is_not_taken_over(tmp_path):
    con, session = _relate_session(tmp_path)
    session.execute("CREATE (:TxItem {i: 1})")
    con.autocommit = False
    session.execute("CREATE (:TxItem {i: 2})")
    assert con._transaction_open() is True

    with pytest.raises(RuntimeError, match="already has an explicit transaction"):
        session.transaction()

    # The caller's work is still open and untouched, not silently committed.
    assert con.autocommit is False
    assert con._transaction_open() is True
    con.rollback()
    assert _relate_values(session) == [1]
    con.autocommit = True


# ---------------------------------------------------------------------------
# Relate (async)
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_relate_async_transaction_commit_rollback_and_failure(tmp_path):
    relate = pytest.importorskip("relate")
    con = relate.connect(str(tmp_path / "async_transaction.rl"))
    session = AsyncSession(bridge=con, dialect="cypher")
    assert con.autocommit is True

    async with session.transaction() as tx:
        await tx.execute("CREATE (:TxItem {i: 1})")
    assert tx.state == "COMMITTED"
    assert con.autocommit is True
    assert await _relate_values_async(session) == [1]

    with pytest.raises(RuntimeError, match="boom"):
        async with session.transaction():
            await session.execute("CREATE (:TxItem {i: 2})")
            raise RuntimeError("boom")
    assert con.autocommit is True
    assert await _relate_values_async(session) == [1]

    with pytest.raises(RuntimeError, match="aborted"):
        async with session.transaction() as tx:
            await tx.execute("CREATE (:TxItem {i: 3})")
            with pytest.raises(Exception, match="/ by zero"):
                await tx.execute("RETURN 1/0")
    assert tx.state == "ROLLED_BACK"
    assert con.autocommit is True
    assert await _relate_values_async(session) == [1]
