"""High-throughput bulk ingestion engine for Voyager OGM.

Provides chunking, batch unrolling (`UNWIND $batch AS row`), and zero-copy Polars / Arrow
DataFrame ingestion into graph nodes and relationships.
"""

from __future__ import annotations

from collections.abc import Iterable, Iterator, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal

import polars as pl

from voyager_ogm._voyager_rs import (
    compile_bulk_create as _rs_compile_bulk_create,
)
from voyager_ogm._voyager_rs import (
    compile_bulk_create_rel as _rs_compile_bulk_create_rel,
)
from voyager_ogm._voyager_rs import (
    compile_bulk_merge_create_only as _rs_compile_bulk_merge,
)
from voyager_ogm.types import serialize_param_value

if TYPE_CHECKING:
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
            # Pandas represents missing scalars as NaN/NaT/pd.NA. Convert them to
            # None so a null cell is an explicit null rather than an opaque value;
            # the column itself still counts as supplied.
            normalized = slice_df.astype(object).where(slice_df.notna(), None)
            yield normalized.to_dict(orient="records")
        return

    if isinstance(df, Sequence):
        yield from chunk_records(df, batch_size=batch_size)
        return

    msg = f"Unsupported data type for chunking: {type(df)}"
    raise TypeError(msg)


def _prepare_batch_records(
    raw_batches: Iterable[list[dict[str, Any]]],
    fields: dict[str, Any],
    defaulted_fields: Sequence[set[str]] | None = None,
) -> list[list[tuple[dict[str, Any], frozenset[str]]]]:
    """Materialises serialized records with defaults and per-record create-only fields.

    Records are copied before defaults are applied so caller-owned dictionaries
    are never mutated. Defaults are applied through the same model-level helper
    used by instance construction, keeping one source of truth.

    Args:
        raw_batches: Chunked input records.
        fields: Declared field descriptors for the target model.
        defaulted_fields: Optional per-record sets naming values that were
            already filled from a default (for example, by an entity instance).

    Returns:
        One list per input chunk. Each entry pairs the serialized record with
        the set of fields that came from a default for that record only.
    """
    from voyager_ogm.models import _apply_field_defaults

    markers = iter(defaulted_fields) if defaulted_fields is not None else None
    prepared: list[list[tuple[dict[str, Any], frozenset[str]]]] = []
    for chunk in raw_batches:
        batch: list[tuple[dict[str, Any], frozenset[str]]] = []
        for record in chunk:
            enriched = dict(record)
            filled = _apply_field_defaults(enriched, fields) if fields else set()
            marker = next(markers, None) if markers is not None else None
            create_only = set(filled)
            if marker:
                create_only.update(k for k in marker if k in enriched)
            batch.append((serialize_param_value(enriched), frozenset(create_only)))
        prepared.append(batch)
    return prepared


