"""Session management and high-level execution facade for Voyager OGM."""

from __future__ import annotations

from collections.abc import Iterator, Sequence
from typing import TYPE_CHECKING, Any

from voyager_ogm.bridge import (
    AsyncDatabaseBridge,
    BulkExecutionResult,
    DatabaseBridge,
    create_bridge,
)
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
        records: list[dict[str, Any]],
        statement: str = "",
        query: Query | CompiledQuery | None = None,
        dialect: str = "cypher",
    ) -> None:
        super().__init__(records)
        self._records = records
        self._statement = statement
        self._query = query
        self._dialect = dialect
        self._nodes: list[dict[str, Any]] | None = None
        self._edges: list[dict[str, Any]] | None = None

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
        return MappingsResult(self._records)

    def scalars(self) -> ScalarsResult:
        """Provides an SQLAlchemy-style single-column scalar extraction view."""
        return ScalarsResult(self._records)

    def all(self) -> list[dict[str, Any]]:
        """Returns all result records as a list of dictionaries."""
        return list(self._records)

    def fetchall(self) -> list[dict[str, Any]]:
        """Synonym for `.all()`."""
        return self.all()

    def first(self) -> dict[str, Any] | None:
        """Returns the first record as a dictionary, or None if empty."""
        return self._records[0] if self._records else None

    def fetchone(self) -> dict[str, Any] | None:
        """Synonym for `.first()`."""
        return self.first()

    def to_polars(self) -> pl.DataFrame:
        """Exports the result records into a Polars DataFrame."""
        import polars as pl

        return pl.DataFrame(self._records) if self._records else pl.DataFrame()

    def to_arrow(self) -> pa.Table:
        """Exports the result records into an Apache Arrow Table."""
        import pyarrow as pa

        return pa.Table.from_pylist(self._records) if self._records else pa.Table.from_pylist([])

    def to_graph(self) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
        """Extracts live graph nodes, relationships, and reconstructed paths from result records."""
        if self._nodes is not None and self._edges is not None:
            return self._nodes, self._edges

        from voyager_ogm.viewer import extract_graph_entities_from_records

        self._nodes, self._edges = extract_graph_entities_from_records(
            records=self._records,
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
            records=self._records,
            query_statement=self._statement,
            default_view="graph" if edges or nodes else "table",
            **kwargs,
        )

    def explore(self, **kwargs: Any) -> Any:
        """Synonym for `.show()`."""
        return self.show(**kwargs)


# Alias for backward compatibility
Result = ExecutionResult


class Session:
    """Synchronous graph database session coordinator for Voyager OGM.

    Provides high-throughput bulk ingestion, Unit-of-Work transactions, and query dispatch
    over pluggable database drivers (Neo4j, Memgraph, DuckDB, Mock, etc.).
    """

    def __init__(self, bridge: Any = None, dialect: str = "cypher") -> None:
        """Initializes a new Voyager Session.

        Args:
            bridge: Pluggable database driver (Neo4j Driver, DuckDB connection, MockBridge, etc.).
                If omitted or None, defaults to MockBridge.
            dialect: Default query dialect for statements generated by this session
                ('cypher', 'iso_gql', 'sql_pgq').
        """
        self._dialect = dialect
        self._bridge: DatabaseBridge = create_bridge(bridge, is_async=False)  # type: ignore[assignment]

    @property
    def dialect(self) -> str:
        """Active dialect for this session."""
        return self._dialect

    @property
    def bridge(self) -> DatabaseBridge:
        """Active database bridge."""
        return self._bridge

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
        stmt = ""
        params = parameters or {}
        q_obj: Query | CompiledQuery | None = None

        if isinstance(query_or_statement, CompiledQuery):
            stmt = query_or_statement.statement
            params = query_or_statement.parameters
            raw_records = self._bridge.execute(stmt, params)
            q_obj = query_or_statement
        elif isinstance(query_or_statement, Query):
            compiled = query_or_statement.compile(dialect=self._dialect)
            stmt = compiled.statement
            params = compiled.parameters
            raw_records = self._bridge.execute(stmt, params)
            q_obj = query_or_statement
        else:
            stmt = str(query_or_statement)
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

        Args:
            query_or_statement: Voyager Query, CompiledQuery, or raw query statement string.
            parameters: Query parameters dictionary (if statement is a string).

        Returns:
            Columnar Polars DataFrame containing the result records.
        """
        if isinstance(query_or_statement, CompiledQuery):
            return self._bridge.execute_to_polars(
                query_or_statement.statement, query_or_statement.parameters
            )
        if isinstance(query_or_statement, Query):
            compiled = query_or_statement.compile(dialect=self._dialect)
            return self._bridge.execute_to_polars(compiled.statement, compiled.parameters)
        return self._bridge.execute_to_polars(str(query_or_statement), parameters)

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
        """Closes the underlying database bridge connection."""
        self._bridge.close()


class AsyncSession:
    """Asynchronous graph database session coordinator for Voyager OGM.

    Provides non-blocking async/await query execution and bulk ingestion over
    async database drivers (e.g. neo4j.AsyncDriver).
    """

    def __init__(self, bridge: Any = None, dialect: str = "cypher") -> None:
        """Initializes a new asynchronous Voyager Session.

        Args:
            bridge: Pluggable async database driver (neo4j.AsyncDriver, AsyncMockBridge, etc.).
            dialect: Default query dialect for statements generated by this session.
        """
        self._dialect = dialect
        self._bridge: AsyncDatabaseBridge = create_bridge(bridge, is_async=True)  # type: ignore[assignment]

    @property
    def dialect(self) -> str:
        """Active dialect for this session."""
        return self._dialect

    @property
    def bridge(self) -> AsyncDatabaseBridge:
        """Active async database bridge."""
        return self._bridge

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
        stmt = ""
        params = parameters or {}
        q_obj: Query | CompiledQuery | None = None

        if isinstance(query_or_statement, CompiledQuery):
            stmt = query_or_statement.statement
            params = query_or_statement.parameters
            raw_records = await self._bridge.execute(stmt, params)
            q_obj = query_or_statement
        elif isinstance(query_or_statement, Query):
            compiled = query_or_statement.compile(dialect=self._dialect)
            stmt = compiled.statement
            params = compiled.parameters
            raw_records = await self._bridge.execute(stmt, params)
            q_obj = query_or_statement
        else:
            stmt = str(query_or_statement)
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

        Args:
            query_or_statement: Voyager Query, CompiledQuery, or raw query statement string.
            parameters: Query parameters dictionary (if statement is a string).

        Returns:
            Columnar Polars DataFrame containing the result records.
        """
        if isinstance(query_or_statement, CompiledQuery):
            return await self._bridge.execute_to_polars(
                query_or_statement.statement, query_or_statement.parameters
            )
        if isinstance(query_or_statement, Query):
            compiled = query_or_statement.compile(dialect=self._dialect)
            return await self._bridge.execute_to_polars(compiled.statement, compiled.parameters)
        return await self._bridge.execute_to_polars(str(query_or_statement), parameters)

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
        """Asynchronously closes the underlying database bridge connection."""
        await self._bridge.close()
