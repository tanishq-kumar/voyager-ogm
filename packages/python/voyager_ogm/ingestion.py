"""High-throughput bulk ingestion engine for Voyager OGM.

Provides chunking, batch unrolling (`UNWIND $batch AS row`), and zero-copy Polars / Arrow
DataFrame ingestion into graph nodes and relationships.
"""

from __future__ import annotations

from collections.abc import Iterator, Sequence
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from voyager_ogm._voyager_rs import (
    compile_bulk_create as _rs_compile_bulk_create,
)
from voyager_ogm._voyager_rs import (
    compile_bulk_create_rel as _rs_compile_bulk_create_rel,
)
from voyager_ogm._voyager_rs import (
    compile_bulk_merge as _rs_compile_bulk_merge,
)

if TYPE_CHECKING:
    import polars as pl

    from voyager_ogm.models import Node, Relationship


def chunk_records(
    records: Sequence[dict[str, Any]], batch_size: int = 50_000
) -> Iterator[list[dict[str, Any]]]:
    """Yield successive batch chunks from a sequence of dict records.

    Args:
        records: List or sequence of dictionary records.
        batch_size: Maximum number of records per batch chunk.

    Yields:
        Chunked batch of records as a list of dictionaries.
    """
    total = len(records)
    for offset in range(0, total, batch_size):
        yield list(records[offset : offset + batch_size])


def chunk_dataframe(df: Any, batch_size: int = 50_000) -> Iterator[list[dict[str, Any]]]:
    """Yield successive batch chunks from a Polars / PyArrow / Pandas DataFrame.

    Leverages zero-copy slicing for Polars DataFrames to avoid unnecessary memory copies.

    Args:
        df: Polars DataFrame, LazyFrame, PyArrow Table, or pandas DataFrame.
        batch_size: Maximum number of records per batch chunk.

    Yields:
        Chunked batch of records formatted as list of dictionaries.
    """
    if hasattr(df, "iter_slices") and hasattr(df, "to_dicts"):
        total_rows = len(df)
        for offset in range(0, total_rows, batch_size):
            slice_df = df.slice(offset, batch_size)
            yield slice_df.to_dicts()
        return

    if hasattr(df, "collect"):
        collected = df.collect()
        yield from chunk_dataframe(collected, batch_size=batch_size)
        return

    if hasattr(df, "to_pylist") and hasattr(df, "slice"):
        total_rows = df.num_rows
        for offset in range(0, total_rows, batch_size):
            slice_tbl = df.slice(offset, batch_size)
            yield slice_tbl.to_pylist()
        return

    if hasattr(df, "to_dict") and hasattr(df, "iloc"):
        total_rows = len(df)
        for offset in range(0, total_rows, batch_size):
            slice_df = df.iloc[offset : offset + batch_size]
            yield slice_df.to_dict(orient="records")
        return

    if isinstance(df, Sequence):
        yield from chunk_records(df, batch_size=batch_size)
        return

    msg = f"Unsupported data type for chunking: {type(df)}"
    raise TypeError(msg)


@dataclass
class BulkIngestionBatch:
    """Represents a single executable batch chunk within a bulk ingestion plan."""

    batch_index: int
    statement: str
    parameters: dict[str, Any]
    record_count: int


@dataclass
class BulkIngestionPlan:
    """Execution plan for high-throughput bulk ingestion transactions.

    Attributes:
        statement: The parameterized bulk query statement.
        total_records: Total number of rows/records to ingest.
        batch_size: Chunk size per batch transaction.
        dialect: Target query dialect string ('cypher', 'iso_gql').
        batches_data: Pre-sliced list of record chunks.
    """

    statement: str
    total_records: int
    batch_size: int
    dialect: str
    batches_data: list[list[dict[str, Any]]]

    def __iter__(self) -> Iterator[BulkIngestionBatch]:
        """Iterates over executable batch descriptors."""
        for idx, chunk in enumerate(self.batches_data):
            yield BulkIngestionBatch(
                batch_index=idx,
                statement=self.statement,
                parameters={"batch": chunk},
                record_count=len(chunk),
            )

    def __len__(self) -> int:
        """Returns the total number of batches in this plan."""
        return len(self.batches_data)

    @property
    def num_batches(self) -> int:
        """Returns the total number of batches in this plan."""
        return len(self.batches_data)

    @property
    def batches(self) -> list[BulkIngestionBatch]:
        """Returns the list of executable batch descriptors."""
        return list(self)


