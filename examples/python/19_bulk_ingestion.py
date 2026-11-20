"""Voyager OGM: Ingestion & Bulk Loading (CSV, Parquet, UNWIND Batches).

Teaches:
    - High-throughput bulk ingestion using UNWIND batching
    - Ingesting local CSV data into graph entities (`session.ingest_csv()`)
    - Ingesting local Parquet files into graph entities (`session.ingest_parquet()`)
    - Ingesting relationships from files (`session.ingest_parquet_relationships()`)
    - In-memory bulk node insertion from Polars DataFrames (`session.bulk_create()`)
    - Bulk relationship creation linking nodes by key (`session.bulk_create_relationships()`)
    - Partial upserts: omitted properties keep stored values; explicit None clears them
    - DataFrame columns count as supplied even when an individual cell is null
    - Inspecting execution plans and executing via `session.run_bulk()`

Prerequisites:
    - Engine: None (runs in-memory / offline via MockBridge)
    - Extras: None

Run with:
    `uv run python examples/python/19_bulk_ingestion.py`
"""

from __future__ import annotations

import tempfile
from pathlib import Path

import polars as pl
from voyager_ogm import (
    Field,
    MockBridge,
    Node,
    Relationship,
    Session,
    create_csv_ingest_plan,
    node,
    relationship,
)


@node(label="User")
class User(Node):
    id: int = Field(primary_key=True)
    username: str = Field()
    department: str = Field()
    score: float = Field(default=0.0)


@relationship(type_name="COLLABORATES_WITH")
class CollaboratesWith(Relationship):
    project: str = Field()
    hours: int = Field(default=10)


def main() -> None:
    print("=== Voyager OGM: Ingestion & Bulk Loading ===\n")

    mock_bridge = MockBridge()
    session = Session(bridge=mock_bridge, dialect="cypher")

    with tempfile.TemporaryDirectory() as tmp_dir_str:
        tmp_dir = Path(tmp_dir_str)

        # ---------------------------------------------------------------------
        # 1. Ingesting from Local CSV Files
        # ---------------------------------------------------------------------
        print("--- [1] Local CSV Ingestion (session.ingest_csv) ---")
        csv_file = tmp_dir / "users.csv"
        df_csv = pl.DataFrame(
            {
                "id": [101, 102, 103, 104, 105],
                "username": ["alice", "bob", "carol", "dan", "erin"],
                "department": ["AI", "Eng", "AI", "Design", "Eng"],
                "score": [95.5, 88.0, 91.2, 79.4, 85.0],
            }
        )
        df_csv.write_csv(csv_file)

        # Inspect plan generated for CSV
        csv_plan = create_csv_ingest_plan(User, csv_file, batch_size=2)
        print(f"Total CSV records: {csv_plan.total_records}, Batches: {csv_plan.num_batches}")
        print(f"Generated UNWIND Statement:\n  {csv_plan.statement}\n")

        # Execute ingestion via Session
        res_csv = session.ingest_csv(csv_file, User, mode="create", batch_size=2)
        print(
            f"Bulk execution completed: {res_csv.total_records} records across {res_csv.total_batches} batches.\n"
        )

        # ---------------------------------------------------------------------
        # 2. Ingesting from Local Parquet Files
        # ---------------------------------------------------------------------
        print("--- [2] Local Parquet Ingestion (session.ingest_parquet) ---")
        parquet_file = tmp_dir / "users.parquet"
        df_parquet = pl.DataFrame(
            {
                "id": [201, 202, 203],
                "username": ["frank", "grace", "heidi"],
                "department": ["Security", "Security", "AI"],
                "score": [99.0, 94.5, 92.0],
            }
        )
        df_parquet.write_parquet(parquet_file)

        res_parquet = session.ingest_parquet(
            parquet_file, User, mode="upsert", key_field="id", batch_size=2
        )
        print(
            f"Total Parquet records: {res_parquet.total_records}, Batches: {res_parquet.total_batches}"
        )
        print(f"Execution completed in {res_parquet.duration_seconds:.5f}s.\n")

        # ---------------------------------------------------------------------
        # 3. In-Memory Bulk Creation from Polars DataFrame
        # ---------------------------------------------------------------------
        print("--- [3] In-Memory DataFrame Bulk Creation (session.bulk_create) ---")
        df_bulk = pl.DataFrame(
            {
                "id": list(range(1000, 1050)),
                "username": [f"user_{i}" for i in range(1000, 1050)],
                "department": [
                    "Engineering" if i % 2 == 0 else "Research" for i in range(1000, 1050)
                ],
                "score": [70.0 + (i % 30) for i in range(1000, 1050)],
            }
        )

        bulk_plan = session.bulk_create(User, df_bulk, batch_size=25)
        print(f"Total records: {bulk_plan.total_records}, Num batches: {bulk_plan.num_batches}")
        res_bulk = session.run_bulk(bulk_plan)
        print(f"Bulk created {res_bulk.total_records} nodes in {res_bulk.total_batches} batches.\n")

        # ---------------------------------------------------------------------
        # 4. Bulk Linking Relationships by Key
        # ---------------------------------------------------------------------
        print("--- [4] Bulk Relationship Creation (session.bulk_create_relationships) ---")
        edges_df = pl.DataFrame(
            {
                "from_id": [101, 101, 102],
                "to_id": [102, 103, 105],
                "project": ["Apollo", "Voyager", "Matrix"],
                "hours": [40, 60, 25],
            }
        )

        rel_plan = session.bulk_create_relationships(
            CollaboratesWith,
            edges_df,
            from_label="User",
            from_key="id",
            to_label="User",
            to_key="id",
            batch_size=50,
        )
        print(f"Relationship Statement:\n  {rel_plan.statement}")
        res_rel = session.run_bulk(rel_plan)
        print(f"Bulk linked {res_rel.total_records} graph edges.\n")

        # ---------------------------------------------------------------------
        # 5. Partial Upserts and Explicit Nulls
        # ---------------------------------------------------------------------
        print("--- [5] Partial Upserts (session.bulk_upsert) ---")
        keep_plan = session.bulk_upsert(
            User, [{"id": 101, "username": "alice_renamed"}], key_field="id"
        )
        print("A row that omits 'department' leaves its stored value untouched:")
        print(f"  {keep_plan.statement}")

        clear_plan = session.bulk_upsert(User, [{"id": 102, "department": None}], key_field="id")
        print("A row that supplies 'department': None clears the stored value:")
        print(f"  {clear_plan.statement}")

        print(
            "\nDataFrame columns always count as supplied: a null cell clears the property, "
            "while an absent column is not written."
        )
        partial_df = pl.DataFrame({"id": [201, 202], "username": ["frank", None]})
        partial_df_plan = session.bulk_upsert(User, partial_df, key_field="id")
        print("DataFrame statement ('username' is set from the column; 'department' is absent):")
        print(f"  {partial_df_plan.statement}\n")

    print("[SUCCESS] Ingestion & bulk loading demonstration completed successfully.")


if __name__ == "__main__":
    main()