def _prepare_batches(
    raw_batches: Iterable[list[dict[str, Any]]],
    fields: dict[str, Any],
    defaulted_fields: Sequence[set[str]] | None = None,
) -> list[list[dict[str, Any]]]:
    """Materialises serialized batches, filling declared field defaults.

    Args:
        raw_batches: Chunked input records.
        fields: Declared field descriptors for the target model.
        defaulted_fields: Optional per-record sets naming values that were
            already filled from a default (for example, by an entity instance).

    Returns:
        The serialized batches, preserving the input chunk boundaries.
    """
    return [
        [record for record, _ in chunk]
        for chunk in _prepare_batch_records(raw_batches, fields, defaulted_fields)
    ]


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
        statement: The parameterized bulk query statement. For plans whose
            records require more than one statement (for example, a merge batch
            mixing explicit and default-derived values), this is the statement
            of the first executable batch.
        total_records: Total number of rows/records to ingest.
        batch_size: Chunk size per batch transaction.
        dialect: Target query dialect string ('cypher', 'iso_gql').
        batches_data: Pre-sliced list of record chunks.
        batch_statements: Optional statement per batch entry. Executors must
            use the statement carried by each iterated batch so grouped plans
            run the statement that matches their rows.
    """

    statement: str
    total_records: int
    batch_size: int
    dialect: str
    batches_data: list[list[dict[str, Any]]]
    batch_statements: list[str] | None = None

    def __iter__(self) -> Iterator[BulkIngestionBatch]:
        """Iterates over executable batch descriptors."""
        for idx, chunk in enumerate(self.batches_data):
            statement = (
                self.batch_statements[idx] if self.batch_statements is not None else self.statement
            )
            yield BulkIngestionBatch(
                batch_index=idx,
                statement=statement,
                parameters={"batch": [serialize_param_value(r) for r in chunk]},
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
    graph_name: str | None = None,
) -> BulkIngestionPlan:
    """Creates a bulk creation execution plan.

    Args:
        model: The Voyager OGM Node class.
        data: Records or DataFrame to ingest.
        batch_size: Number of entities per batch transaction.
        dialect: Target graph query dialect ('cypher', 'iso_gql').
        graph_name: Optional graph name for Apache AGE or SQL:2023 PGQ.

    Returns:
        The generated execution plan with chunked parameter batches.
    """
    labels = getattr(model, "__labels__", [model.__name__])
    label = labels[0] if labels else model.__name__
    fields = getattr(model, "_schema_fields", getattr(model, "__fields__", {}))
    properties = list(fields.keys())

    raw_batches = list(chunk_dataframe(data, batch_size=batch_size))
    batches = _prepare_batches(raw_batches, fields)
    total_records = sum(len(b) for b in batches)

    if not properties and batches and batches[0]:
        properties = list(batches[0][0].keys())

    compiled = _rs_compile_bulk_create(
        label=label,
        properties=properties,
        batch_param="batch",
        row_alias="row",
        dialect=dialect,
        graph_name=graph_name,
    )
    statement = compiled["statement"]

    return BulkIngestionPlan(
        statement=statement,
        total_records=total_records,
        batch_size=batch_size,
        dialect=dialect,
        batches_data=batches,
        batch_statements=[statement] * len(batches),
    )


def create_bulk_merge_plan(
    model: type[Node],
    key_field: str,
    data: list[dict[str, Any]] | pl.DataFrame | Any,
    batch_size: int = 50_000,
    dialect: str = "cypher",
    graph_name: str | None = None,
    defaulted_fields: Sequence[set[str]] | None = None,
) -> BulkIngestionPlan:
    """Creates an idempotent bulk upsert (MERGE) execution plan.

    Only properties a record actually supplies are written to ``ON CREATE SET``
    and ``ON MATCH SET``, so merging a partial row preserves properties the row
    omits. A property present in a record with the value ``None`` is supplied
    explicitly and therefore clears the stored property.

    Fields that were filled from a declared default are create-only: they are
    assigned in ``ON CREATE SET`` and never in ``ON MATCH SET``, so an existing
    node keeps its stored value. A default-filled field is still a supplied
    property for the row that defaulted it.

    Records are grouped by both their supplied property names (excluding
    ``key_field``) and their default-filled property names, so one record
    defaulting a field cannot change how another record with a different
    property set is compiled. Groups keep first-appearance order, rows keep
    their input order inside a group, and each group is chunked independently
    by ``batch_size``.

    DataFrame inputs (Polars, PyArrow, or pandas) cannot express per-row
    absence: every column present in the frame counts as supplied for every row,
    so a null cell clears that property. Columns absent from the frame are not
    written.

    Args:
        model: The Voyager OGM Node class.
        key_field: The unique identifier field name to match on.
        data: Records or DataFrame to upsert.
        batch_size: Number of entities per batch transaction within each group.
        dialect: Target graph query dialect ('cypher', 'iso_gql').
        graph_name: Optional graph name for Apache AGE or SQL:2023 PGQ.
        defaulted_fields: Optional per-record sets naming values that were
            already filled from a declared default.

    Returns:
        The generated execution plan with chunked parameter batches. Each batch
        carries the statement compiled for its group's create-only fields.
    """
    labels = getattr(model, "__labels__", [model.__name__])
    label = labels[0] if labels else model.__name__
    fields = getattr(model, "_schema_fields", getattr(model, "__fields__", {}))
    merge_fields = {k: v for k, v in fields.items() if k != key_field}
    merge_field_names = frozenset(merge_fields)

    raw_batches = list(chunk_dataframe(data, batch_size=batch_size))
    prepared = [
        (record, create_only & merge_field_names)
        for chunk in _prepare_batch_records(raw_batches, merge_fields, defaulted_fields)
        for record, create_only in chunk
    ]

    if merge_field_names:
        known_names = merge_field_names
    else:
        known_names = frozenset(
            name for record, _ in prepared for name in record if name != key_field
        )

    grouped: dict[tuple[frozenset[str], frozenset[str]], list[dict[str, Any]]] = {}
    for record, create_only in prepared:
        supplied = frozenset(name for name in record if name != key_field and name in known_names)
        defaulted = frozenset(create_only & supplied)
        grouped.setdefault((supplied, defaulted), []).append(record)

    batches: list[list[dict[str, Any]]] = []
    batch_statements: list[str] = []
    for (supplied, defaulted), records in grouped.items():
        if merge_field_names:
            ordered = [name for name in merge_fields if name in supplied]
        else:
            ordered = [name for name in records[0] if name != key_field and name in supplied]
        properties = [name for name in ordered if name not in defaulted]
        create_only_properties = [name for name in ordered if name in defaulted]

        compiled = _rs_compile_bulk_merge(
            label=label,
            key_property=key_field,
            properties=properties,
            create_only_properties=create_only_properties,
            dialect=dialect,
            graph_name=graph_name,
        )
        for chunk in chunk_records(records, batch_size=batch_size):
            batches.append(chunk)
            batch_statements.append(compiled["statement"])

    total_records = sum(len(b) for b in batches)

    if batch_statements:
        first_statement = batch_statements[0]
    else:
        compiled = _rs_compile_bulk_merge(
            label=label,
            key_property=key_field,
            properties=list(merge_fields),
            create_only_properties=[],
            dialect=dialect,
            graph_name=graph_name,
        )
        first_statement = compiled["statement"]

    return BulkIngestionPlan(
        statement=first_statement,
        total_records=total_records,
        batch_size=batch_size,
        dialect=dialect,
        batches_data=batches,
        batch_statements=batch_statements,
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
    properties: list[str] | None = None,
    graph_name: str | None = None,
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
        properties: Explicit list of edge properties to set, or None to infer.
        graph_name: Optional graph name for Apache AGE or SQL:2023 PGQ.

    Returns:
        The generated execution plan.
    """
    if isinstance(rel_model, str):
        rel_type = rel_model
    else:
        rel_type = getattr(
            rel_model,
            "__type__",
            getattr(rel_model, "__rel_type__", rel_model.__name__.upper()),
        )

    expected_from = f"from_{from_key}"
    expected_to = f"to_{to_key}"

    rel_fields = (
        getattr(rel_model, "_schema_fields", getattr(rel_model, "__fields__", {}))
        if not isinstance(rel_model, str)
        else {}
    )

    raw_batches = list(chunk_dataframe(data, batch_size=batch_size))
    batches = _prepare_batches(raw_batches, rel_fields)
    total_records = sum(len(b) for b in batches)

    if batches and batches[0]:
        first_record = batches[0][0]
        missing = [k for k in (expected_from, expected_to) if k not in first_record]
        if missing:
            raise ValueError(
                f"Bulk relationship data records are missing required endpoint key(s): {missing}. "
                f"Expected record keys to include '{expected_from}' and '{expected_to}' based on "
                f"from_key='{from_key}' and to_key='{to_key}'."
            )

    if properties is not None:
        props = list(properties)
    elif isinstance(rel_model, str):
        if batches and batches[0]:
            skip_keys = {expected_from, expected_to}
            props = [k for k in batches[0][0].keys() if k not in skip_keys]
        else:
            props = []
    else:
        fields = rel_fields
        props = list(fields.keys())
        if not props and batches and batches[0]:
            skip_keys = {expected_from, expected_to}
            props = [k for k in batches[0][0].keys() if k not in skip_keys]

    compiled = _rs_compile_bulk_create_rel(
        rel_type=rel_type,
        from_label=from_label,
        from_key=from_key,
        to_label=to_label,
        to_key=to_key,
        properties=props,
        batch_param="batch",
        row_alias="row",
        dialect=dialect,
        graph_name=graph_name,
    )
    statement = compiled["statement"]

    return BulkIngestionPlan(
        statement=statement,
        total_records=total_records,
        batch_size=batch_size,
        dialect=dialect,
        batches_data=batches,
        batch_statements=[statement] * len(batches),
    )


