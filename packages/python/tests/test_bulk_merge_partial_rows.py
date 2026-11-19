"""Regression tests for partial-row bulk MERGE and dirty-field flush writes.

A bulk MERGE must only write the properties a row actually supplies. The old
plan wrote every model field on both ``ON CREATE SET`` and ``ON MATCH SET``, so
an omitted field became a null parameter and deleted the stored property.
"""

from __future__ import annotations

import socket
from typing import Any

import polars as pl
import pytest
from voyager_ogm import (
    Field,
    MockBridge,
    Node,
    Session,
    create_bridge,
    node,
    reset_alias_counters,
)
from voyager_ogm._voyager_rs import compile_bulk_merge as _rs_compile_bulk_merge


@node("BulkNullAcct")
class Acct(Node):
    acc_no: str = Field(primary_key=True)
    name: str = Field()
    email: str = Field()


@node("BulkNullAcctLive205")
class LiveAcct(Node):
    acc_no: str = Field(primary_key=True)
    name: str = Field()
    email: str = Field()


@node("BulkNullGroupedAcct")
class GroupedAcct(Node):
    acc_no: str = Field(primary_key=True)
    name: str = Field()
    email: str = Field()
    status: str = Field(default="PENDING")


@pytest.fixture(autouse=True)
def _reset_aliases() -> None:
    reset_alias_counters()


def _plan(data: Any, batch_size: int = 50_000) -> Any:
    session = Session(dialect="cypher")
    return session.bulk_upsert(Acct, data, key_field="acc_no", batch_size=batch_size)


def test_partial_row_omits_absent_field_from_statement() -> None:
    plan = _plan([{"acc_no": "A1", "name": "Ada L."}])

    assert len(plan) == 1
    batch = plan.batches[0]
    assert "email" not in batch.statement
    assert batch.statement.endswith("ON MATCH SET _bulknullacct_0.name = row.name")
    assert batch.parameters["batch"] == [{"acc_no": "A1", "name": "Ada L."}]


def test_explicit_none_writes_and_clears_property() -> None:
    plan = _plan([{"acc_no": "A1", "email": None}])

    batch = plan.batches[0]
    assert "_bulknullacct_0.email = row.email" in batch.statement
    assert "name" not in batch.statement
    assert batch.parameters["batch"] == [{"acc_no": "A1", "email": None}]


def test_mixed_rows_compile_separate_groups() -> None:
    plan = _plan(
        [
            {"acc_no": "A1", "name": "Ada"},
            {"acc_no": "A2", "email": "ada@x.io"},
            {"acc_no": "A3", "name": "Grace", "email": "grace@x.io"},
        ]
    )

    assert len(plan) == 3
    assert [b.statement for b in plan] == [
        "UNWIND $batch AS row MERGE (_bulknullacct_0:BulkNullAcct {acc_no: row.acc_no}) "
        "ON CREATE SET _bulknullacct_0.name = row.name "
        "ON MATCH SET _bulknullacct_0.name = row.name",
        "UNWIND $batch AS row MERGE (_bulknullacct_0:BulkNullAcct {acc_no: row.acc_no}) "
        "ON CREATE SET _bulknullacct_0.email = row.email "
        "ON MATCH SET _bulknullacct_0.email = row.email",
        "UNWIND $batch AS row MERGE (_bulknullacct_0:BulkNullAcct {acc_no: row.acc_no}) "
        "ON CREATE SET _bulknullacct_0.name = row.name, _bulknullacct_0.email = row.email "
        "ON MATCH SET _bulknullacct_0.name = row.name, _bulknullacct_0.email = row.email",
    ]
    assert plan.statement == plan.batch_statements[0]
    assert [r["acc_no"] for b in plan for r in b.parameters["batch"]] == [
        "A1",
        "A2",
        "A3",
    ]


