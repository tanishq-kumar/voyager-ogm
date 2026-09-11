"""Session management and high-level execution facade for Voyager OGM."""

from __future__ import annotations

import weakref
from collections import defaultdict
from collections.abc import Iterator, Sequence
from typing import TYPE_CHECKING, Any

from voyager_ogm.bridge import (
    AsyncDatabaseBridge,
    AsyncMockBridge,
    BulkExecutionResult,
    DatabaseBridge,
    MockBridge,
    create_bridge,
)
from voyager_ogm.config import get_config
from voyager_ogm.ingestion import (
    BulkIngestionPlan,
    create_bulk_create_plan,
    create_bulk_create_rel_plan,
    create_bulk_merge_plan,
)
from voyager_ogm.query import CompiledQuery, Query
from voyager_ogm.transaction import Transaction

if TYPE_CHECKING:
    import polars as pl
    import pyarrow as pa

    from voyager_ogm.models import Node, Relationship


class MappingsResult(Sequence[dict[str, Any]]):
    """SQLAlchemy-compatible dictionary mapping sequence."""

    def __init__(self, records: list[dict[str, Any]]) -> None:
        self._records = records

    def all(self) -> list[dict[str, Any]]:
        """Returns all records mapped as dictionaries."""
        return list(self._records)

    def first(self) -> dict[str, Any] | None:
        """Returns the first record mapped as a dictionary, or None."""
        return self._records[0] if self._records else None

    def fetchone(self) -> dict[str, Any] | None:
        """Synonym for `.first()`."""
        return self.first()

    def fetchall(self) -> list[dict[str, Any]]:
        """Synonym for `.all()`."""
        return self.all()

    def __iter__(self) -> Iterator[dict[str, Any]]:
        return iter(self._records)

    def __len__(self) -> int:
        return len(self._records)

    def __getitem__(self, index: Any) -> Any:
        return self._records[index]


class ScalarsResult(Sequence[Any]):
    """SQLAlchemy-compatible scalar values sequence."""

    def __init__(self, records: list[dict[str, Any]]) -> None:
        self._records = records

    def all(self) -> list[Any]:
        """Returns the first column of all records as a flat scalar list."""
        if not self._records:
            return []
        first_key = next(iter(self._records[0].keys())) if self._records[0] else None
        if first_key is None:
            return []
        return [r.get(first_key) for r in self._records]

    def first(self) -> Any | None:
        """Returns the first scalar value, or None."""
        scalars = self.all()
        return scalars[0] if scalars else None

    def fetchone(self) -> Any | None:
        """Synonym for `.first()`."""
        return self.first()

    def fetchall(self) -> list[Any]:
        """Synonym for `.all()`."""
        return self.all()

    def __iter__(self) -> Iterator[Any]:
        return iter(self.all())

    def __len__(self) -> int:
        return len(self._records)

    def __getitem__(self, index: Any) -> Any:
        return self.all()[index]


