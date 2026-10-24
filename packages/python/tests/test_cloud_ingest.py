"""Unit tests for local file and cloud ingestion loaders (Issue #110)."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock

import polars as pl
import pytest
from voyager_ogm import (
    AsyncMockBridge,
    AsyncSession,
    Field,
    MockBridge,
    Node,
    Relationship,
    Session,
    create_csv_ingest_plan,
    create_csv_rel_ingest_plan,
    create_parquet_ingest_plan,
    create_parquet_rel_ingest_plan,
    ingest_csv,
    ingest_csv_relationships,
    ingest_parquet,
    ingest_parquet_relationships,
    node,
    relationship,
    reset_alias_counters,
)


@node("User")
class User(Node):
    id: Field[int] = Field()
    name: Field[str] = Field()
    age: Field[int] = Field()


@relationship("FRIENDS_WITH")
class FriendsWith(Relationship):
    since: Field[int] = Field()


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


@pytest.fixture
def sample_parquet(tmp_path: Path) -> Path:
    file_path = tmp_path / "users.parquet"
    df = pl.DataFrame(
        {
            "id": [1, 2, 3, 4, 5],
            "name": ["Alice", "Bob", "Charlie", "Diana", "Eve"],
            "age": [25, 30, 35, 28, 22],
        }
    )
    df.write_parquet(file_path)
    return file_path


@pytest.fixture
def sample_csv(tmp_path: Path) -> Path:
    file_path = tmp_path / "users.csv"
    df = pl.DataFrame(
        {
            "id": [10, 20, 30],
            "name": ["Xavier", "Yolanda", "Zack"],
            "age": [40, 42, 45],
        }
    )
    df.write_csv(file_path)
    return file_path


@pytest.fixture
def sample_rel_parquet(tmp_path: Path) -> Path:
    file_path = tmp_path / "relations.parquet"
    df = pl.DataFrame(
        {
            "from_id": [1, 2],
            "to_id": [2, 3],
            "since": [2021, 2022],
        }
    )
    df.write_parquet(file_path)
    return file_path


@pytest.fixture
def sample_rel_csv(tmp_path: Path) -> Path:
    file_path = tmp_path / "relations.csv"
    df = pl.DataFrame(
        {
            "src_user": [10, 20],
            "dst_user": [20, 30],
            "since": [2023, 2024],
        }
    )
    df.write_csv(file_path)
    return file_path


def test_create_parquet_ingest_plan_create_mode(sample_parquet: Path):
    plan = create_parquet_ingest_plan(
        model=User,
        source=sample_parquet,
        mode="create",
        batch_size=2,
        dialect="cypher",
    )

    assert plan.total_records == 5
    assert plan.num_batches == 3
    assert plan.statement == (
        "UNWIND $batch AS row CREATE (_user_0:User) "
        "SET _user_0.id = row.id, _user_0.name = row.name, _user_0.age = row.age"
    )
    batches = list(plan)
    assert len(batches) == 3
    assert len(batches[0].parameters["batch"]) == 2
    assert batches[0].parameters["batch"][0]["name"] == "Alice"


def test_create_parquet_ingest_plan_upsert_mode(sample_parquet: Path):
    plan = create_parquet_ingest_plan(
        model=User,
        source=str(sample_parquet),
        mode="upsert",
        key_field="id",
        batch_size=10,
        dialect="cypher",
    )

    assert plan.total_records == 5
    assert plan.num_batches == 1
    assert plan.statement == (
        "UNWIND $batch AS row MERGE (_user_0:User {id: row.id}) "
        "ON CREATE SET _user_0.name = row.name, _user_0.age = row.age "
        "ON MATCH SET _user_0.name = row.name, _user_0.age = row.age"
    )


def test_create_parquet_ingest_plan_invalid_mode(sample_parquet: Path):
    with pytest.raises(ValueError, match="Unsupported ingestion mode: 'invalid'"):
        create_parquet_ingest_plan(
            model=User,
            source=sample_parquet,
            mode="invalid",  # type: ignore[arg-type]
        )


def test_create_csv_ingest_plan_create_and_upsert(sample_csv: Path):
    plan_create = create_csv_ingest_plan(
        model=User,
        source=sample_csv,
        mode="create",
        batch_size=2,
    )
    assert plan_create.total_records == 3
    assert plan_create.num_batches == 2
    assert "CREATE (_user_0:User)" in plan_create.statement

    plan_upsert = create_csv_ingest_plan(
        model=User,
        source=str(sample_csv),
        mode="upsert",
        key_field="id",
    )
    assert plan_upsert.total_records == 3
    assert "MERGE (_user_0:User {id: row.id})" in plan_upsert.statement


def test_create_csv_ingest_plan_invalid_mode(sample_csv: Path):
    with pytest.raises(ValueError, match="Unsupported ingestion mode: 'drop'"):
        create_csv_ingest_plan(
            model=User,
            source=sample_csv,
            mode="drop",  # type: ignore[arg-type]
        )


def test_create_parquet_rel_ingest_plan(sample_rel_parquet: Path):
    plan = create_parquet_rel_ingest_plan(
        rel_model=FriendsWith,
        source=sample_rel_parquet,
        from_label="User",
        from_key="id",
        to_label="User",
        to_key="id",
        batch_size=50_000,
    )

    assert plan.total_records == 2
    assert plan.statement == (
        "UNWIND $batch AS row "
        "MATCH (_from_user_0:User {id: row.from_id}), (_to_user_0:User {id: row.to_id}) "
        "CREATE (_from_user_0)-[_friends_with_0:FRIENDS_WITH]->(_to_user_0) "
        "SET _friends_with_0.since = row.since"
    )


def test_create_csv_rel_ingest_plan_with_field_mapping(sample_rel_csv: Path):
    plan = create_csv_rel_ingest_plan(
        rel_model="KNOWS",
        source=sample_rel_csv,
        from_label="Person",
        from_key="id",
        to_label="Person",
        to_key="id",
        from_source_field="src_user",
        to_source_field="dst_user",
        properties=["since"],
    )

    assert plan.total_records == 2
    assert plan.statement == (
        "UNWIND $batch AS row "
        "MATCH (_from_person_0:Person {id: row.from_id}), (_to_person_0:Person {id: row.to_id}) "
        "CREATE (_from_person_0)-[_knows_0:KNOWS]->(_to_person_0) "
        "SET _knows_0.since = row.since"
    )
    batches = list(plan)
    assert "from_id" in batches[0].parameters["batch"][0]
    assert "to_id" in batches[0].parameters["batch"][0]
    assert batches[0].parameters["batch"][0]["from_id"] == 10
    assert batches[0].parameters["batch"][0]["to_id"] == 20


def test_multi_source_list_paths(tmp_path: Path):
    p1 = tmp_path / "part1.parquet"
    p2 = tmp_path / "part2.parquet"
    pl.DataFrame({"id": [1], "name": ["A"], "age": [10]}).write_parquet(p1)
    pl.DataFrame({"id": [2], "name": ["B"], "age": [20]}).write_parquet(p2)

    plan = create_parquet_ingest_plan(User, [p1, p2], mode="create")
    assert plan.total_records == 2


def test_storage_options_forwarding(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    mock_scan_parquet = MagicMock(
        return_value=pl.LazyFrame({"id": [1], "name": ["A"], "age": [20]})
    )
    mock_scan_csv = MagicMock(return_value=pl.LazyFrame({"id": [2], "name": ["B"], "age": [22]}))

    monkeypatch.setattr(pl, "scan_parquet", mock_scan_parquet)
    monkeypatch.setattr(pl, "scan_csv", mock_scan_csv)

    cloud_s3 = "s3://my-bucket/data.parquet"
    storage_opts = {"aws_access_key_id": "test_key", "aws_secret_access_key": "test_secret"}

    create_parquet_ingest_plan(User, cloud_s3, storage_options=storage_opts)
    mock_scan_parquet.assert_called_once_with(cloud_s3, storage_options=storage_opts)

    cloud_az = "az://my-container/data.csv"
    create_csv_ingest_plan(User, cloud_az, storage_options=storage_opts)
    mock_scan_csv.assert_called_once_with(cloud_az, storage_options=storage_opts)


def test_session_ingest_parquet_and_csv(sample_parquet: Path, sample_csv: Path):
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher")

    res_parquet = session.ingest_parquet(sample_parquet, User, mode="create", batch_size=2)
    assert res_parquet.total_records == 5
    assert res_parquet.total_batches == 3

    res_csv = session.ingest_csv(sample_csv, User, mode="upsert", key_field="id")
    assert res_csv.total_records == 3
    assert res_csv.total_batches == 1

    assert len(bridge.executed_queries) == 4


def test_session_ingest_relationships(sample_rel_parquet: Path, sample_rel_csv: Path):
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher")

    res_rel_parquet = session.ingest_parquet_relationships(
        sample_rel_parquet,
        rel_model=FriendsWith,
        from_label="User",
        from_key="id",
        to_label="User",
        to_key="id",
    )
    assert res_rel_parquet.total_records == 2

    res_rel_csv = session.ingest_csv_relationships(
        sample_rel_csv,
        rel_model="KNOWS",
        from_label="User",
        from_key="id",
        to_label="User",
        to_key="id",
        from_source_field="src_user",
        to_source_field="dst_user",
    )
    assert res_rel_csv.total_records == 2
    assert len(bridge.executed_queries) == 2


@pytest.mark.asyncio
async def test_async_session_ingest(
    sample_parquet: Path,
    sample_csv: Path,
    sample_rel_parquet: Path,
    sample_rel_csv: Path,
):
    bridge = AsyncMockBridge()
    session = AsyncSession(bridge=bridge, dialect="cypher")

    res_parquet = await session.ingest_parquet(sample_parquet, User, mode="create")
    assert res_parquet.total_records == 5

    res_csv = await session.ingest_csv(sample_csv, User, mode="upsert", key_field="id")
    assert res_csv.total_records == 3

    res_rel_p = await session.ingest_parquet_relationships(
        sample_rel_parquet,
        rel_model=FriendsWith,
        from_label="User",
        from_key="id",
        to_label="User",
        to_key="id",
    )
    assert res_rel_p.total_records == 2

    res_rel_c = await session.ingest_csv_relationships(
        sample_rel_csv,
        rel_model="KNOWS",
        from_label="User",
        from_key="id",
        to_label="User",
        to_key="id",
        from_source_field="src_user",
        to_source_field="dst_user",
    )
    assert res_rel_c.total_records == 2
    assert len(bridge.sync_mock.executed_queries) == 4


def test_top_level_functional_helpers(sample_parquet: Path, sample_rel_parquet: Path):
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher")

    res_p = ingest_parquet(session, sample_parquet, User)
    assert res_p.total_records == 5

    res_rel = ingest_parquet_relationships(
        session,
        sample_rel_parquet,
        rel_model=FriendsWith,
        from_label="User",
        from_key="id",
        to_label="User",
        to_key="id",
    )
    assert res_rel.total_records == 2


def test_top_level_csv_helpers(sample_csv: Path, sample_rel_csv: Path):
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher")

    res_c = ingest_csv(session, sample_csv, User)
    assert res_c.total_records == 3

    res_rel = ingest_csv_relationships(
        session,
        sample_rel_csv,
        rel_model="KNOWS",
        from_label="User",
        from_key="id",
        to_label="User",
        to_key="id",
        from_source_field="src_user",
        to_source_field="dst_user",
    )
    assert res_rel.total_records == 2
