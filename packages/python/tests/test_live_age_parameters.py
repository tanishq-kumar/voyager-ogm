"""Live Apache AGE parameter-binding tests for the native client (#222).

Every scenario runs through the same public Session API on ``backend="bridge"``
and then on ``backend="auto"`` / ``backend="native"``; the normalized results
must be identical. Parameters must be bound, never interpolated, so values may
contain quotes, dollar-quoting sequences, backslashes, unicode, nested maps and
lists, and nulls.
"""

from __future__ import annotations

import json
import os
import socket
import uuid
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Any

import pytest
from voyager_ogm import Field, Node, Query, Session

pytestmark = pytest.mark.live

AGE_URI = "age://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph"
AGE_DSN = (
    "host=127.0.0.1 port=5455 user=postgres password=voyagerpass123 "
    "dbname=voyager_graph connect_timeout=2"
)
BACKENDS = ("auto", "native", "bridge")


class ParamNode(Node):
    __label__ = "Fix222Param"

    key: str = Field(primary_key=True)
    name: str
    score: int = 0


class ParamBulkNode(Node):
    __label__ = "Fix222Bulk"

    key: str = Field(primary_key=True)
    name: str
    score: int = 0


class ParamValueNode(Node):
    __label__ = "Fix222Value"

    key: str = Field(primary_key=True)
    text: str | None = None
    number: int = 0
    flag: bool = False
    payload: dict = {}
    items: list = []


def _port_open(host: str = "127.0.0.1", port: int = 5455) -> bool:
    """Returns True when the Apache AGE TCP port accepts connections."""
    try:
        with socket.create_connection((host, port), timeout=1.0):
            return True
    except OSError:
        return False


def _require_age() -> Any:
    """Skips the test when AGE or psycopg is unavailable."""
    if not _port_open():
        pytest.skip("Apache AGE container not reachable on port 5455")
    psycopg = pytest.importorskip("psycopg")
    return psycopg


@contextmanager
def _age_session(
    backend: str,
    *,
    enable_identity_map: bool = False,
) -> Iterator[Session]:
    """Creates a fresh graph, yields a session, then drops the graph."""
    psycopg = _require_age()
    graph = f"fix222_{backend}_{os.getpid()}_{uuid.uuid4().hex[:10]}"
    raw = psycopg.connect(AGE_DSN, autocommit=True)
    try:
        with raw.cursor() as cur:
            cur.execute("LOAD 'age';")
            cur.execute('SET search_path = ag_catalog, "$user", public;')
            cur.execute("SELECT ag_catalog.create_graph(%s);", (graph,))
        session = Session(
            AGE_URI,
            dialect="age",
            graph_name=graph,
            backend=backend,
            enable_identity_map=enable_identity_map,
        )
        try:
            yield session
        finally:
            session.close()
    finally:
        try:
            with raw.cursor() as cur:
                cur.execute("LOAD 'age';")
                cur.execute("SELECT ag_catalog.drop_graph(%s, true);", (graph,))
        finally:
            raw.close()


def _decode(value: Any) -> Any:
    """Decodes AGE agtype text (JSON) while leaving ordinary strings intact."""
    if isinstance(value, str):
        try:
            return json.loads(value)
        except (json.JSONDecodeError, ValueError):
            return value
    return value


