"""Database bridging layer and driver adapters for Voyager OGM.

Provides vendor-neutral sync and async bridge protocols, enabling applications
to connect existing database drivers (Neo4j/Memgraph Bolt, DuckDB DuckPGQ, etc.)
or mock bridges without hard coupling to specific driver packages.
"""

from __future__ import annotations

import asyncio
import inspect
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Protocol, runtime_checkable

import polars as pl

if TYPE_CHECKING:
    from voyager_ogm.ingestion import BulkIngestionPlan


@dataclass
class BulkExecutionResult:
    """Result summary of a bulk ingestion execution.

    Attributes:
        total_batches: Number of chunks dispatched to the database.
        total_records: Total count of records inserted or merged.
        duration_seconds: Total wall-clock execution time in seconds.
        statement: Parameterized query statement executed.
    """

    total_batches: int
    total_records: int
    duration_seconds: float
    statement: str


@runtime_checkable
class DatabaseBridge(Protocol):
    """Synchronous Database Bridge Protocol."""

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a compiled statement and returns records as a list of dicts.

        Args:
            statement: Query statement string.
            parameters: Query parameters dictionary.

        Returns:
            List of result records.
        """
        ...

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a compiled statement and returns records as a Polars DataFrame.

        Args:
            statement: Query statement string.
            parameters: Query parameters dictionary.

        Returns:
            Polars DataFrame containing result records.
        """
        ...

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk ingestion plan across batches.

        Args:
            plan_or_statement: BulkIngestionPlan instance or query statement.
            batches: Batch parameter dictionaries if statement is raw string.

        Returns:
            BulkExecutionResult metrics.
        """
        ...

    def close(self) -> None:
        """Closes the underlying driver connection."""
        ...


@runtime_checkable
class AsyncDatabaseBridge(Protocol):
    """Asynchronous Database Bridge Protocol."""

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a compiled statement and returns records as a list of dicts.

        Args:
            statement: Query statement string.
            parameters: Query parameters dictionary.

        Returns:
            List of result records.
        """
        ...

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a compiled statement and returns records as a Polars DataFrame.

        Args:
            statement: Query statement string.
            parameters: Query parameters dictionary.

        Returns:
            Polars DataFrame containing result records.
        """
        ...

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk ingestion plan across batches asynchronously.

        Args:
            plan_or_statement: BulkIngestionPlan instance or query statement.
            batches: Batch parameter dictionaries if statement is raw string.

        Returns:
            BulkExecutionResult metrics.
        """
        ...

    async def close(self) -> None:
        """Closes the underlying driver connection."""
        ...


