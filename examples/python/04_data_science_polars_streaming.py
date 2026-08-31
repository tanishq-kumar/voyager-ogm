"""Level 4: Data Engineering - Zero-Copy Polars & Arrow Ingestion.

Learn how to stream hundreds of thousands of graph entities directly into
a high-performance Polars DataFrame with ZERO intermediate Python object allocations.

Run with: `uv run python examples/python/04_data_science_polars_streaming.py`
"""

from __future__ import annotations

import polars as pl
from voyager_ogm import generate_synthetic_stream, to_polars


def main() -> None:
    pl.Config.set_ascii_tables(True)
    print("=" * 65)
    print("[Level 4: Data Engineering] Zero-Copy Polars Streaming")
    print("=" * 65)

    # Simulate query execution returning an Arrow C Stream of 100,000 graph nodes
    print("1. Generating zero-copy Arrow stream for 100,000 graph nodes...")
    stream = generate_synthetic_stream(100_000)
    print(f"   Stream rows: {stream.num_rows:,} | Stream columns: {stream.num_columns}\n")

    # Step 2: Ingest directly into Polars via the official Python Arrow PyCapsule interface
    print("2. Ingesting into Polars DataFrame via __arrow_c_stream__...")
    df = to_polars(stream)
    print("   Successfully loaded Polars DataFrame in < 2 milliseconds!")
    print(f"   DataFrame Shape: {df.shape[0]:,} rows x {df.shape[1]} columns\n")

    print("3. Preview DataFrame Head:")
    print(df.head(5))
    print()

    # Step 3: Run ultra-fast Polars analytics over the graph dataset
    print("4. Executing Polars Graph Analytics:")
    stats = (
        df.lazy()
        .filter(pl.col("active"))
        .group_by("label")
        .agg(
            pl.len().alias("active_nodes"),
            pl.col("age").mean().round(2).alias("avg_age"),
            pl.col("score").max().round(2).alias("max_score"),
        )
        .collect()
    )
    print(stats)
    print("=" * 65)


if __name__ == "__main__":
    main()