def create_parquet_ingest_plan(
    model: type[Node],
    source: str | Path | Sequence[str] | Sequence[Path],
    mode: Literal["create", "upsert"] = "create",
    key_field: str = "id",
    batch_size: int = 50_000,
    dialect: str = "cypher",
    storage_options: dict[str, Any] | None = None,
    graph_name: str | None = None,
    **kwargs: Any,
) -> BulkIngestionPlan:
    """Creates a bulk ingestion plan from local or cloud Parquet file sources.

    Args:
        model: Target Node model class.
        source: Local file path, glob pattern, or cloud URI (s3://, az://, gs://).
        mode: Ingestion mode ("create" for CREATE, "upsert" for MERGE).
        key_field: Unique matching property for upsert mode (default: "id").
        batch_size: Number of records per batch transaction.
        dialect: Target graph query dialect ('cypher', 'iso_gql').
        storage_options: Multi-cloud credentials and filesystem options passed to Polars.
        graph_name: Optional graph name for Apache AGE or SQL:2023 PGQ.
        **kwargs: Extra arguments forwarded to `pl.scan_parquet`.

    Returns:
        BulkIngestionPlan containing chunked parameter batches.
    """
    scan_source: Any = (
        [str(s) if isinstance(s, Path) else s for s in source]
        if isinstance(source, (list, tuple))
        else (str(source) if isinstance(source, Path) else source)
    )

    scan_kwargs = dict(kwargs)
    if storage_options is not None:
        scan_kwargs["storage_options"] = storage_options

    lazy_df = pl.scan_parquet(scan_source, **scan_kwargs)
    if mode == "create":
        return create_bulk_create_plan(
            model, lazy_df, batch_size=batch_size, dialect=dialect, graph_name=graph_name
        )
    elif mode == "upsert":
        return create_bulk_merge_plan(
            model,
            key_field=key_field,
            data=lazy_df,
            batch_size=batch_size,
            dialect=dialect,
            graph_name=graph_name,
        )
    else:
        raise ValueError(
            f"Unsupported ingestion mode: '{mode}'. Supported modes are 'create' and 'upsert'."
        )