class MockBridge:
    """In-memory Mock Bridge for zero-network testing and query inspection."""

    def __init__(self) -> None:
        self.executed_queries: list[tuple[str, dict[str, Any]]] = []
        self._canned_results: list[list[dict[str, Any]] | pl.DataFrame] = []

    def queue_result(self, result: list[dict[str, Any]] | pl.DataFrame) -> None:
        """Queues a canned result to be returned by future execute() calls.

        Args:
            result: Result records or DataFrame to return.
        """
        self._canned_results.append(result)

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Records the query and returns the next canned result or empty list.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            List of record dictionaries.
        """
        params = parameters or {}
        self.executed_queries.append((statement, params))
        if self._canned_results:
            res = self._canned_results.pop(0)
            if isinstance(res, pl.DataFrame):
                return res.to_dicts()
            return res
        return []

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Records the query and returns the next canned result as a Polars DataFrame.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            Polars DataFrame.
        """
        params = parameters or {}
        self.executed_queries.append((statement, params))
        if self._canned_results:
            res = self._canned_results.pop(0)
            if isinstance(res, pl.DataFrame):
                return res
            return pl.DataFrame(res)
        return pl.DataFrame()

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk ingestion plan against the mock bridge.

        Args:
            plan_or_statement: BulkIngestionPlan or statement string.
            batches: Batch parameter sequence.

        Returns:
            BulkExecutionResult summary.
        """
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0

        if hasattr(plan_or_statement, "statement") and hasattr(plan_or_statement, "__iter__"):
            statement = plan_or_statement.statement
            for batch_item in plan_or_statement:
                total_batches += 1
                batch_data = batch_item.parameters.get("batch", [])
                total_records += len(batch_data)
                self.executed_queries.append((statement, batch_item.parameters))
        else:
            statement = str(plan_or_statement)
            batch_list = batches or []
            total_batches = len(batch_list)
            for b in batch_list:
                batch_data = b.get("batch", [])
                total_records += len(batch_data)
                self.executed_queries.append((statement, b))

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def close(self) -> None:
        """Closes the bridge."""
        pass


class AsyncMockBridge:
    """Asynchronous in-memory Mock Bridge for testing."""

    def __init__(self) -> None:
        self.sync_mock = MockBridge()

    @property
    def executed_queries(self) -> list[tuple[str, dict[str, Any]]]:
        """Returns history of executed queries."""
        return self.sync_mock.executed_queries

    def queue_result(self, result: list[dict[str, Any]] | pl.DataFrame) -> None:
        """Queues canned result for future executions.

        Args:
            result: Result records or DataFrame.
        """
        self.sync_mock.queue_result(result)

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Asynchronously executes and records query.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            List of record dictionaries.
        """
        return self.sync_mock.execute(statement, parameters)

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Asynchronously executes query returning Polars DataFrame.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            Polars DataFrame.
        """
        return self.sync_mock.execute_to_polars(statement, parameters)

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Asynchronously executes bulk ingestion plan.

        Args:
            plan_or_statement: BulkIngestionPlan or statement.
            batches: Batch parameter sequence.

        Returns:
            BulkExecutionResult.
        """
        return self.sync_mock.execute_bulk(plan_or_statement, batches)

    async def close(self) -> None:
        """Closes bridge."""
        pass


class Neo4jBoltBridge:
    """Synchronous Neo4j / Memgraph Bolt protocol driver bridge."""

    def __init__(self, driver: Any, database: str | None = None) -> None:
        """Initializes Neo4jBoltBridge.

        Args:
            driver: Neo4j driver instance.
            database: Optional target database name.
        """
        self.driver = driver
        self.database = database

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a Cypher statement over Neo4j Bolt session.

        Args:
            statement: Cypher statement.
            parameters: Query parameters.

        Returns:
            List of record dictionaries.
        """
        params = parameters or {}
        session_kwargs = {"database": self.database} if self.database else {}
        with self.driver.session(**session_kwargs) as session:
            result = session.run(statement, params)
            return [record.data() for record in result]

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes Cypher statement and returns Polars DataFrame.

        Args:
            statement: Cypher statement.
            parameters: Query parameters.

        Returns:
            Polars DataFrame.
        """
        records = self.execute(statement, parameters)
        return pl.DataFrame(records) if records else pl.DataFrame()

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes bulk batch ingestion over Bolt protocol.

        Args:
            plan_or_statement: BulkIngestionPlan or query statement.
            batches: Batch parameter sequence.

        Returns:
            BulkExecutionResult summary.
        """
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0
        session_kwargs = {"database": self.database} if self.database else {}

        with self.driver.session(**session_kwargs) as session:
            if hasattr(plan_or_statement, "statement") and hasattr(plan_or_statement, "__iter__"):
                statement = plan_or_statement.statement
                for batch_item in plan_or_statement:
                    total_batches += 1
                    batch_data = batch_item.parameters.get("batch", [])
                    total_records += len(batch_data)
                    session.run(statement, batch_item.parameters)
            else:
                statement = str(plan_or_statement)
                batch_list = batches or []
                total_batches = len(batch_list)
                for b in batch_list:
                    batch_data = b.get("batch", [])
                    total_records += len(batch_data)
                    session.run(statement, b)

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def close(self) -> None:
        """Closes the underlying driver."""
        if hasattr(self.driver, "close"):
            self.driver.close()


class AsyncNeo4jBoltBridge:
    """Asynchronous Neo4j / Memgraph Bolt protocol driver bridge."""

    def __init__(self, driver: Any, database: str | None = None) -> None:
        """Initializes AsyncNeo4jBoltBridge.

        Args:
            driver: Async Neo4j driver instance.
            database: Optional target database name.
        """
        self.driver = driver
        self.database = database

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Asynchronously executes Cypher statement over async Bolt session.

        Args:
            statement: Cypher statement.
            parameters: Query parameters.

        Returns:
            List of record dictionaries.
        """
        params = parameters or {}
        session_kwargs = {"database": self.database} if self.database else {}
        async with self.driver.session(**session_kwargs) as session:
            result = await session.run(statement, params)
            records = await result.data()
            return records

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Asynchronously executes Cypher statement returning Polars DataFrame.

        Args:
            statement: Cypher statement.
            parameters: Query parameters.

        Returns:
            Polars DataFrame.
        """
        records = await self.execute(statement, parameters)
        return pl.DataFrame(records) if records else pl.DataFrame()

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Asynchronously executes bulk ingestion plan over Bolt.

        Args:
            plan_or_statement: BulkIngestionPlan or statement.
            batches: Batch parameter sequence.

        Returns:
            BulkExecutionResult metrics.
        """
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0
        session_kwargs = {"database": self.database} if self.database else {}

        async with self.driver.session(**session_kwargs) as session:
            if hasattr(plan_or_statement, "statement") and hasattr(plan_or_statement, "__iter__"):
                statement = plan_or_statement.statement
                for batch_item in plan_or_statement:
                    total_batches += 1
                    batch_data = batch_item.parameters.get("batch", [])
                    total_records += len(batch_data)
                    await session.run(statement, batch_item.parameters)
            else:
                statement = str(plan_or_statement)
                batch_list = batches or []
                total_batches = len(batch_list)
                for b in batch_list:
                    batch_data = b.get("batch", [])
                    total_records += len(batch_data)
                    await session.run(statement, b)

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    async def close(self) -> None:
        """Closes the underlying async driver."""
        if hasattr(self.driver, "close"):
            if inspect.iscoroutinefunction(self.driver.close):
                await self.driver.close()
            else:
                self.driver.close()