def create_bulk_create_plan(
    model: type[Node],
    data: list[dict[str, Any]] | pl.DataFrame | Any,
    batch_size: int = 50_000,
    dialect: str = "cypher",
) -> BulkIngestionPlan:
    """Creates a bulk creation execution plan.

    Args:
        model: The Voyager OGM Node class.
        data: Records or DataFrame to ingest.
        batch_size: Number of entities per batch transaction.
        dialect: Target graph query dialect ('cypher', 'iso_gql').

    Returns:
        The generated execution plan with chunked parameter batches.
    """
    labels = getattr(model, "__labels__", [model.__name__])
    label = labels[0] if labels else model.__name__
    fields = getattr(model, "_schema_fields", getattr(model, "__fields__", {}))
    properties = list(fields.keys())

    batches = list(chunk_dataframe(data, batch_size=batch_size))
    total_records = sum(len(b) for b in batches)

    if not properties and batches and batches[0]:
        properties = list(batches[0][0].keys())

    compiled = _rs_compile_bulk_create(
        label=label,
        properties=properties,
        batch_param="batch",
        row_alias="row",
        dialect=dialect,
    )

    return BulkIngestionPlan(
        statement=compiled["statement"],
        total_records=total_records,
        batch_size=batch_size,
        dialect=dialect,
        batches_data=batches,
    )


def create_bulk_merge_plan(
    model: type[Node],
    key_field: str,
    data: list[dict[str, Any]] | pl.DataFrame | Any,
    batch_size: int = 50_000,
    dialect: str = "cypher",
) -> BulkIngestionPlan:
    """Creates an idempotent bulk upsert (MERGE) execution plan.

    Args:
        model: The Voyager OGM Node class.
        key_field: The unique identifier field name to match on.
        data: Records or DataFrame to upsert.
        batch_size: Number of entities per batch transaction.
        dialect: Target graph query dialect ('cypher', 'iso_gql').

    Returns:
        The generated execution plan with chunked parameter batches.
    """
    labels = getattr(model, "__labels__", [model.__name__])
    label = labels[0] if labels else model.__name__
    fields = getattr(model, "_schema_fields", getattr(model, "__fields__", {}))
    properties = [f for f in fields.keys() if f != key_field]

    batches = list(chunk_dataframe(data, batch_size=batch_size))
    total_records = sum(len(b) for b in batches)

    if not properties and batches and batches[0]:
        properties = [k for k in batches[0][0].keys() if k != key_field]

    compiled = _rs_compile_bulk_merge(
        label=label,
        key_property=key_field,
        properties=properties,
        batch_param="batch",
        row_alias="row",
        dialect=dialect,
    )

    return BulkIngestionPlan(
        statement=compiled["statement"],
        total_records=total_records,
        batch_size=batch_size,
        dialect=dialect,
        batches_data=batches,
    )


def create_bulk_create_rel_plan(
    rel_model: type[Relationship] | str,
    data: list[dict[str, Any]] | pl.DataFrame | Any,
    from_label: str,
    from_key: str,
    to_label: str,
    to_key: str,
    batch_size: int = 50_000,
    dialect: str = "cypher",
) -> BulkIngestionPlan:
    """Creates a bulk relationship ingestion execution plan.

    Args:
        rel_model: The relationship model class or relationship type string (e.g. "KNOWS").
        data: Records containing `from_<from_key>`, `to_<to_key>`, and edge properties.
        from_label: Source node label.
        from_key: Source node matching property name (e.g. "id").
        to_label: Target node label.
        to_key: Target node matching property name (e.g. "id").
        batch_size: Number of edges per batch transaction.
        dialect: Target graph query dialect ('cypher', 'iso_gql').

    Returns:
        The generated execution plan.
    """
    if isinstance(rel_model, str):
        rel_type = rel_model
        properties = []
    else:
        rel_type = getattr(
            rel_model,
            "__type__",
            getattr(rel_model, "__rel_type__", rel_model.__name__.upper()),
        )
        fields = getattr(rel_model, "_schema_fields", getattr(rel_model, "__fields__", {}))
        properties = list(fields.keys())

    batches = list(chunk_dataframe(data, batch_size=batch_size))
    total_records = sum(len(b) for b in batches)

    compiled = _rs_compile_bulk_create_rel(
        rel_type=rel_type,
        from_label=from_label,
        from_key=from_key,
        to_label=to_label,
        to_key=to_key,
        properties=properties,
        batch_param="batch",
        row_alias="row",
        dialect=dialect,
    )

    return BulkIngestionPlan(
        statement=compiled["statement"],
        total_records=total_records,
        batch_size=batch_size,
        dialect=dialect,
        batches_data=batches,
    )