def _normalize(records: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Normalizes rows so native Arrow typing and bridge text typing compare equal."""
    return [{key: _decode(value) for key, value in row.items()} for row in records]


def _match_all(session: Session, model: type[Node]) -> list[dict[str, Any]]:
    """Returns every node of a model in a deterministic normalized form."""
    node = model("n")
    records = session.execute(Query.match(node).return_(node.key, node.name, node.score)).records
    return sorted(_normalize(records), key=lambda item: str(item["key"]))


def _run_match_where(backend: str) -> list[dict[str, Any]]:
    """Creates nodes and runs MATCH with a WHERE parameter."""
    with _age_session(backend) as session:
        for key, name, score in (
            ("m1", "Alice", 10),
            ("m2", "Bob", 20),
            ("m3", "O'Reilly", 30),
        ):
            session.execute(
                Query.create(ParamNode(key=key, name=name, score=score)).return_(ParamNode.key)
            )
        node = ParamNode("n")
        result = session.execute(
            Query.match(node).where(node.name == "Alice").return_(node.key, node.name, node.score)
        )
        return _normalize(result.records)


def _run_create_and_merge(backend: str) -> list[dict[str, Any]]:
    """Creates, merges, and re-merges nodes using property parameters."""
    with _age_session(backend) as session:
        created = ParamNode(key="c1", name="created", score=1)
        assert session.execute(Query.create(created).return_(ParamNode.key))
        merged = ParamNode(key="k1", name="merged", score=2)
        assert session.execute(Query.merge(merged).return_(ParamNode.key))
        updated = ParamNode(key="k1", name="merged-again", score=3)
        assert session.execute(Query.merge(updated).return_(ParamNode.key))
        return _match_all(session, ParamNode)


def _run_bulk_and_flush(backend: str) -> dict[str, Any]:
    """Runs bulk create, bulk upsert, and identity-map flush."""
    with _age_session(backend, enable_identity_map=True) as session:
        create_plan = session.bulk_create(
            ParamBulkNode,
            [
                {"key": "b1", "name": "bulk-one", "score": 1},
                {"key": "b2", "name": "bulk-two", "score": 2},
            ],
            batch_size=1,
        )
        create_result = session.run_bulk(create_plan)
        upsert_plan = session.bulk_upsert(
            ParamBulkNode,
            [
                {"key": "b1", "name": "bulk-updated", "score": 11},
                {"key": "b3", "name": "bulk-three", "score": 3},
            ],
            key_field="key",
            batch_size=1,
        )
        upsert_result = session.run_bulk(upsert_plan)
        flushed = ParamBulkNode(key="b4", name="flush-four", score=4)
        session.register(flushed)
        flush_results = session.flush()

        return {
            "create_records": create_result.total_records,
            "upsert_records": upsert_result.total_records,
            "flush_batches": sum(result.total_batches for result in flush_results),
            "rows": _match_all(session, ParamBulkNode),
        }


def _run_transaction_rollback(backend: str) -> dict[str, int]:
    """Writes parameterized nodes in a transaction and rolls back via an error."""
    with _age_session(backend) as session:
        node = ParamNode("n")
        try:
            with session.transaction():
                session.execute(
                    Query.create(ParamNode(key="tx-rollback", name="rollback", score=1)).return_(
                        ParamNode.key
                    )
                )
                inside = session.execute(
                    Query.match(node).where(node.key == "tx-rollback").return_(node.key)
                )
                assert len(inside) == 1, "transaction did not see its own write"
                raise RuntimeError("rollback")
        except RuntimeError:
            pass
        after_rollback = session.execute(
            Query.match(node).where(node.key == "tx-rollback").return_(node.key)
        )
        with session.transaction():
            session.execute(
                Query.create(ParamNode(key="tx-commit", name="commit", score=2)).return_(
                    ParamNode.key
                )
            )
        after_commit = session.execute(
            Query.match(node).where(node.key == "tx-commit").return_(node.key)
        )
        return {
            "after_rollback": len(after_rollback),
            "after_commit": len(after_commit),
        }


def _run_special_values(backend: str) -> list[dict[str, Any]]:
    """Stores and reads back quotes, $$, backslashes, unicode, nesting, and null."""
    text = "x') ; DROP TABLE t; -- $$ \\ ünïcode 日本語"
    payload = {"nested": {"list": [1, None, "a'b", "$$"], "flag": True}}
    items = [1, "two", {"x": 3}, None, ["nested"]]
    with _age_session(backend) as session:
        created = ParamValueNode(
            key="v1",
            text=text,
            number=7,
            flag=True,
            payload=payload,
            items=items,
        )
        assert session.execute(Query.create(created).return_(ParamValueNode.key))
        node = ParamValueNode("n")
        result = session.execute(
            Query.match(node)
            .where(node.key == "v1")
            .return_(node.key, node.text, node.number, node.flag, node.payload, node.items)
        )
        decoded = _normalize(result.records)
        assert decoded == [
            {
                "key": "v1",
                "text": text,
                "number": 7,
                "flag": True,
                "payload": payload,
                "items": items,
            }
        ]
        return decoded


def _run_null_value(backend: str) -> list[dict[str, Any]]:
    """Stores an explicit null and returns it as null."""
    with _age_session(backend) as session:
        created = ParamValueNode(key="null1", text=None)
        assert session.execute(Query.create(created).return_(ParamValueNode.key))
        node = ParamValueNode("n")
        result = session.execute(Query.match(node).where(node.key == "null1").return_(node.text))
        return _normalize(result.records)


def _run_injection_payload(backend: str) -> list[dict[str, Any]]:
    """Stores an injection-shaped string as ordinary data."""
    payload = "x') ; DROP TABLE t; --"
    with _age_session(backend) as session:
        session.execute(
            Query.create(ParamNode(key="inj", name=payload, score=9)).return_(ParamNode.key)
        )
        node = ParamNode("n")
        result = session.execute(
            Query.match(node).where(node.name == payload).return_(node.key, node.name)
        )
        # The table the injection tries to drop must still exist afterwards.
        session.execute("SELECT 1")
        return _normalize(result.records)


@pytest.mark.parametrize("backend", BACKENDS)
def test_live_age_match_where_parameter(backend: str) -> None:
    """MATCH ... WHERE with a bound parameter matches bridge results."""
    result = _run_match_where(backend)
    if backend == "bridge":
        assert result == [{"key": "m1", "name": "Alice", "score": 10}]
    else:
        assert result == _run_match_where("bridge")


@pytest.mark.parametrize("backend", BACKENDS)
def test_live_age_create_and_merge_parameters(backend: str) -> None:
    """CREATE and MERGE write property parameters identically to the bridge."""
    result = _run_create_and_merge(backend)
    expected = [
        {"key": "c1", "name": "created", "score": 1},
        {"key": "k1", "name": "merged-again", "score": 3},
    ]
    assert result == expected
    if backend != "bridge":
        assert result == _run_create_and_merge("bridge")


@pytest.mark.parametrize("backend", BACKENDS)
def test_live_age_bulk_create_upsert_and_flush(backend: str) -> None:
    """Bulk create, bulk upsert, and flush work identically on every backend."""
    result = _run_bulk_and_flush(backend)
    expected_rows = [
        {"key": "b1", "name": "bulk-updated", "score": 11},
        {"key": "b2", "name": "bulk-two", "score": 2},
        {"key": "b3", "name": "bulk-three", "score": 3},
        {"key": "b4", "name": "flush-four", "score": 4},
    ]
    assert result["create_records"] == 2
    assert result["upsert_records"] == 2
    assert result["flush_batches"] >= 1
    assert result["rows"] == expected_rows
    if backend != "bridge":
        reference = _run_bulk_and_flush("bridge")
        assert result["rows"] == reference["rows"]


@pytest.mark.parametrize("backend", BACKENDS)
def test_live_age_transaction_rollback_and_commit(backend: str) -> None:
    """A parameterized transactional write rolls back cleanly and commits when asked."""
    result = _run_transaction_rollback(backend)
    assert result == {"after_rollback": 0, "after_commit": 1}
    if backend != "bridge":
        assert result == _run_transaction_rollback("bridge")


@pytest.mark.parametrize("backend", BACKENDS)
def test_live_age_special_values_round_trip(backend: str) -> None:
    """Quotes, $$, backslashes, unicode, nested maps/lists, and null round-trip."""
    result = _run_special_values(backend)
    expected = _run_special_values("bridge")
    assert result == expected
    assert result[0]["text"] == "x') ; DROP TABLE t; -- $$ \\ ünïcode 日本語"
    assert result[0]["payload"] == {"nested": {"list": [1, None, "a'b", "$$"], "flag": True}}
    assert result[0]["items"] == [1, "two", {"x": 3}, None, ["nested"]]

    null_result = _run_null_value(backend)
    assert null_result == [{"text": None}]
    if backend != "bridge":
        assert null_result == _run_null_value("bridge")


@pytest.mark.parametrize("backend", BACKENDS)
def test_live_age_injection_payload_stored_as_data(backend: str) -> None:
    """An injection-shaped parameter is stored and matched as plain data."""
    result = _run_injection_payload(backend)
    assert result == [{"key": "inj", "name": "x') ; DROP TABLE t; --"}]
    if backend != "bridge":
        assert result == _run_injection_payload("bridge")


def _run_sql_parameters(backend: str) -> list[dict[str, Any]]:
    """Binds named SQL placeholders (not just AGE ``%s``) through the session."""
    _require_age()
    table = f"fix222_sql_{os.getpid()}_{uuid.uuid4().hex[:10]}"
    payload = "x') ; DROP TABLE t; -- ünïcode $$ \\"
    session = Session(AGE_URI, dialect="age", backend=backend)
    try:
        session.execute(f"CREATE TABLE {table} (id int, name text)")
        session.execute(
            f"INSERT INTO {table} VALUES ($id, $name)",
            {"id": 1, "name": payload},
        )
        rows = session.execute(
            f"SELECT id, name FROM {table} WHERE name = $name", {"name": payload}
        ).records
        return _normalize(rows)
    finally:
        try:
            session.execute(f"DROP TABLE IF EXISTS {table}")
        finally:
            session.close()


@pytest.mark.parametrize("backend", BACKENDS)
def test_live_age_sql_parameter_binding(backend: str) -> None:
    """Plain SQL ``$name`` parameters are bound, never interpolated, on every backend."""
    payload = "x') ; DROP TABLE t; -- ünïcode $$ \\"
    result = _run_sql_parameters(backend)
    assert result == [{"id": 1, "name": payload}]
    if backend != "bridge":
        assert result == _run_sql_parameters("bridge")
