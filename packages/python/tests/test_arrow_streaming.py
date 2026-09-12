"""Tests for Apache Arrow C-Stream & Polars Streaming Ingestion in Voyager OGM."""

from __future__ import annotations

import polars as pl
import pyarrow as pa
from voyager_ogm import (
    ArrowStream,
    QueryResult,
    generate_synthetic_stream,
    to_arrow,
    to_polars,
)


def test_arrow_stream_metadata():
    stream = generate_synthetic_stream(1000)
    assert isinstance(stream, ArrowStream)
    assert stream.num_rows == 1000
    assert stream.num_columns == 6


def test_to_arrow_table_ingestion():
    stream = generate_synthetic_stream(5000)
    table = to_arrow(stream)

    assert isinstance(table, pa.Table)
    assert table.num_rows == 5000
    assert table.num_columns == 6
    assert table.column_names == ["id", "label", "name", "age", "score", "active"]
    assert table.column("name")[0].as_py() == "Person_0"
    assert table.column("age")[0].as_py() == 20


def test_to_polars_dataframe_ingestion():
    stream = generate_synthetic_stream(10000)
    df = to_polars(stream)

    assert isinstance(df, pl.DataFrame)
    assert df.height == 10000
    assert df.width == 6
    assert df.columns == ["id", "label", "name", "age", "score", "active"]

    # Verify column datatypes
    assert df.schema["id"] == pl.Int64
    assert df.schema["label"] == pl.String
    assert df.schema["name"] == pl.String
    assert df.schema["age"] == pl.Int64
    assert df.schema["score"] == pl.Float64
    assert df.schema["active"] == pl.Boolean

    # Verify values and filtering in Polars
    seniors = df.filter(pl.col("age") > 65)
    assert seniors.height > 0


def test_query_result_wrapper():
    stream = generate_synthetic_stream(1000)
    result = QueryResult(stream)

    assert result.num_rows == 1000
    assert result.num_columns == 6

    df = result.to_polars()
    assert isinstance(df, pl.DataFrame)
    assert df.height == 1000

    dicts = result.to_dicts()
    assert len(dicts) == 1000
    assert dicts[0]["name"] == "Person_0"


def test_arrow_stream_iter_batches():
    stream = generate_synthetic_stream(1000)
    chunks = stream.iter_batches(batch_size=250)
    assert len(chunks) == 4
    for chunk in chunks:
        assert chunk.num_rows == 250
        assert chunk.num_columns == 6


def test_execution_result_iter_batches():
    from voyager_ogm.session import ExecutionResult

    stream = generate_synthetic_stream(1000)
    res = ExecutionResult(stream=stream)

    # Test dicts format
    batch_count = 0
    total_rows = 0
    for batch in res.iter_batches(batch_size=300, as_format="dicts"):
        assert isinstance(batch, list)
        batch_count += 1
        total_rows += len(batch)
        assert len(batch) <= 300
    assert batch_count == 4
    assert total_rows == 1000

    # Test polars format on fresh synthetic stream
    stream2 = generate_synthetic_stream(1000)
    res2 = ExecutionResult(stream=stream2)
    df_batches = list(res2.iter_batches(batch_size=400, as_format="polars"))
    assert len(df_batches) == 3
    assert df_batches[0].height == 400
    assert df_batches[1].height == 400
    assert df_batches[2].height == 200

    # Test arrow format on fresh synthetic stream
    stream3 = generate_synthetic_stream(1000)
    res3 = ExecutionResult(stream=stream3)
    arrow_batches = list(res3.iter_batches(batch_size=500, as_format="arrow"))
    assert len(arrow_batches) == 2
    assert arrow_batches[0].num_rows == 500
    assert arrow_batches[1].num_rows == 500


def test_query_compilation_cache():
    from voyager_ogm._voyager_rs import (
        clear_query_cache,
        compile_query_from_spec,
        get_query_cache_stats,
    )

    clear_query_cache()
    stats_initial = get_query_cache_stats()
    assert stats_initial["len"] == 0

    spec = {
        "matches": [
            {
                "paths": [[("node", "p", ["Person"], None)]],
            }
        ],
    }

    # First compile: cache miss
    res1 = compile_query_from_spec(spec, "cypher")
    assert "MATCH (p:Person)" in res1["statement"]
    stats_after_first = get_query_cache_stats()
    assert stats_after_first["len"] == 1
    assert stats_after_first["misses"] == 1
    assert stats_after_first["hits"] == 0

    # Second compile: cache hit!
    res2 = compile_query_from_spec(spec, "cypher")
    assert res2["statement"] == res1["statement"]
    stats_after_second = get_query_cache_stats()
    assert stats_after_second["len"] == 1
    assert stats_after_second["hits"] == 1
    assert stats_after_second["hit_ratio"] == 0.5