def test_batch_size_applies_per_group_and_preserves_order() -> None:
    plan = _plan(
        [
            {"acc_no": "n1", "name": "N1"},
            {"acc_no": "e1", "email": "E1"},
            {"acc_no": "n2", "name": "N2"},
            {"acc_no": "n3", "name": "N3"},
        ],
        batch_size=2,
    )

    assert [len(b.parameters["batch"]) for b in plan] == [2, 1, 1]
    assert [r["acc_no"] for b in plan for r in b.parameters["batch"]] == [
        "n1",
        "n2",
        "n3",
        "e1",
    ]
    assert [("email" in b.statement) for b in plan] == [False, False, True]
    assert plan.statement == plan.batch_statements[0]


def test_all_fields_output_matches_original_single_statement() -> None:
    data = [
        {"acc_no": "A1", "name": "Ada", "email": "ada@x.io"},
        {"acc_no": "A2", "name": "Grace", "email": "grace@x.io"},
    ]
    plan = _plan(data, batch_size=1)

    expected = _rs_compile_bulk_merge(
        label="BulkNullAcct",
        key_property="acc_no",
        properties=["name", "email"],
        batch_param="batch",
        row_alias="row",
        dialect="cypher",
    )["statement"]

    assert plan.statement == expected
    assert plan.batch_statements == [expected, expected]
    assert len(plan) == 2


def test_flush_writes_only_dirty_field() -> None:
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)
    tracked = session.register(Acct(acc_no="A1", name="Ada", email="ada@x.io"), is_clean=True)

    tracked.name = "Ada K."
    session.flush()

    statement, parameters = bridge.executed_queries[-1]
    assert "email" not in statement
    assert "_bulknullacct_0.name = row.name" in statement
    assert parameters["batch"] == [{"name": "Ada K.", "acc_no": "A1"}]


def test_polars_missing_column_is_not_supplied() -> None:
    frame = pl.DataFrame({"acc_no": ["A1"], "name": ["Ada L."]})
    plan = _plan(frame)

    statement = plan.batches[0].statement
    assert "email" not in statement
    assert "_bulknullacct_0.name = row.name" in statement


def test_polars_null_cell_is_explicit_none() -> None:
    frame = pl.DataFrame({"acc_no": ["A1"], "name": ["Ada L."], "email": [None]})
    plan = _plan(frame)

    statement = plan.batches[0].statement
    assert "_bulknullacct_0.email = row.email" in statement
    assert plan.batches[0].parameters["batch"] == [
        {"acc_no": "A1", "name": "Ada L.", "email": None}
    ]


def test_pyarrow_missing_column_and_null_cell() -> None:
    pa = pytest.importorskip("pyarrow")

    missing = pa.table({"acc_no": ["A1"], "name": ["Ada L."]})
    assert "email" not in _plan(missing).batches[0].statement

    with_null = pa.table({"acc_no": ["A1"], "name": ["Ada L."], "email": [None]})
    plan = _plan(with_null)
    assert "_bulknullacct_0.email = row.email" in plan.batches[0].statement
    assert plan.batches[0].parameters["batch"][0]["email"] is None


def test_pandas_missing_column_and_null_cell() -> None:
    pd = pytest.importorskip("pandas")

    missing = pd.DataFrame({"acc_no": ["A1"], "name": ["Ada L."]})
    assert "email" not in _plan(missing).batches[0].statement

    with_null = pd.DataFrame({"acc_no": ["A1"], "name": ["Ada L."], "email": [None]})
    plan = _plan(with_null)
    assert "_bulknullacct_0.email = row.email" in plan.batches[0].statement
    assert plan.batches[0].parameters["batch"][0]["email"] is None


