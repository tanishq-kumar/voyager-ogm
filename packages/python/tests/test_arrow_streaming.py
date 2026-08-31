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