class DuckDbBridge:
    """Synchronous DuckDB driver bridge with zero-copy Polars / Arrow output."""

    def __init__(self, connection: Any) -> None:
        """Initializes DuckDbBridge.

        Args:
            connection: DuckDB connection object.
        """
        self.con = connection

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes query on DuckDB.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            List of record dictionaries.
        """
        params = parameters or {}
        rel = self.con.execute(statement, params)
        if rel.description:
            cols = [d[0] for d in rel.description]
            rows = rel.fetchall()
            return [dict(zip(cols, row, strict=False)) for row in rows]
        return []

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes query and streams to Polars DataFrame using zero-copy extraction.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            Polars DataFrame.
        """
        params = parameters or {}
        if hasattr(self.con, "pl"):
            return self.con.execute(statement, params).pl()
        records = self.execute(statement, params)
        return pl.DataFrame(records) if records else pl.DataFrame()

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes bulk ingestion plan in DuckDB.

        Args:
            plan_or_statement: BulkIngestionPlan or statement.
            batches: Batch parameter sequence.

        Returns:
            BulkExecutionResult metrics.
        """
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0

        if hasattr(plan_or_statement, "statement") and hasattr(plan_or_statement, "__iter__"):
            statement = plan_or_statement.statement
            for batch_item in plan_or_statement:
                total_batches += 1
                batch_data = batch_item.parameters.get("batch", [])
                total_records += len(batch_data)
                self.con.execute(statement, batch_item.parameters)
        else:
            statement = str(plan_or_statement)
            batch_list = batches or []
            total_batches = len(batch_list)
            for b in batch_list:
                batch_data = b.get("batch", [])
                total_records += len(batch_data)
                self.con.execute(statement, b)

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def close(self) -> None:
        """Closes connection."""
        if hasattr(self.con, "close"):
            self.con.close()


class AsyncDuckDbBridge:
    """Asynchronous DuckDB driver bridge using asyncio worker threads."""

    def __init__(self, connection: Any) -> None:
        """Initializes AsyncDuckDbBridge.

        Args:
            connection: DuckDB connection object.
        """
        self.sync_bridge = DuckDbBridge(connection)

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Asynchronously executes query.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            List of record dictionaries.
        """
        return await asyncio.to_thread(self.sync_bridge.execute, statement, parameters)

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Asynchronously streams query results to Polars DataFrame.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            Polars DataFrame.
        """
        return await asyncio.to_thread(self.sync_bridge.execute_to_polars, statement, parameters)

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Asynchronously executes bulk ingestion plan.

        Args:
            plan_or_statement: BulkIngestionPlan or statement.
            batches: Batch parameter sequence.

        Returns:
            BulkExecutionResult.
        """
        return await asyncio.to_thread(self.sync_bridge.execute_bulk, plan_or_statement, batches)

    async def close(self) -> None:
        """Closes connection asynchronously."""
        await asyncio.to_thread(self.sync_bridge.close)