class ExecutionResult(list[dict[str, Any]]):
    """SQLAlchemy-grade query execution result with graph entity and path extraction.

    Subclasses `list[dict[str, Any]]` for 100% backwards compatibility with standard
    Python list operations, while offering SQLAlchemy-like and Neo4j Browser-grade capabilities:
    - `.mappings()`: Dictionary mapping views (`result.mappings().all()`).
    - `.scalars()`: Single-column scalar extraction (`result.scalars().all()`).
    - `.all()` / `.fetchall()`: All result records.
    - `.first()` / `.fetchone()`: First record or None.
    - `.to_polars()`: Streaming Polars DataFrame export.
    - `.to_arrow()`: PyArrow Table export.
    - `.nodes`: Extracted graph nodes from the live session records.
    - `.edges`: Extracted graph edges/relationships from the live session records.
    - `.show(**kwargs)` / `.explore(**kwargs)`: Interactive GraphViewer visualizer.
    """

    def __init__(
        self,
        records: list[dict[str, Any]] | None = None,
        statement: str = "",
        query: Query | CompiledQuery | None = None,
        dialect: str = "cypher",
        stream: Any = None,
        summary: Any = None,
    ) -> None:
        self._stream = stream
        self._summary = summary
        self._statement = statement
        self._query = query
        self._dialect = dialect
        self._nodes: list[dict[str, Any]] | None = None
        self._edges: list[dict[str, Any]] | None = None
        self._cached_df: pl.DataFrame | None = None
        self._cached_table: pa.Table | None = None

        if records is not None:
            self._records: list[dict[str, Any]] | None = records
            super().__init__(records)
        else:
            self._records = None
            super().__init__()

    def _ensure_records(self) -> list[dict[str, Any]]:
        """Lazily materializes Python dictionaries from stream or cached DataFrame if not yet loaded."""
        if self._records is None:
            if self._cached_df is not None:
                self._records = self._cached_df.to_dicts()
            elif self._cached_table is not None:
                self._records = self._cached_table.to_pylist()
            elif self._stream is not None:
                if hasattr(self._stream, "is_consumed") and self._stream.is_consumed:
                    if self._cached_df is not None:
                        self._records = self._cached_df.to_dicts()
                    elif self._cached_table is not None:
                        self._records = self._cached_table.to_pylist()
                    else:
                        self._records = []
                elif hasattr(self._stream, "to_dicts"):
                    self._records = self._stream.to_dicts()
                else:
                    self._records = []
            else:
                self._records = []

            super().clear()
            super().extend(self._records)
        return self._records

    def __len__(self) -> int:
        if self._records is not None:
            return len(self._records)
        if self._cached_df is not None:
            return self._cached_df.height
        if self._cached_table is not None:
            return self._cached_table.num_rows
        if self._stream is not None and hasattr(self._stream, "num_rows"):
            return int(self._stream.num_rows)
        return super().__len__()

    def __iter__(self) -> Iterator[dict[str, Any]]:
        return iter(self._ensure_records())

    def __getitem__(self, index: Any) -> Any:
        self._ensure_records()
        return super().__getitem__(index)

    def __bool__(self) -> bool:
        return len(self) > 0

    def __repr__(self) -> str:
        self._ensure_records()
        return super().__repr__()

    def __contains__(self, item: Any) -> bool:
        self._ensure_records()
        return super().__contains__(item)

    @property
    def records(self) -> list[dict[str, Any]]:
        """Returns all result records as a list of dictionaries."""
        return self._ensure_records()

    @property
    def stream(self) -> Any:
        """The underlying ArrowStream if executed via the native Rust network engine."""
        return self._stream

    @property
    def nodes_created(self) -> int:
        """Number of graph nodes created by the query."""
        return getattr(self._summary, "nodes_created", 0)

    @property
    def nodes_deleted(self) -> int:
        """Number of graph nodes deleted by the query."""
        return getattr(self._summary, "nodes_deleted", 0)

    @property
    def relationships_created(self) -> int:
        """Number of graph relationships created by the query."""
        return getattr(self._summary, "relationships_created", 0)

    @property
    def relationships_deleted(self) -> int:
        """Number of graph relationships deleted by the query."""
        return getattr(self._summary, "relationships_deleted", 0)

    @property
    def properties_set(self) -> int:
        """Number of properties set or updated by the query."""
        return getattr(self._summary, "properties_set", 0)

    @property
    def execution_time_ms(self) -> int:
        """Total server-side query planning and execution duration in milliseconds."""
        return getattr(self._summary, "execution_time_ms", 0)

    @property
    def statement(self) -> str:
        """The executed Cypher, GQL, or SQL query statement string."""
        return self._statement

    @property
    def dialect(self) -> str:
        """The active query dialect for this result."""
        return self._dialect

    def mappings(self) -> MappingsResult:
        """Provides an SQLAlchemy-style dictionary mapping view over the result rows."""
        return MappingsResult(self._ensure_records())

    def scalars(self) -> ScalarsResult:
        """Provides an SQLAlchemy-style single-column scalar extraction view."""
        return ScalarsResult(self._ensure_records())

    def all(self) -> list[dict[str, Any]]:
        """Returns all result records as a list of dictionaries."""
        return list(self._ensure_records())

    def fetchall(self) -> list[dict[str, Any]]:
        """Synonym for `.all()`."""
        return self.all()

    def first(self) -> dict[str, Any] | None:
        """Returns the first record as a dictionary, or None if empty."""
        records = self._ensure_records()
        return records[0] if records else None

    def fetchone(self) -> dict[str, Any] | None:
        """Synonym for `.first()`."""
        return self.first()

    def to_polars(self) -> pl.DataFrame:
        """Exports the result records into a Polars DataFrame.

        When running on the native network backend, streams Arrow C data directly
        into Polars without intermediate Python dict allocation.
        """
        if self._cached_df is not None:
            return self._cached_df
        import polars as pl

        if self._cached_table is not None:
            df = pl.DataFrame(self._cached_table)
            self._cached_df = df
            return df

        if self._stream is not None and not getattr(self._stream, "is_consumed", False):
            try:
                df = pl.DataFrame(self._stream)
                self._cached_df = df
                return df
            except Exception:
                pass

        records = self._ensure_records()
        df = pl.DataFrame(records) if records else pl.DataFrame()
        self._cached_df = df
        return df

    def to_arrow(self) -> pa.Table:
        """Exports the result records into an Apache Arrow Table.

        When running on the native network backend, streams Arrow C data directly
        into PyArrow without intermediate Python dict allocation.
        """
        if self._cached_table is not None:
            return self._cached_table
        import pyarrow as pa

        if self._cached_df is not None:
            tbl = self._cached_df.to_arrow()
            self._cached_table = tbl
            return tbl

        if self._stream is not None and not getattr(self._stream, "is_consumed", False):
            try:
                reader = pa.RecordBatchReader.from_stream(self._stream)
                tbl = reader.read_all()
                self._cached_table = tbl
                return tbl
            except Exception:
                pass

        records = self._ensure_records()
        tbl = pa.Table.from_pylist(records) if records else pa.Table.from_pylist([])
        self._cached_table = tbl
        return tbl

    def iter_batches(
        self,
        batch_size: int = 10_000,
        as_format: str = "dicts",
    ) -> Iterator[Any]:
        """Streams the execution result in constant-memory chunks of `batch_size` rows.

        Prevents CPython memory explosions and GC stop-the-world pauses when
        processing high-cardinality result sets (e.g. 100k - 10M rows).

        Parameters
        ----------
        batch_size : int, default 10,000
            Maximum number of rows per yielded batch chunk.
        as_format : str, default "dicts"
            Format of each yielded batch:
            - `"dicts"`: yields a `list[dict[str, Any]]` chunk of at most `batch_size` rows.
            - `"polars"`: yields a `polars.DataFrame` chunk of at most `batch_size` rows.
            - `"arrow"`: yields a `pyarrow.RecordBatch` or `pyarrow.Table` chunk.
            - `"stream"`: yields an `ArrowStream` slice capsule.

        Yields:
        ------
        Iterator[Any]
            Successive chunks of the result set.
        """
        chunk_size = max(1, batch_size)

        # 1. Native Rust ArrowStream backend
        if (
            self._stream is not None
            and hasattr(self._stream, "iter_batches")
            and not getattr(self._stream, "is_consumed", False)
        ):
            stream_chunks = self._stream.iter_batches(chunk_size)
            for chunk in stream_chunks:
                if as_format == "dicts":
                    yield chunk.to_dicts()
                elif as_format == "polars":
                    import polars as pl

                    yield pl.DataFrame(chunk)
                elif as_format == "arrow":
                    import pyarrow as pa

                    reader = pa.RecordBatchReader.from_stream(chunk)
                    yield reader.read_next_batch()
                elif as_format == "stream":
                    yield chunk
                else:
                    raise ValueError(
                        f"Unsupported batch format: '{as_format}'. Supported formats: 'dicts', 'polars', 'arrow', 'stream'"
                    )
            return

        # 2. Cached Polars DataFrame backend
        if self._cached_df is not None:
            total_rows = self._cached_df.height
            offset = 0
            while offset < total_rows:
                length = min(chunk_size, total_rows - offset)
                sliced_df = self._cached_df.slice(offset, length)
                offset += length
                if as_format == "polars":
                    yield sliced_df
                elif as_format == "dicts":
                    yield sliced_df.to_dicts()
                elif as_format == "arrow":
                    yield sliced_df.to_arrow()
                elif as_format == "stream":
                    yield sliced_df.to_arrow()
                else:
                    raise ValueError(f"Unsupported batch format: '{as_format}'")
            return

        # 3. Cached PyArrow Table backend
        if self._cached_table is not None:
            total_rows = self._cached_table.num_rows
            offset = 0
            while offset < total_rows:
                length = min(chunk_size, total_rows - offset)
                sliced_table = self._cached_table.slice(offset, length)
                offset += length
                if as_format == "arrow":
                    yield sliced_table
                elif as_format == "dicts":
                    yield sliced_table.to_pylist()
                elif as_format == "polars":
                    import polars as pl

                    yield pl.DataFrame(sliced_table)
                elif as_format == "stream":
                    yield sliced_table
                else:
                    raise ValueError(f"Unsupported batch format: '{as_format}'")
            return

        # 4. Fallback: Python records list
        records = self._ensure_records()
        total_rows = len(records)
        offset = 0
        while offset < total_rows:
            chunk = records[offset : offset + chunk_size]
            offset += chunk_size
            if as_format == "dicts":
                yield chunk
            elif as_format == "polars":
                import polars as pl

                yield pl.DataFrame(chunk) if chunk else pl.DataFrame()
            elif as_format == "arrow":
                import pyarrow as pa

                yield pa.Table.from_pylist(chunk) if chunk else pa.Table.from_pylist([])
            elif as_format == "stream":
                import pyarrow as pa

                yield pa.Table.from_pylist(chunk) if chunk else pa.Table.from_pylist([])
            else:
                raise ValueError(f"Unsupported batch format: '{as_format}'")

    def to_graph(self) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
        """Extracts live graph nodes, relationships, and reconstructed paths from result records."""
        if self._nodes is not None and self._edges is not None:
            return self._nodes, self._edges

        from voyager_ogm.viewer import extract_graph_entities_from_records

        self._nodes, self._edges = extract_graph_entities_from_records(
            records=self._ensure_records(),
            statement=self._statement,
            query=self._query,
        )
        return self._nodes, self._edges

    @property
    def nodes(self) -> list[dict[str, Any]]:
        """List of graph nodes extracted from the executed result records."""
        nodes, _ = self.to_graph()
        return nodes

    @property
    def edges(self) -> list[dict[str, Any]]:
        """List of graph edges/relationships extracted from the executed result records."""
        _, edges = self.to_graph()
        return edges

    def show(self, **kwargs: Any) -> Any:
        """Visualizes the live executed query results in an interactive GraphViewer widget."""
        from voyager_ogm.viewer import GraphViewer

        nodes, edges = self.to_graph()
        return GraphViewer(
            nodes=nodes,
            edges=edges,
            records=self._ensure_records(),
            query_statement=self._statement,
            default_view="graph" if edges or nodes else "table",
            **kwargs,
        )

    def explore(self, **kwargs: Any) -> Any:
        """Synonym for `.show()`."""
        return self.show(**kwargs)