def create_csv_ingest_plan(
    model: type[Node],
    source: str | Path | Sequence[str] | Sequence[Path],
    mode: Literal["create", "upsert"] = "create",
    key_field: str = "id",
    batch_size: int = 50_000,
    dialect: str = "cypher",
    storage_options: dict[str, Any] | None = None,
    graph_name: str | None = None,
    **kwargs: Any,
) -> BulkIngestionPlan:
    """Creates a bulk ingestion plan from local or cloud CSV file sources.

    Note:
        CSV schemas are inferred from a row sample (default 100 rows in Polars);
        for production loads with string IDs (e.g. leading zeros), all-null
        leading columns, or specific formats, pass explicit `dtypes` or
        `infer_schema_length` (e.g. `infer_schema_length=None`) via `**kwargs`.

    Args:
        model: Target Node model class.
        source: Local file path, glob pattern, or cloud URI (s3://, az://, gs://).
        mode: Ingestion mode ("create" for CREATE, "upsert" for MERGE).
        key_field: Unique matching property for upsert mode (default: "id").
        batch_size: Number of records per batch transaction.
        dialect: Target graph query dialect ('cypher', 'iso_gql').
        storage_options: Multi-cloud credentials and filesystem options passed to Polars.
        graph_name: Optional graph name for Apache AGE or SQL:2023 PGQ.
        **kwargs: Extra arguments forwarded to `pl.scan_csv`.

    Returns:
        BulkIngestionPlan containing chunked parameter batches.
    """
    scan_source: Any = (
        [str(s) if isinstance(s, Path) else s for s in source]
        if isinstance(source, (list, tuple))
        else (str(source) if isinstance(source, Path) else source)
    )

    scan_kwargs = dict(kwargs)
    if storage_options is not None:
        scan_kwargs["storage_options"] = storage_options

    lazy_df = pl.scan_csv(scan_source, **scan_kwargs)
    if mode == "create":
        return create_bulk_create_plan(
            model, lazy_df, batch_size=batch_size, dialect=dialect, graph_name=graph_name
        )
    elif mode == "upsert":
        return create_bulk_merge_plan(
            model,
            key_field=key_field,
            data=lazy_df,
            batch_size=batch_size,
            dialect=dialect,
            graph_name=graph_name,
        )
    else:
        raise ValueError(
            f"Unsupported ingestion mode: '{mode}'. Supported modes are 'create' and 'upsert'."
        )