# =========================================================================
# Dynamic Bridge Registry and Factory
# =========================================================================

_BRIDGE_REGISTRY: list[tuple[Callable[[Any], bool], type, bool]] = []


def register_bridge(
    predicate_or_type: type | Callable[[Any], bool],
    bridge_class: type,
    is_async: bool = False,
) -> None:
    """Registers a new database driver adapter into the global bridge registry.

    Allows third-party and custom drivers to be auto-detected by Session.

    Args:
        predicate_or_type: Driver class type or matcher predicate function.
        bridge_class: Adapter class to instantiate.
        is_async: Flag indicating whether this adapter implements AsyncDatabaseBridge.
    """
    if isinstance(predicate_or_type, type):
        target_cls = predicate_or_type

        def matcher(obj: Any) -> bool:
            return isinstance(obj, target_cls)

    else:
        matcher = predicate_or_type

    _BRIDGE_REGISTRY.insert(0, (matcher, bridge_class, is_async))


# Register built-in driver detectors
def _is_neo4j_sync_driver(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    return "neo4j" in type_name and "Async" not in type_name and hasattr(obj, "session")


def _is_neo4j_async_driver(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    return "neo4j" in type_name and "Async" in type_name and hasattr(obj, "session")


def _is_duckdb_conn(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    return "duckdb" in type_name and hasattr(obj, "execute")


register_bridge(_is_neo4j_sync_driver, Neo4jBoltBridge, is_async=False)
register_bridge(_is_neo4j_async_driver, AsyncNeo4jBoltBridge, is_async=True)
register_bridge(_is_duckdb_conn, DuckDbBridge, is_async=False)


def create_bridge(
    driver_or_connection: Any, is_async: bool = False
) -> DatabaseBridge | AsyncDatabaseBridge:
    """Creates or adapts a database bridge from a user-supplied driver or connection.

    Args:
        driver_or_connection: Database driver, connection instance, MockBridge, or custom bridge.
        is_async: Whether an async bridge is required.

    Returns:
        Configured database bridge adapter.
    """
    # 1. If already implements bridge protocol
    if is_async and isinstance(driver_or_connection, AsyncDatabaseBridge):
        return driver_or_connection
    if not is_async and isinstance(driver_or_connection, DatabaseBridge):
        return driver_or_connection

    # 2. Check dynamic registry matching the requested async/sync mode
    for matcher, bridge_cls, reg_is_async in _BRIDGE_REGISTRY:
        if reg_is_async == is_async and matcher(driver_or_connection):
            return bridge_cls(driver_or_connection)

    # 3. Fallback: wrap sync DuckDB bridge with AsyncDuckDbBridge if async was requested
    if is_async:
        for matcher, bridge_cls, reg_is_async in _BRIDGE_REGISTRY:
            if not reg_is_async and matcher(driver_or_connection):
                sync_inst = bridge_cls(driver_or_connection)
                if isinstance(sync_inst, DuckDbBridge):
                    return AsyncDuckDbBridge(driver_or_connection)

    # 3. Default fallback to MockBridge if None or unrecognized
    if driver_or_connection is None:
        return AsyncMockBridge() if is_async else MockBridge()

    # Duck typing wrapper for generic objects
    if is_async:
        return AsyncMockBridge()
    return MockBridge()
