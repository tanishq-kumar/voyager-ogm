"""Zero-copy Apache Arrow & Polars streaming ingestion for Voyager OGM.

Provides high-throughput graph result streaming via the Arrow C Data Interface
(`__arrow_c_stream__` PyCapsule protocol) to prevent Python object allocation overhead.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    import polars as pl
    import pyarrow as pa

from voyager_ogm._voyager_rs import ArrowStream


class QueryResult:
    """Zero-copy query execution result supporting Arrow and Polars ingestion.

    Wraps a C-level Arrow Array Stream and allows instantaneous ingestion
    into Arrow Tables or Polars DataFrames without copying memory.

    Attributes:
        num_rows: Total count of rows contained in the result stream.
        num_columns: Total count of columns in the result schema.

    Example:
        >>> result = QueryResult(stream)
        >>> df = result.to_polars()
    """

    def __init__(self, stream: ArrowStream) -> None:
        """Initializes a QueryResult wrapping an ArrowStream capsule.

        Args:
            stream: Underlying Rust ArrowStream capsule.
        """
        self._stream = stream
        self._cached_df: pl.DataFrame | None = None
        self._cached_table: pa.Table | None = None

    @property
    def num_rows(self) -> int:
        """Returns the total number of rows in the result stream."""
        return self._stream.num_rows

    @property
    def num_columns(self) -> int:
        """Returns the total number of columns in the result stream."""
        return self._stream.num_columns

    def to_arrow(self) -> pa.Table:
        """Zero-copy export into an Apache Arrow Table.

        Returns:
            PyArrow Table containing the full result stream.
        """
        if self._cached_table is not None:
            return self._cached_table
        if self._cached_df is not None:
            tbl = self._cached_df.to_arrow()
            self._cached_table = tbl
            return tbl
        import pyarrow as pa

        if hasattr(self._stream, "is_consumed") and self._stream.is_consumed:
            tbl = pa.Table.from_pylist(self.to_dicts())
            self._cached_table = tbl
            return tbl
        reader = pa.RecordBatchReader.from_stream(self._stream)
        tbl = reader.read_all()
        self._cached_table = tbl
        return tbl

    def to_polars(self) -> pl.DataFrame:
        """Zero-copy ingestion directly into a Polars DataFrame.

        Consumes the stream via the `__arrow_c_stream__` PyCapsule protocol.

        Returns:
            Polars DataFrame containing the hydrated graph dataset.
        """
        if self._cached_df is not None:
            return self._cached_df
        import polars as pl

        if self._cached_table is not None:
            df = pl.DataFrame(self._cached_table)
            self._cached_df = df
            return df

        if hasattr(self._stream, "is_consumed") and self._stream.is_consumed:
            df = pl.DataFrame(self.to_dicts())
            self._cached_df = df
            return df
        df = pl.DataFrame(self._stream)
        self._cached_df = df
        return df

    def to_dicts(self) -> list[dict[str, Any]]:
        """Convenience method to export records as Python dictionaries.

        Returns:
            List of dictionaries representing the row records.
        """
        if self._cached_df is not None:
            return self._cached_df.to_dicts()
        if hasattr(self._stream, "to_dicts"):
            return self._stream.to_dicts()
        return self.to_polars().to_dicts()


def to_polars(stream: ArrowStream) -> pl.DataFrame:
    """Converts an ArrowStream directly into a Polars DataFrame.

    Args:
        stream: An object implementing `__arrow_c_stream__` (e.g. ArrowStream).

    Returns:
        A Polars DataFrame referencing the columnar memory directly.

    Example:
        >>> df = to_polars(stream)
    """
    import polars as pl

    return pl.DataFrame(stream)


def to_arrow(stream: ArrowStream) -> pa.Table:
    """Converts an ArrowStream directly into an Apache Arrow Table.

    Args:
        stream: An object implementing `__arrow_c_stream__` (e.g. ArrowStream).

    Returns:
        A PyArrow Table populated from the C stream reader.

    Example:
        >>> table = to_arrow(stream)
    """
    import pyarrow as pa

    reader = pa.RecordBatchReader.from_stream(stream)
    return reader.read_all()
