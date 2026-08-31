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

    Parameters
    ----------
    records : Sequence[dict[str, Any]]
        List or sequence of dictionary records.
    batch_size : int, default=50_000
        Maximum number of records per batch chunk.

    Yields
    ------
    list[dict[str, Any]]
        Chunked batch of records.
    """
    total = len(records)
    for offset in range(0, total, batch_size):
        yield list(records[offset : offset + batch_size])


def chunk_dataframe(df: Any, batch_size: int = 50_000) -> Iterator[list[dict[str, Any]]]:
    """Yield successive batch chunks from a Polars / PyArrow / Pandas DataFrame.

    Leverages zero-copy slicing for Polars DataFrames to avoid unnecessary memory copies.

    Parameters
    ----------
    df : Any
        Polars DataFrame, LazyFrame, PyArrow Table, or pandas DataFrame.
    batch_size : int, default=50_000
        Maximum number of records per batch chunk.

    Yields
    ------
    list[dict[str, Any]]
        Chunked batch of records formatted as list of dictionaries.
    """
    # Polars DataFrame
    if hasattr(df, "iter_slices") and hasattr(df, "to_dicts"):
        total_rows = len(df)
        for offset in range(0, total_rows, batch_size):
            slice_df = df.slice(offset, batch_size)
            yield slice_df.to_dicts()
        return

    # Polars LazyFrame
    if hasattr(df, "collect"):
        collected = df.collect()
        yield from chunk_dataframe(collected, batch_size=batch_size)
        return

    # PyArrow Table
    if hasattr(df, "to_pylist") and hasattr(df, "slice"):
        total_rows = df.num_rows
        for offset in range(0, total_rows, batch_size):
            slice_tbl = df.slice(offset, batch_size)
            yield slice_tbl.to_pylist()
        return

    # Pandas DataFrame
    if hasattr(df, "to_dict") and hasattr(df, "iloc"):
        total_rows = len(df)
        for offset in range(0, total_rows, batch_size):
            slice_df = df.iloc[offset : offset + batch_size]
            yield slice_df.to_dict(orient="records")
        return

    # Fallback for sequence of dicts
    if isinstance(df, Sequence):
        yield from chunk_records(df, batch_size=batch_size)
        return

    msg = f"Unsupported DataFrame/data structure for bulk ingestion: {type(df)}"
    raise TypeError(msg)


@dataclass(frozen=True)
class BulkIngestionBatch:
    """A single compiled batch statement with parameters."""

    statement: str
    parameters: dict[str, Any]
    batch_index: int
    batch_size: int


@dataclass(frozen=True)
class BulkIngestionPlan:
    """A compiled bulk ingestion execution plan."""

    statement: str
    total_records: int
    batch_size: int
    dialect: str
    batches_data: list[list[dict[str, Any]]]

    def __iter__(self) -> Iterator[BulkIngestionBatch]:
        """Iterate over prepared batch queries."""
        for idx, batch_rows in enumerate(self.batches_data):
            yield BulkIngestionBatch(
                statement=self.statement,
                parameters={"batch": batch_rows},
                batch_index=idx,
                batch_size=len(batch_rows),
            )

    @property
    def num_batches(self) -> int:
        """Total number of batches in this plan."""
        return len(self.batches_data)


def create_bulk_create_plan(
    model: type[Node],
    data: list[dict[str, Any]] | pl.DataFrame | Any,
    batch_size: int = 50_000,
    dialect: str = "cypher",
) -> BulkIngestionPlan:
    """Creates a high-performance bulk creation execution plan.

    Parameters
    ----------
    model : type[Node]
        The Voyager OGM Node class.
    data : list[dict[str, Any]] | pl.DataFrame
        Records or DataFrame to ingest.
    batch_size : int, default=50_000
        Number of entities per batch transaction.
    dialect : str, default="cypher"
        Target graph query dialect ('cypher', 'iso_gql').

    Returns
    -------
    BulkIngestionPlan
        The generated execution plan with chunked parameter batches.
    """
    labels = getattr(model, "__labels__", [model.__name__])
    label = labels[0] if labels else model.__name__
    fields = getattr(model, "__fields__", {})
    properties = list(fields.keys())

    batches = list(chunk_dataframe(data, batch_size=batch_size))
    total_records = sum(len(b) for b in batches)

    # If no fields were explicitly defined on model, infer from first record
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
    """Creates a high-performance idempotent bulk upsert (MERGE) execution plan.

    Parameters
    ----------
    model : type[Node]
        The Voyager OGM Node class.
    key_field : str
        The unique identifier field name to match on.
    data : list[dict[str, Any]] | pl.DataFrame
        Records or DataFrame to upsert.
    batch_size : int, default=50_000
        Number of entities per batch transaction.
    dialect : str, default="cypher"
        Target graph query dialect ('cypher', 'iso_gql').

    Returns
    -------
    BulkIngestionPlan
        The generated execution plan with chunked parameter batches.
    """
    labels = getattr(model, "__labels__", [model.__name__])
    label = labels[0] if labels else model.__name__
    fields = getattr(model, "__fields__", {})
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
    """Creates a high-performance bulk relationship ingestion execution plan.

    Parameters
    ----------
    rel_model : type[Relationship] | str
        The relationship model class or relationship type string (e.g. "KNOWS").
    data : list[dict[str, Any]] | pl.DataFrame
        Records containing `from_<from_key>`, `to_<to_key>`, and edge properties.
    from_label : str
        Source node label.
    from_key : str
        Source node matching property name (e.g. "id").
    to_label : str
        Target node label.
    to_key : str
        Target node matching property name (e.g. "id").
    batch_size : int, default=50_000
        Number of edges per batch transaction.
    dialect : str, default="cypher"
        Target graph query dialect ('cypher', 'iso_gql').

    Returns
    -------
    BulkIngestionPlan
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
        fields = getattr(rel_model, "__fields__", {})
        properties = list(fields.keys())

    batches = list(chunk_dataframe(data, batch_size=batch_size))
    total_records = sum(len(b) for b in batches)

    if not properties and batches and batches[0]:
        from_col = f"from_{from_key}"
        to_col = f"to_{to_key}"
        properties = [k for k in batches[0][0].keys() if k not in (from_col, to_col)]

    compiled = _rs_compile_bulk_create_rel(
        rel_type=rel_type,
        properties=properties,
        from_label=from_label,
        from_key=from_key,
        to_label=to_label,
        to_key=to_key,
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