def test_partial_defaulted_and_full_groups_use_age_graph_name() -> None:
    session = Session(dialect="age", graph_name="g2")
    plan = session.bulk_upsert(
        GroupedAcct,
        [
            {"acc_no": "P1", "name": "Partial"},
            {"acc_no": "D1"},
            {"acc_no": "F1", "name": "Full", "email": "full@x.io", "status": "ACTIVE"},
        ],
        key_field="acc_no",
    )

    assert len(plan) == 3
    by_key = {}
    for batch in plan.batches:
        row = batch.parameters["batch"][0]
        by_key[row["acc_no"]] = batch.statement
        assert "cypher('g2'," in batch.statement
        assert "age_graph" not in batch.statement

    partial = by_key["P1"]
    partial_create, _, partial_match = partial.partition("ON MATCH SET")
    assert "_bulknullgroupedacct_0.name = row.name" in partial_create
    assert "_bulknullgroupedacct_0.status = row.status" in partial_create
    assert "_bulknullgroupedacct_0.email = row.email" not in partial_create
    assert "_bulknullgroupedacct_0.name = row.name" in partial_match
    assert "_bulknullgroupedacct_0.status = row.status" not in partial_match

    defaulted = by_key["D1"]
    defaulted_create, _, defaulted_match = defaulted.partition("ON MATCH SET")
    assert "_bulknullgroupedacct_0.status = row.status" in defaulted_create
    assert "_bulknullgroupedacct_0.name = row.name" not in defaulted_create
    assert defaulted_match == ""

    full = by_key["F1"]
    full_create, _, full_match = full.partition("ON MATCH SET")
    for prop in ("name", "email", "status"):
        assert f"_bulknullgroupedacct_0.{prop} = row.{prop}" in full_create
        assert f"_bulknullgroupedacct_0.{prop} = row.{prop}" in full_match


def _port_open(host: str, port: int) -> bool:
    try:
        with socket.create_connection((host, port), timeout=0.5):
            return True
    except OSError:
        return False


def _fetch(session: Session) -> dict[str, Any]:
    rows = session.execute(
        "MATCH (a:BulkNullAcctLive205) RETURN a.acc_no AS acc_no, a.name AS name, a.email AS email"
    )
    return {row["acc_no"]: row for row in rows}


def _exercise_partial_merge(uri: str, dialect: str) -> None:
    session = Session(bridge=create_bridge(uri), dialect=dialect)
    try:
        session.execute("MATCH (a:BulkNullAcctLive205) DETACH DELETE a")
        session.run_bulk(
            session.bulk_upsert(
                LiveAcct,
                [{"acc_no": "A1", "name": "Ada", "email": "ada@x.io"}],
                key_field="acc_no",
            )
        )

        session.run_bulk(
            session.bulk_upsert(
                LiveAcct,
                [{"acc_no": "A1", "name": "Ada L."}],
                key_field="acc_no",
            )
        )
        row = _fetch(session)["A1"]
        assert row["name"] == "Ada L."
        assert row["email"] == "ada@x.io"

        session.run_bulk(
            session.bulk_upsert(
                LiveAcct,
                [{"acc_no": "A1", "name": "Ada", "email": "ada@x.io"}],
                key_field="acc_no",
            )
        )
        flush_session = Session(
            bridge=create_bridge(uri), dialect=dialect, enable_identity_map=True
        )
        try:
            tracked = flush_session.register(
                LiveAcct(acc_no="A1", name="Ada", email="ada@x.io"), is_clean=True
            )
            tracked.name = "Ada K."
            flush_session.flush()
        finally:
            flush_session.close()

        row = _fetch(session)["A1"]
        assert row["name"] == "Ada K."
        assert row["email"] == "ada@x.io"
    finally:
        session.execute("MATCH (a:BulkNullAcctLive205) DETACH DELETE a")
        session.close()


@pytest.mark.live
@pytest.mark.skipif(not _port_open("127.0.0.1", 6379), reason="FalkorDB is not running")
def test_live_falkordb_partial_bulk_merge_and_flush() -> None:
    _exercise_partial_merge("falkordb://127.0.0.1:6379", "falkordb")


@pytest.mark.live
@pytest.mark.skipif(not _port_open("127.0.0.1", 7688), reason="Memgraph is not running")
def test_live_memgraph_partial_bulk_merge_and_flush() -> None:
    _exercise_partial_merge("bolt://127.0.0.1:7688", "cypher")