# Alias for backward compatibility
Result = ExecutionResult


def _is_query_semantic_error(exc: Exception) -> bool:
    """Checks if the exception represents a query syntax, semantic, or constraint error.

    In async web servers, semantic query errors must NEVER trigger permanent backend degradation,
    as they are query-specific defects that would fail identically across all drivers.
    """
    msg = str(exc)
    semantic_indicators = (
        "SyntaxError",
        "syntax error",
        "SemanticError",
        "ConstraintValidationFailed",
        "ParameterMissing",
        "TypeError",
        "EntityNotFound",
        "Unknown function",
        "Invalid input",
        "already exists",
        "does not exist",
        "violates unique constraint",
        "violates not-null constraint",
        "violates foreign key constraint",
    )
    return any(indicator in msg for indicator in semantic_indicators)


class Session:
    """Synchronous graph database session coordinator for Voyager OGM.

    Provides high-throughput bulk ingestion, Unit-of-Work transactions, and query dispatch
    over pluggable database drivers (Neo4j, Memgraph, DuckDB, Mock, etc.).
    """

    def __init__(
        self,
        bridge: Any = None,
        dialect: str | None = None,
        enable_identity_map: bool | None = None,
        optimize: bool | str | None = None,
        optimization_level: str | None = None,
        backend: str = "auto",
        pool_size: int = 10,
    ) -> None:
        """Initializes a new Voyager Session.

        Args:
            bridge: Pluggable database driver (Neo4j Driver, DuckDB connection, MockBridge, or URI string).
                If omitted or None, defaults to MockBridge.
            dialect: Default query dialect for statements generated by this session
                ('cypher', 'iso_gql', 'sql_pgq'). If None, defaults to global config setting.
            enable_identity_map: [Experimental] Opt-in flag to enable the hybrid Active Record & Data Mapper
                Identity Map. When True, entities registered in this session are tracked via
                weak references, enabling coalesced single-batch UNWIND flushes (eliminating N+1
                network queries) and active record `.save()` ergonomics without memory leaks.
            optimize: Enable/disable AST query optimization, or specify level ('standard', 'aggressive', 'none', True, False).
                If None, defaults to global config setting.
            optimization_level: Explicit optimization level ('standard', 'aggressive', 'none').
            backend: Query execution engine ('native', 'bridge', or 'auto'). When 'native' or 'auto',
                uses the native Rust async network engine (`voyager-net`) over raw sockets with connection
                pooling and direct wire-to-Arrow streaming, falling back to 'bridge' if unavailable.
            pool_size: Connection pool capacity for native network engine connections (default 10).
        """
        cfg = get_config()
        self._dialect = dialect or cfg.default_dialect
        self._enable_identity_map = (
            cfg.enable_identity_map if enable_identity_map is None else enable_identity_map
        )

        if isinstance(optimize, str):
            opt_str = optimize.strip().lower()
            if opt_str in ("none", "off", "false", "0"):
                self._optimize = False
                self._optimization_level = "none"
            else:
                self._optimize = True
                self._optimization_level = opt_str
        elif isinstance(optimize, bool):
            self._optimize = optimize
            self._optimization_level = optimization_level or cfg.optimization_level
        else:
            self._optimize = cfg.optimize
            self._optimization_level = optimization_level or cfg.optimization_level

        self._bridge: DatabaseBridge = create_bridge(bridge, is_async=False)  # type: ignore[assignment]
        self._identity_map: weakref.WeakValueDictionary[tuple[type[Any], Any], Any] = (
            weakref.WeakValueDictionary()
        )

        self._requested_backend = backend.lower() if isinstance(backend, str) else "auto"
        self._native_client: Any = None
        self._active_backend = "bridge"

        if self._requested_backend in ("native", "auto"):
            if hasattr(bridge, "execute_sync"):
                self._native_client = bridge
                self._active_backend = "native"
            elif isinstance(bridge, str) and not bridge.startswith("mock://"):
                try:
                    from voyager_ogm._voyager_rs import NativeClient

                    self._native_client = NativeClient(bridge, min_idle=1, max_size=pool_size)
                    self._active_backend = "native"
                except Exception as e:
                    if self._requested_backend == "native":
                        raise RuntimeError(f"Failed to initialize native backend: {e}") from e
                    self._active_backend = "bridge"
            elif self._requested_backend == "native":
                raise ValueError(
                    f"Native backend requires a valid URI string, got: {type(bridge).__name__}"
                )

        self._is_explicit_mock = (
            bridge is None
            or (isinstance(bridge, str) and bridge.startswith("mock://"))
            or isinstance(bridge, (MockBridge, AsyncMockBridge))
        )

    def reset_backend(self) -> None:
        """Resets the active backend back to native if NativeClient is available.

        Allows web servers and health probes to restore high-throughput native
        execution after transient network connectivity issues resolve.
        """
        if self._native_client is not None and self._requested_backend in ("native", "auto"):
            self._active_backend = "native"

    @property
    def backend(self) -> str:
        """The active query execution backend ('native' or 'bridge')."""
        return self._active_backend

    @property
    def native_client(self) -> Any:
        """Underlying native Rust network client if using the native backend, else None."""
        return self._native_client

    @property
    def identity_map_enabled(self) -> bool:
        """[Experimental] Whether the Identity Map is enabled for this session."""
        return self._enable_identity_map

    @property
    def optimize_enabled(self) -> bool:
        """Whether AST query optimization is enabled for this session."""
        return self._optimize

    @property
    def optimization_level(self) -> str:
        """The active AST optimization level ('none', 'standard', 'aggressive')."""
        return self._optimization_level

    def register(self, node: Any, key_field: str = "id", is_clean: bool = False) -> Any:
        """[Experimental] Registers a node instance with this session's Identity Map.

        Technique: Hybrid Data Mapper + Active Record Unit-of-Work
        ----------------------------------------------------------
        - Combines the ergonomics of Active Record (tracking dirty fields directly on instances)
          with the clean architecture of the Data Mapper pattern (entities remain pure Python,
          while the Session coordinates batch transpilation and database persistence).
        - Uses weak references (`WeakValueDictionary`) to avoid memory leaks: entities are
          automatically garbage-collected when dropped from user scope.
        - Deduplicates entities: querying or registering an entity with the same (Model, primary_key)
          returns the existing tracked in-memory instance, preventing split-brain mutations.

        Args:
            node: The Node instance to register and track.
            key_field: Unique primary key property name (defaults to 'id').
            is_clean: If True, marks the node as clean (clearing dirty_fields), typically
                used when loading persisted entities from the database into the Identity Map.

        Returns:
            The registered node instance (or existing tracked instance if already registered).
        """
        if not self._enable_identity_map:
            return node

        if is_clean and hasattr(node, "clear_dirty"):
            node.clear_dirty()

        key_val = node.get(key_field) if hasattr(node, "get") else getattr(node, key_field, None)
        if key_val is None:
            return node

        map_key = (type(node), key_val)
        existing = self._identity_map.get(map_key)
        if existing is not None and existing is not node:
            if hasattr(node, "dirty_fields"):
                for k, v in node.dirty_fields.items():
                    setattr(existing, k, v)
            if is_clean and hasattr(existing, "clear_dirty"):
                existing.clear_dirty()
            if hasattr(existing, "_attach_session"):
                existing._attach_session(self)
            return existing

        self._identity_map[map_key] = node
        if hasattr(node, "_attach_session"):
            node._attach_session(self)
        return node

    def get_node(self, model: type[Any], key: Any) -> Any | None:
        """[Experimental] Retrieves a tracked node instance from the Identity Map if present.

        Args:
            model: The Node model class.
            key: Primary key identifier value.

        Returns:
            Tracked Node instance if present in identity map, else None.
        """
        if not self._enable_identity_map:
            return None
        return self._identity_map.get((model, key))

    def flush(self, key_field: str = "id") -> list[BulkExecutionResult]:
        """[Experimental] Flushes all dirty entities tracked in the Identity Map in a single batch per model.

        Technique: Vectorized Batch Coalescing (Zero N+1 Network Penalty)
        ----------------------------------------------------------------
        Rather than executing N individual UPDATE statements across the network (the standard
        Active Record anti-pattern), the Session inspects all tracked entities with non-empty
        `dirty_fields`, groups them by Model class, and compiles ONE single vectorized `UNWIND $batch`
        merge statement per entity type.

        This delivers up to 100x higher throughput while preserving simple active record
        mutation syntax (`node.prop = val`).

        Args:
            key_field: Primary key property name (defaults to 'id').

        Returns:
            List of BulkExecutionResult metrics for executed batch upsert plans.
        """
        if not self._enable_identity_map or not self._identity_map:
            return []

        dirty_by_model: dict[type[Any], list[Any]] = defaultdict(list)
        for (_, _), node in list(self._identity_map.items()):
            if hasattr(node, "dirty_fields") and node.dirty_fields:
                dirty_by_model[type(node)].append(node)

        results: list[BulkExecutionResult] = []
        for model_cls, nodes in dirty_by_model.items():
            batch_data: list[dict[str, Any]] = []
            for n in nodes:
                record = dict(n.dirty_fields)
                primary_val = n.get(key_field) if hasattr(n, "get") else getattr(n, key_field, None)
                if primary_val is not None:
                    record[key_field] = primary_val
                batch_data.append(record)

            if batch_data:
                plan = self.bulk_upsert(
                    model=model_cls,
                    data=batch_data,
                    key_field=key_field,
                )
                res = self.run_bulk(plan)
                results.append(res)
                for n in nodes:
                    if hasattr(n, "clear_dirty"):
                        n.clear_dirty()

        return results

    def clear(self) -> None:
        """[Experimental] Clears all tracked entities from the Identity Map."""
        self._identity_map.clear()

    @property
    def dialect(self) -> str:
        """Active dialect for this session."""
        return self._dialect

    @property
    def bridge(self) -> DatabaseBridge:
        """Active database bridge."""
        return self._bridge

    def _prepare_statement(
        self,
        query_or_statement: Query | CompiledQuery | str,
        parameters: dict[str, Any] | None = None,
    ) -> tuple[str, dict[str, Any], Query | CompiledQuery | None]:
        stmt = ""
        params = parameters or {}
        q_obj: Query | CompiledQuery | None = None

        if isinstance(query_or_statement, CompiledQuery):
            stmt = query_or_statement.statement
            params = query_or_statement.parameters
            q_obj = query_or_statement
        elif isinstance(query_or_statement, Query):
            opt = (
                query_or_statement._optimize
                if query_or_statement._optimize is not None
                else self._optimize
            )
            lvl = (
                query_or_statement._optimization_level
                if query_or_statement._optimization_level is not None
                else self._optimization_level
            )
            compiled = query_or_statement.compile(
                dialect=self._dialect,
                optimize=opt,
                optimization_level=lvl,
            )
            stmt = compiled.statement
            params = compiled.parameters
            q_obj = query_or_statement
        else:
            stmt = str(query_or_statement)

        return stmt, params, q_obj

    def execute(
        self,
        query_or_statement: Query | CompiledQuery | str,
        parameters: dict[str, Any] | None = None,
    ) -> ExecutionResult:
        """Executes a Query object, CompiledQuery, or raw statement through the bridge.

        Args:
            query_or_statement: Voyager Query, CompiledQuery, or raw query statement string.
            parameters: Query parameters dictionary (if statement is a string).

        Returns:
            ExecutionResult containing rows, SQLAlchemy mappings/scalars access, and graph entity extraction.
        """
        stmt, params, q_obj = self._prepare_statement(query_or_statement, parameters)

        if self._active_backend == "native" and self._native_client is not None:
            try:
                native_res = self._native_client.execute_sync(stmt, params)
                return ExecutionResult(
                    records=None,
                    statement=stmt,
                    query=q_obj,
                    dialect=self._dialect,
                    stream=native_res.stream,
                    summary=native_res.summary,
                )
            except Exception as e:
                if self._requested_backend == "native" or _is_query_semantic_error(e):
                    raise
                if (
                    isinstance(self._bridge, (MockBridge, AsyncMockBridge))
                    and not self._is_explicit_mock
                ):
                    raise RuntimeError(
                        f"Native backend execution failed: {e}. No official database driver available for fallback."
                    ) from e
                self._active_backend = "bridge"

        raw_records = self._bridge.execute(stmt, params)
        return ExecutionResult(
            records=raw_records,
            statement=stmt,
            query=q_obj,
            dialect=self._dialect,
        )

    def execute_to_polars(
        self,
        query_or_statement: Query | CompiledQuery | str,
        parameters: dict[str, Any] | None = None,
    ) -> pl.DataFrame:
        """Executes a query and streams results directly into a Polars DataFrame.

        When running on the native network backend, streams Arrow C data directly
        into Polars without intermediate Python dict allocation or ExecutionResult overhead.

        Args:
            query_or_statement: Voyager Query, CompiledQuery, or raw query statement string.
            parameters: Query parameters dictionary (if statement is a string).

        Returns:
            Columnar Polars DataFrame containing the result records.
        """
        stmt, params, _ = self._prepare_statement(query_or_statement, parameters)

        if self._active_backend == "native" and self._native_client is not None:
            try:
                native_res = self._native_client.execute_sync(stmt, params)
                import polars as pl

                return pl.DataFrame(native_res.stream)
            except Exception as e:
                if self._requested_backend == "native" or _is_query_semantic_error(e):
                    raise
                if (
                    isinstance(self._bridge, (MockBridge, AsyncMockBridge))
                    and not self._is_explicit_mock
                ):
                    raise RuntimeError(
                        f"Native backend execution failed: {e}. No official database driver available for fallback."
                    ) from e
                self._active_backend = "bridge"

        return self._bridge.execute_to_polars(stmt, params)

    def execute_to_arrow(
        self,
        query_or_statement: Query | CompiledQuery | str,
        parameters: dict[str, Any] | None = None,
    ) -> pa.Table:
        """Executes a query and exports results directly into an Apache Arrow Table.

        When running on the native network backend, streams Arrow C data directly
        into PyArrow without intermediate Python dict allocation.

        Args:
            query_or_statement: Voyager Query, CompiledQuery, or raw query statement string.
            parameters: Query parameters dictionary (if statement is a string).

        Returns:
            PyArrow Table containing the result records.
        """
        stmt, params, _ = self._prepare_statement(query_or_statement, parameters)

        if self._active_backend == "native" and self._native_client is not None:
            try:
                native_res = self._native_client.execute_sync(stmt, params)
                import pyarrow as pa

                reader = pa.RecordBatchReader.from_stream(native_res.stream)
                return reader.read_all()
            except Exception as e:
                if self._requested_backend == "native" or _is_query_semantic_error(e):
                    raise
                if (
                    isinstance(self._bridge, (MockBridge, AsyncMockBridge))
                    and not self._is_explicit_mock
                ):
                    raise RuntimeError(
                        f"Native backend execution failed: {e}. No official database driver available for fallback."
                    ) from e
                self._active_backend = "bridge"

        res = self.execute(query_or_statement, parameters)
        return res.to_arrow()

    def ping(self) -> bool:
        """Pings the database connection to verify liveness and network connectivity."""
        if self._active_backend == "native" and self._native_client is not None:
            return bool(self._native_client.ping_sync())
        ping_fn = getattr(self._bridge, "ping", None)
        if callable(ping_fn):
            return bool(ping_fn())
        try:
            self.execute("RETURN 1")
            return True
        except Exception:
            return False

    def run_bulk(self, plan: BulkIngestionPlan) -> BulkExecutionResult:
        """Dispatches and executes a BulkIngestionPlan across the database bridge.

        Args:
            plan: Prepared bulk ingestion plan generated by bulk_create/bulk_upsert.

        Returns:
            Execution metrics summary including total batches, records, and elapsed time.
        """
        return self._bridge.execute_bulk(plan)

    def explore(self, target: Any = None, **kwargs: Any) -> Any:
        """Opens an interactive graph and records visualizer for a Query, DataFrame, or database view.

        Args:
            target: Query instance, Polars DataFrame, or raw query string.
            **kwargs: Styling and layout arguments passed to GraphViewer.

        Returns:
            Interactive GraphViewer component for Marimo, Jupyter, and VS Code.
        """
        from voyager_ogm.query import Query
        from voyager_ogm.viewer import GraphViewer

        if target is None:
            target = "MATCH (n)-[r]->(m) RETURN n, r, m LIMIT 100"

        if isinstance(target, Query):
            return GraphViewer.from_query(target, session=self, **kwargs)
        if isinstance(target, str):
            res = self.execute(target)
            return res.show(**kwargs)
        if hasattr(target, "to_dicts"):
            return GraphViewer.from_polars(target, **kwargs)
        return GraphViewer(nodes=[], edges=[], records=[], **kwargs)

    def transaction(self) -> Transaction:
        """Creates a fresh two-layer rollback transaction context manager.

        Returns:
            Active transaction instance with savepoint support.
        """
        return Transaction()

    def bulk_create(
        self,
        model: type[Node],
        data: list[dict[str, Any]] | pl.DataFrame | Any,
        batch_size: int = 50_000,
        dialect: str | None = None,
    ) -> BulkIngestionPlan:
        """Prepares a high-throughput bulk node creation execution plan.

        Compiles `UNWIND $batch AS row CREATE (n:Model) SET n.prop = row.prop` and chunks
        the input data/DataFrame into optimal batches.

        Args:
            model: Node class to instantiate.
            data: Records or Polars DataFrame to insert.
            batch_size: Maximum records per batch transaction.
            dialect: Override the session default dialect.

        Returns:
            Executable plan containing parameterized batch queries.
        """
        target_dialect = dialect or self._dialect
        return create_bulk_create_plan(
            model=model,
            data=data,
            batch_size=batch_size,
            dialect=target_dialect,
        )

    def bulk_upsert(
        self,
        model: type[Node],
        data: list[dict[str, Any]] | pl.DataFrame | Any,
        key_field: str,
        batch_size: int = 50_000,
        dialect: str | None = None,
    ) -> BulkIngestionPlan:
        """Prepares a high-throughput idempotent bulk upsert (MERGE) execution plan.

        Compiles `UNWIND $batch AS row MERGE (n:Model {key: row.key})` with
        `ON CREATE SET ... ON MATCH SET ...` and chunks the input data/DataFrame.

        Args:
            model: Node class to upsert.
            data: Records or Polars DataFrame to upsert.
            key_field: Unique key property field name.
            batch_size: Maximum records per batch transaction.
            dialect: Override the session default dialect.

        Returns:
            Executable plan containing parameterized batch queries.
        """
        target_dialect = dialect or self._dialect
        return create_bulk_merge_plan(
            model=model,
            key_field=key_field,
            data=data,
            batch_size=batch_size,
            dialect=target_dialect,
        )

    def bulk_create_relationships(
        self,
        rel_model: type[Relationship] | str,
        data: list[dict[str, Any]] | pl.DataFrame | Any,
        from_label: str,
        from_key: str,
        to_label: str,
        to_key: str,
        batch_size: int = 50_000,
        dialect: str | None = None,
    ) -> BulkIngestionPlan:
        """Prepares a high-throughput bulk relationship creation execution plan.

        Args:
            rel_model: Relationship class or type string.
            data: Records containing `from_<from_key>`, `to_<to_key>`, and edge properties.
            from_label: Source node label.
            from_key: Source node matching property name.
            to_label: Target node label.
            to_key: Target node matching property name.
            batch_size: Maximum edges per batch transaction.
            dialect: Override the session default dialect.

        Returns:
            Executable plan.
        """
        target_dialect = dialect or self._dialect
        return create_bulk_create_rel_plan(
            rel_model=rel_model,
            data=data,
            from_label=from_label,
            from_key=from_key,
            to_label=to_label,
            to_key=to_key,
            batch_size=batch_size,
            dialect=target_dialect,
        )

    def close(self) -> None:
        """Closes the underlying database bridge and native connection pool, clearing the identity map."""
        self.clear()
        if self._native_client is not None:
            self._native_client.close()
        self._bridge.close()

    def __enter__(self) -> Session:
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        self.close()