def create_parquet_rel_ingest_plan(
    rel_model: type[Relationship] | str,
    source: str | Path | Sequence[str] | Sequence[Path],
    from_label: str,
    from_key: str,
    to_label: str,
    to_key: str,
    batch_size: int = 50_000,
    dialect: str = "cypher",
    properties: list[str] | None = None,
    from_source_field: str | None = None,
    to_source_field: str | None = None,
    storage_options: dict[str, Any] | None = None,
    graph_name: str | None = None,
    **kwargs: Any,
) -> BulkIngestionPlan:
    """Creates a bulk relationship ingestion plan from local or cloud Parquet file sources.

    Args:
        rel_model: Target Relationship model class or relationship type string (e.g. "KNOWS").
        source: Local file path, glob pattern, or cloud URI (s3://, az://, gs://).
        from_label: Source node label.
        from_key: Source node matching property name (e.g. "id").
        to_label: Target node label.
        to_key: Target node matching property name (e.g. "id").
        batch_size: Number of edges per batch transaction.
        dialect: Target graph query dialect ('cypher', 'iso_gql').
        properties: Explicit list of edge properties to set, or None to infer.
        from_source_field: Optional column name in file to map to `from_<from_key>`.
        to_source_field: Optional column name in file to map to `to_<to_key>`.
        storage_options: Multi-cloud credentials and filesystem options passed to Polars.
        graph_name: Optional graph name for Apache AGE or SQL:2023 PGQ.
        **kwargs: Extra arguments forwarded to `pl.scan_parquet`.

    Returns:
        BulkIngestionPlan containing chunked parameter batches.
    """
    scan_source: Any = (
        [str(s) if isinstance(s, Path) else s for s in source]
        if isinstance(source, (list, tuple))
        else (str(source) if isinstance(source, Path) else source)
    )

    scan_kwargs = dict(kwargs)
    if storage_options is not None:
        scan_kwargs["storage_options"] = storage_options

    lazy_df = pl.scan_parquet(scan_source, **scan_kwargs)

    rename_map = {}
    expected_from = f"from_{from_key}"
    expected_to = f"to_{to_key}"
    if from_source_field and from_source_field != expected_from:
        rename_map[from_source_field] = expected_from
    if to_source_field and to_source_field != expected_to:
        rename_map[to_source_field] = expected_to
    if rename_map:
        lazy_df = lazy_df.rename(rename_map)

    return create_bulk_create_rel_plan(
        rel_model=rel_model,
        data=lazy_df,
        from_label=from_label,
        from_key=from_key,
        to_label=to_label,
        to_key=to_key,
        batch_size=batch_size,
        dialect=dialect,
        properties=properties,
        graph_name=graph_name,
    )


def create_csv_rel_ingest_plan(
    rel_model: type[Relationship] | str,
    source: str | Path | Sequence[str] | Sequence[Path],
    from_label: str,
    from_key: str,
    to_label: str,
    to_key: str,
    batch_size: int = 50_000,
    dialect: str = "cypher",
    properties: list[str] | None = None,
    from_source_field: str | None = None,
    to_source_field: str | None = None,
    storage_options: dict[str, Any] | None = None,
    graph_name: str | None = None,
    **kwargs: Any,
) -> BulkIngestionPlan:
    """Creates a bulk relationship ingestion plan from local or cloud CSV file sources.

    Note:
        CSV schemas are inferred from a row sample (default 100 rows in Polars);
        for production loads with string IDs (e.g. leading zeros), all-null
        leading columns, or specific formats, pass explicit `dtypes` or
        `infer_schema_length` (e.g. `infer_schema_length=None`) via `**kwargs`.

    Args:
        rel_model: Target Relationship model class or relationship type string (e.g. "KNOWS").
        source: Local file path, glob pattern, or cloud URI (s3://, az://, gs://).
        from_label: Source node label.
        from_key: Source node matching property name (e.g. "id").
        to_label: Target node label.
        to_key: Target node matching property name (e.g. "id").
        batch_size: Number of edges per batch transaction.
        dialect: Target graph query dialect ('cypher', 'iso_gql').
        properties: Explicit list of edge properties to set, or None to infer.
        from_source_field: Optional column name in file to map to `from_<from_key>`.
        to_source_field: Optional column name in file to map to `to_<to_key>`.
        storage_options: Multi-cloud credentials and filesystem options passed to Polars.
        graph_name: Optional graph name for Apache AGE or SQL:2023 PGQ.
        **kwargs: Extra arguments forwarded to `pl.scan_csv`.

    Returns:
        BulkIngestionPlan containing chunked parameter batches.
    """
    scan_source: Any = (
        [str(s) if isinstance(s, Path) else s for s in source]
        if isinstance(source, (list, tuple))
        else (str(source) if isinstance(source, Path) else source)
    )

    scan_kwargs = dict(kwargs)
    if storage_options is not None:
        scan_kwargs["storage_options"] = storage_options

    lazy_df = pl.scan_csv(scan_source, **scan_kwargs)

    rename_map = {}
    expected_from = f"from_{from_key}"
    expected_to = f"to_{to_key}"
    if from_source_field and from_source_field != expected_from:
        rename_map[from_source_field] = expected_from
    if to_source_field and to_source_field != expected_to:
        rename_map[to_source_field] = expected_to
    if rename_map:
        lazy_df = lazy_df.rename(rename_map)

    return create_bulk_create_rel_plan(
        rel_model=rel_model,
        data=lazy_df,
        from_label=from_label,
        from_key=from_key,
        to_label=to_label,
        to_key=to_key,
        batch_size=batch_size,
        dialect=dialect,
        properties=properties,
        graph_name=graph_name,
    )


def ingest_parquet(
    session: Any,
    source: str | Path | Sequence[str] | Sequence[Path],
    model: type[Node],
    mode: Literal["create", "upsert"] = "create",
    key_field: str = "id",
    batch_size: int = 50_000,
    storage_options: dict[str, Any] | None = None,
    **kwargs: Any,
) -> Any:
    """Ingests nodes directly from local or cloud Parquet files into a session.

    Note:
        The canonical entry point is `session.ingest_parquet(...)`. This function
        is a functional convenience wrapper matching the GQLAlchemy loader idiom.
    """
    return session.ingest_parquet(
        source=source,
        model=model,
        mode=mode,
        key_field=key_field,
        batch_size=batch_size,
        storage_options=storage_options,
        **kwargs,
    )


def ingest_csv(
    session: Any,
    source: str | Path | Sequence[str] | Sequence[Path],
    model: type[Node],
    mode: Literal["create", "upsert"] = "create",
    key_field: str = "id",
    batch_size: int = 50_000,
    storage_options: dict[str, Any] | None = None,
    **kwargs: Any,
) -> Any:
    """Ingests nodes directly from local or cloud CSV files into a session.

    Note:
        The canonical entry point is `session.ingest_csv(...)`. This function
        is a functional convenience wrapper matching the GQLAlchemy loader idiom.
        CSV schemas are inferred from a row sample; for production loads pass
        explicit `dtypes` or `infer_schema_length` via `**kwargs`.
    """
    return session.ingest_csv(
        source=source,
        model=model,
        mode=mode,
        key_field=key_field,
        batch_size=batch_size,
        storage_options=storage_options,
        **kwargs,
    )


def ingest_parquet_relationships(
    session: Any,
    source: str | Path | Sequence[str] | Sequence[Path],
    rel_model: type[Relationship] | str,
    from_label: str,
    from_key: str,
    to_label: str,
    to_key: str,
    batch_size: int = 50_000,
    properties: list[str] | None = None,
    from_source_field: str | None = None,
    to_source_field: str | None = None,
    storage_options: dict[str, Any] | None = None,
    **kwargs: Any,
) -> Any:
    """Ingests relationships directly from local or cloud Parquet files into a session.

    Note:
        The canonical entry point is `session.ingest_parquet_relationships(...)`.
        This function is a functional convenience wrapper matching the GQLAlchemy loader idiom.
    """
    return session.ingest_parquet_relationships(
        source=source,
        rel_model=rel_model,
        from_label=from_label,
        from_key=from_key,
        to_label=to_label,
        to_key=to_key,
        batch_size=batch_size,
        properties=properties,
        from_source_field=from_source_field,
        to_source_field=to_source_field,
        storage_options=storage_options,
        **kwargs,
    )


def ingest_csv_relationships(
    session: Any,
    source: str | Path | Sequence[str] | Sequence[Path],
    rel_model: type[Relationship] | str,
    from_label: str,
    from_key: str,
    to_label: str,
    to_key: str,
    batch_size: int = 50_000,
    properties: list[str] | None = None,
    from_source_field: str | None = None,
    to_source_field: str | None = None,
    storage_options: dict[str, Any] | None = None,
    **kwargs: Any,
) -> Any:
    """Ingests relationships directly from local or cloud CSV files into a session.

    Note:
        The canonical entry point is `session.ingest_csv_relationships(...)`.
        This function is a functional convenience wrapper matching the GQLAlchemy loader idiom.
        CSV schemas are inferred from a row sample; for production loads pass
        explicit `dtypes` or `infer_schema_length` via `**kwargs`.
    """
    return session.ingest_csv_relationships(
        source=source,
        rel_model=rel_model,
        from_label=from_label,
        from_key=from_key,
        to_label=to_label,
        to_key=to_key,
        batch_size=batch_size,
        properties=properties,
        from_source_field=from_source_field,
        to_source_field=to_source_field,
        storage_options=storage_options,
        **kwargs,
    )