class AsyncSession:
    """Asynchronous graph database session coordinator for Voyager OGM.

    Provides non-blocking async/await query execution and bulk ingestion over
    async database drivers (e.g. neo4j.AsyncDriver) or the native Rust network engine.
    """

    def __init__(
        self,
        bridge: Any = None,
        dialect: str | None = None,
        enable_identity_map: bool | None = None,
        optimize: bool | str | None = None,
        optimization_level: str | None = None,
        backend: str = "auto",
        pool_size: int = 10,
    ) -> None:
        """Initializes a new asynchronous Voyager Session.

        Args:
            bridge: Pluggable async database driver (neo4j.AsyncDriver, AsyncMockBridge, or URI string).
            dialect: Default query dialect for statements generated by this session.
                If None, defaults to global config setting.
            enable_identity_map: [Experimental] Opt-in flag to enable the hybrid Active Record & Data Mapper
                Identity Map with weak references and vectorized batch flushes.
            optimize: Enable/disable AST query optimization, or specify level ('standard', 'aggressive', 'none', True, False).
                If None, defaults to global config setting.
            optimization_level: Explicit optimization level ('standard', 'aggressive', 'none').
            backend: Query execution engine ('native', 'bridge', or 'auto'). When 'native' or 'auto',
                uses the native Rust async network engine (`voyager-net`) over raw sockets with connection
                pooling and direct wire-to-Arrow streaming, falling back to 'bridge' if unavailable.
            pool_size: Connection pool capacity for native network engine connections (default 10).
        """
        cfg = get_config()
        self._dialect = dialect or cfg.default_dialect
        self._enable_identity_map = (
            cfg.enable_identity_map if enable_identity_map is None else enable_identity_map
        )

        if isinstance(optimize, str):
            opt_str = optimize.strip().lower()
            if opt_str in ("none", "off", "false", "0"):
                self._optimize = False
                self._optimization_level = "none"
            else:
                self._optimize = True
                self._optimization_level = opt_str
        elif isinstance(optimize, bool):
            self._optimize = optimize
            self._optimization_level = optimization_level or cfg.optimization_level
        else:
            self._optimize = cfg.optimize
            self._optimization_level = optimization_level or cfg.optimization_level

        self._bridge: AsyncDatabaseBridge = create_bridge(bridge, is_async=True)  # type: ignore[assignment]
        self._identity_map: weakref.WeakValueDictionary[tuple[type[Any], Any], Any] = (
            weakref.WeakValueDictionary()
        )

        self._requested_backend = backend.lower() if isinstance(backend, str) else "auto"
        self._native_client: Any = None
        self._active_backend = "bridge"

        if self._requested_backend in ("native", "auto"):
            if hasattr(bridge, "execute") and hasattr(bridge, "execute_sync"):
                self._native_client = bridge
                self._active_backend = "native"
            elif isinstance(bridge, str) and not bridge.startswith("mock://"):
                try:
                    from voyager_ogm._voyager_rs import NativeClient

                    self._native_client = NativeClient(bridge, min_idle=1, max_size=pool_size)
                    self._active_backend = "native"
                except Exception as e:
                    if self._requested_backend == "native":
                        raise RuntimeError(f"Failed to initialize native backend: {e}") from e
                    self._active_backend = "bridge"
            elif self._requested_backend == "native":
                raise ValueError(
                    f"Native backend requires a valid URI string, got: {type(bridge).__name__}"
                )

        self._is_explicit_mock = (
            bridge is None
            or (isinstance(bridge, str) and bridge.startswith("mock://"))
            or isinstance(bridge, (MockBridge, AsyncMockBridge))
        )

    def reset_backend(self) -> None:
        """Resets the active backend back to native if NativeClient is available.

        Allows web servers and health probes to restore high-throughput native
        execution after transient network connectivity issues resolve.
        """
        if self._native_client is not None and self._requested_backend in ("native", "auto"):
            self._active_backend = "native"

    @property
    def backend(self) -> str:
        """The active query execution backend ('native' or 'bridge')."""
        return self._active_backend

    @property
    def native_client(self) -> Any:
        """Underlying native Rust network client if using the native backend, else None."""
        return self._native_client

    @property
    def identity_map_enabled(self) -> bool:
        """[Experimental] Whether the Identity Map is enabled for this async session."""
        return self._enable_identity_map

    @property
    def optimize_enabled(self) -> bool:
        """Whether AST query optimization is enabled for this async session."""
        return self._optimize

    @property
    def optimization_level(self) -> str:
        """The active AST optimization level ('none', 'standard', 'aggressive')."""
        return self._optimization_level

    def register(self, node: Any, key_field: str = "id", is_clean: bool = False) -> Any:
        """[Experimental] Registers a node instance with this async session's Identity Map.

        Technique: Hybrid Data Mapper + Active Record Unit-of-Work
        ----------------------------------------------------------
        - Combines Active Record ergonomics with Data Mapper performance.
        - Employs weak references (`WeakValueDictionary`) for automatic GC memory reclamation.
        - Deduplicates entities by (Model, primary_key).

        Args:
            node: The Node instance to register.
            key_field: Unique primary key property name (defaults to 'id').
            is_clean: If True, marks the node as clean (clearing dirty_fields), typically
                used when loading persisted entities from the database into the Identity Map.

        Returns:
            The registered node instance.
        """
        if not self._enable_identity_map:
            return node

        if is_clean and hasattr(node, "clear_dirty"):
            node.clear_dirty()

        key_val = node.get(key_field) if hasattr(node, "get") else getattr(node, key_field, None)
        if key_val is None:
            return node

        map_key = (type(node), key_val)
        existing = self._identity_map.get(map_key)
        if existing is not None and existing is not node:
            if hasattr(node, "dirty_fields"):
                for k, v in node.dirty_fields.items():
                    setattr(existing, k, v)
            if is_clean and hasattr(existing, "clear_dirty"):
                existing.clear_dirty()
            if hasattr(existing, "_attach_session"):
                existing._attach_session(self)
            return existing

        self._identity_map[map_key] = node
        if hasattr(node, "_attach_session"):
            node._attach_session(self)
        return node

    def get_node(self, model: type[Any], key: Any) -> Any | None:
        """[Experimental] Retrieves a tracked node instance from the Identity Map if present.

        Args:
            model: The Node model class.
            key: Primary key identifier value.

        Returns:
            Tracked Node instance if present, else None.
        """
        if not self._enable_identity_map:
            return None
        return self._identity_map.get((model, key))

    async def flush(self, key_field: str = "id") -> list[BulkExecutionResult]:
        """[Experimental] Asynchronously flushes all dirty entities tracked in the Identity Map in a single batch per model.

        Technique: Vectorized Batch Coalescing (Zero N+1 Network Penalty)
        ----------------------------------------------------------------
        Gathers all dirty entities across the Identity Map and compiles a single vectorized
        UNWIND batch upsert per entity type, avoiding individual round-trip overhead.

        Args:
            key_field: Primary key property name (defaults to 'id').

        Returns:
            List of BulkExecutionResult metrics for executed batch upsert plans.
        """
        if not self._enable_identity_map or not self._identity_map:
            return []

        dirty_by_model: dict[type[Any], list[Any]] = defaultdict(list)
        for (_, _), node in list(self._identity_map.items()):
            if hasattr(node, "dirty_fields") and node.dirty_fields:
                dirty_by_model[type(node)].append(node)

        results: list[BulkExecutionResult] = []
        for model_cls, nodes in dirty_by_model.items():
            batch_data: list[dict[str, Any]] = []
            for n in nodes:
                record = dict(n.dirty_fields)
                primary_val = n.get(key_field) if hasattr(n, "get") else getattr(n, key_field, None)
                if primary_val is not None:
                    record[key_field] = primary_val
                batch_data.append(record)

            if batch_data:
                plan = self.bulk_upsert(
                    model=model_cls,
                    data=batch_data,
                    key_field=key_field,
                )
                res = await self.run_bulk(plan)
                results.append(res)
                for n in nodes:
                    if hasattr(n, "clear_dirty"):
                        n.clear_dirty()

        return results

    def clear(self) -> None:
        """[Experimental] Clears all tracked entities from the Identity Map."""
        self._identity_map.clear()

    @property
    def dialect(self) -> str:
        """Active dialect for this session."""
        return self._dialect

    @property
    def bridge(self) -> AsyncDatabaseBridge:
        """Active async database bridge."""
        return self._bridge

    def _prepare_statement(
        self,
        query_or_statement: Query | CompiledQuery | str,
        parameters: dict[str, Any] | None = None,
    ) -> tuple[str, dict[str, Any], Query | CompiledQuery | None]:
        stmt = ""
        params = parameters or {}
        q_obj: Query | CompiledQuery | None = None

        if isinstance(query_or_statement, CompiledQuery):
            stmt = query_or_statement.statement
            params = query_or_statement.parameters
            q_obj = query_or_statement
        elif isinstance(query_or_statement, Query):
            opt = (
                query_or_statement._optimize
                if query_or_statement._optimize is not None
                else self._optimize
            )
            lvl = (
                query_or_statement._optimization_level
                if query_or_statement._optimization_level is not None
                else self._optimization_level
            )
            compiled = query_or_statement.compile(
                dialect=self._dialect,
                optimize=opt,
                optimization_level=lvl,
            )
            stmt = compiled.statement
            params = compiled.parameters
            q_obj = query_or_statement
        else:
            stmt = str(query_or_statement)

        return stmt, params, q_obj

    async def execute(
        self,
        query_or_statement: Query | CompiledQuery | str,
        parameters: dict[str, Any] | None = None,
    ) -> ExecutionResult:
        """Asynchronously executes a Query object, CompiledQuery, or raw statement string.

        Args:
            query_or_statement: Voyager Query, CompiledQuery, or raw query statement string.
            parameters: Query parameters dictionary (if statement is a string).

        Returns:
            ExecutionResult containing rows, SQLAlchemy mappings/scalars access, and graph entity extraction.
        """
        stmt, params, q_obj = self._prepare_statement(query_or_statement, parameters)

        if self._active_backend == "native" and self._native_client is not None:
            try:
                native_res = await self._native_client.execute(stmt, params)
                return ExecutionResult(
                    records=None,
                    statement=stmt,
                    query=q_obj,
                    dialect=self._dialect,
                    stream=native_res.stream,
                    summary=native_res.summary,
                )
            except Exception as e:
                if self._requested_backend == "native" or _is_query_semantic_error(e):
                    raise
                if (
                    isinstance(self._bridge, (MockBridge, AsyncMockBridge))
                    and not self._is_explicit_mock
                ):
                    raise RuntimeError(
                        f"Native backend execution failed: {e}. No official database driver available for fallback."
                    ) from e
                self._active_backend = "bridge"

        raw_records = await self._bridge.execute(stmt, params)
        return ExecutionResult(
            records=raw_records,
            statement=stmt,
            query=q_obj,
            dialect=self._dialect,
        )

    async def execute_to_polars(
        self,
        query_or_statement: Query | CompiledQuery | str,
        parameters: dict[str, Any] | None = None,
    ) -> pl.DataFrame:
        """Asynchronously executes a query and streams results into a Polars DataFrame.

        When running on the native network backend, streams Arrow C data directly
        into Polars without intermediate Python dict allocation or ExecutionResult overhead.

        Args:
            query_or_statement: Voyager Query, CompiledQuery, or raw query statement string.
            parameters: Query parameters dictionary (if statement is a string).

        Returns:
            Columnar Polars DataFrame containing the result records.
        """
        stmt, params, _ = self._prepare_statement(query_or_statement, parameters)

        if self._active_backend == "native" and self._native_client is not None:
            try:
                native_res = await self._native_client.execute(stmt, params)
                import polars as pl

                return pl.DataFrame(native_res.stream)
            except Exception as e:
                if self._requested_backend == "native" or _is_query_semantic_error(e):
                    raise
                if (
                    isinstance(self._bridge, (MockBridge, AsyncMockBridge))
                    and not self._is_explicit_mock
                ):
                    raise RuntimeError(
                        f"Native backend execution failed: {e}. No official database driver available for fallback."
                    ) from e
                self._active_backend = "bridge"

        return await self._bridge.execute_to_polars(stmt, params)

    async def execute_to_arrow(
        self,
        query_or_statement: Query | CompiledQuery | str,
        parameters: dict[str, Any] | None = None,
    ) -> pa.Table:
        """Asynchronously executes a query and exports results directly into an Apache Arrow Table.

        When running on the native network backend, streams Arrow C data directly
        into PyArrow without intermediate Python dict allocation.

        Args:
            query_or_statement: Voyager Query, CompiledQuery, or raw query statement string.
            parameters: Query parameters dictionary (if statement is a string).

        Returns:
            PyArrow Table containing the result records.
        """
        stmt, params, _ = self._prepare_statement(query_or_statement, parameters)

        if self._active_backend == "native" and self._native_client is not None:
            try:
                native_res = await self._native_client.execute(stmt, params)
                import pyarrow as pa

                reader = pa.RecordBatchReader.from_stream(native_res.stream)
                return reader.read_all()
            except Exception as e:
                if self._requested_backend == "native" or _is_query_semantic_error(e):
                    raise
                if (
                    isinstance(self._bridge, (MockBridge, AsyncMockBridge))
                    and not self._is_explicit_mock
                ):
                    raise RuntimeError(
                        f"Native backend execution failed: {e}. No official database driver available for fallback."
                    ) from e
                self._active_backend = "bridge"

        res = await self.execute(query_or_statement, parameters)
        return res.to_arrow()

    async def ping(self) -> bool:
        """Pings the database connection asynchronously to verify liveness."""
        if self._active_backend == "native" and self._native_client is not None:
            return bool(await self._native_client.ping())
        ping_fn = getattr(self._bridge, "ping", None)
        if callable(ping_fn):
            res = ping_fn()
            if hasattr(res, "__await__"):
                return bool(await res)
            return bool(res)
        try:
            await self.execute("RETURN 1")
            return True
        except Exception:
            return False

    async def run_bulk(self, plan: BulkIngestionPlan) -> BulkExecutionResult:
        """Asynchronously executes a BulkIngestionPlan across the database bridge.

        Args:
            plan: Prepared bulk ingestion plan.

        Returns:
            Execution metrics summary including total batches, records, and elapsed time.
        """
        return await self._bridge.execute_bulk(plan)

    def bulk_create(
        self,
        model: type[Node],
        data: list[dict[str, Any]] | pl.DataFrame | Any,
        batch_size: int = 50_000,
        dialect: str | None = None,
    ) -> BulkIngestionPlan:
        """Prepares a bulk node creation plan.

        Args:
            model: Node class to instantiate.
            data: Records or Polars DataFrame to insert.
            batch_size: Maximum records per batch transaction.
            dialect: Override dialect.

        Returns:
            BulkIngestionPlan instance.
        """
        target_dialect = dialect or self._dialect
        return create_bulk_create_plan(
            model=model,
            data=data,
            batch_size=batch_size,
            dialect=target_dialect,
        )

    def bulk_upsert(
        self,
        model: type[Node],
        data: list[dict[str, Any]] | pl.DataFrame | Any,
        key_field: str,
        batch_size: int = 50_000,
        dialect: str | None = None,
    ) -> BulkIngestionPlan:
        """Prepares an idempotent bulk upsert plan.

        Args:
            model: Node class to upsert.
            data: Records or Polars DataFrame to upsert.
            key_field: Unique key property field name.
            batch_size: Maximum records per batch transaction.
            dialect: Override dialect.

        Returns:
            BulkIngestionPlan instance.
        """
        target_dialect = dialect or self._dialect
        return create_bulk_merge_plan(
            model=model,
            key_field=key_field,
            data=data,
            batch_size=batch_size,
            dialect=target_dialect,
        )

    def bulk_create_relationships(
        self,
        rel_model: type[Relationship] | str,
        data: list[dict[str, Any]] | pl.DataFrame | Any,
        from_label: str,
        from_key: str,
        to_label: str,
        to_key: str,
        batch_size: int = 50_000,
        dialect: str | None = None,
    ) -> BulkIngestionPlan:
        """Prepares an async bulk relationship creation execution plan."""
        target_dialect = dialect or self._dialect
        return create_bulk_create_rel_plan(
            rel_model=rel_model,
            data=data,
            from_label=from_label,
            from_key=from_key,
            to_label=to_label,
            to_key=to_key,
            batch_size=batch_size,
            dialect=target_dialect,
        )

    async def close(self) -> None:
        """Asynchronously closes the underlying database bridge and native connection pool, clearing the identity map."""
        self.clear()
        if self._native_client is not None:
            self._native_client.close()
        await self._bridge.close()

    async def __aenter__(self) -> AsyncSession:
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        await self.close()
