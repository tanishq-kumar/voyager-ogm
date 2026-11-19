"""Database bridging layer and driver adapters for Voyager OGM.

Provides vendor-neutral sync and async bridge protocols, enabling applications
to connect existing database drivers (Neo4j/Memgraph Bolt, DuckDB DuckPGQ, etc.)
or mock bridges without hard coupling to specific driver packages.
"""

from __future__ import annotations

import asyncio
import datetime
import inspect
import json
import re
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Any, Literal, Protocol, overload, runtime_checkable

import polars as pl

from voyager_ogm.ingestion import BulkIngestionPlan
from voyager_ogm.types import unwrap_spatial_param


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


def _validate_savepoint_name(name: str) -> str:
    """Validates an unquoted savepoint identifier before interpolating SQL.

    Args:
        name: Caller-supplied savepoint name.

    Returns:
        The validated savepoint name.

    Raises:
        ValueError: If the name is not a safe SQL identifier.
    """
    if not isinstance(name, str) or not name:
        raise ValueError("Savepoint name must be a non-empty string")
    if not (name[0].isalpha() or name[0] == "_"):
        raise ValueError(f"Invalid savepoint name {name!r}")
    if not all(ch.isalnum() or ch == "_" for ch in name):
        raise ValueError(f"Invalid savepoint name {name!r}")
    return name


_SAFE_GRAPH_NAME_REGEX = re.compile(r"^[A-Za-z_][A-Za-z0-9_]{0,62}$")


def validate_graph_name(graph_name: str) -> str:
    """Validates that a graph name is a safe SQL/AGE identifier.

    Graph names in Apache AGE are interpolated into SQL queries
    (e.g. `cypher('<graph_name>', ...)`). To prevent SQL injection and syntax
    errors, graph names must be valid alphanumeric/underscore identifiers
    starting with an ASCII letter or underscore, up to 63 characters in length.

    Args:
        graph_name: The graph name to validate.

    Returns:
        The validated graph name.

    Raises:
        ValueError: If graph_name is not a valid identifier.
    """
    if not isinstance(graph_name, str) or not _SAFE_GRAPH_NAME_REGEX.match(graph_name):
        raise ValueError(
            f"Invalid graph name {graph_name!r}. Graph names must be valid alphanumeric/underscore "
            "identifiers (1-63 characters, starting with an ASCII letter or underscore)."
        )
    return graph_name


def _pg_transaction_status(conn: Any) -> str | None:
    """Returns the psycopg transaction status name when the driver exposes it."""
    info = getattr(conn, "info", None)
    if info is not None and hasattr(info, "transaction_status"):
        status = info.transaction_status
        return str(getattr(status, "name", status)).upper()
    getter = getattr(conn, "get_transaction_status", None)
    if callable(getter):
        try:
            status = getter()
        except Exception:
            return None
        return str(getattr(status, "name", status)).upper()
    return None


def _pg_connection_closed(conn: Any) -> bool:
    """Returns whether a psycopg connection reports itself closed."""
    value = getattr(conn, "closed", 0)
    try:
        return bool(value)
    except Exception:
        return False


class MockTransaction:
    """In-memory stand-in for a database transaction on MockBridge."""

    def __init__(self, bridge: MockBridge) -> None:
        """Initializes the mock transaction.

        Args:
            bridge: Parent MockBridge that records executed statements.
        """
        self._bridge = bridge
        self._state = "ACTIVE"
        self._savepoints: dict[str, int] = {}

    @property
    def state(self) -> str:
        """Returns the lifecycle state ('ACTIVE', 'FAILED', 'COMMITTED', 'ROLLED_BACK')."""
        return self._state

    @property
    def is_active(self) -> bool:
        """Returns whether the mock transaction accepts statements."""
        return self._state == "ACTIVE"

    @property
    def supports_savepoints(self) -> bool:
        """Mock transactions support in-memory savepoints."""
        return True

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a statement through the mock bridge."""
        if not self.is_active:
            raise ValueError("Cannot execute query on inactive transaction")
        return self._bridge.execute(statement, parameters)

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a statement through the mock bridge and returns a DataFrame."""
        if not self.is_active:
            raise ValueError("Cannot execute query on inactive transaction")
        return self._bridge.execute_to_polars(statement, parameters)

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk plan through the mock bridge."""
        if not self.is_active:
            raise ValueError("Cannot execute bulk plan on inactive transaction")
        return self._bridge.execute_bulk(plan_or_statement, batches)

    def commit(self) -> None:
        """Marks the mock transaction committed."""
        if not self.is_active:
            raise ValueError("Cannot commit inactive transaction")
        self._state = "COMMITTED"
        self._savepoints.clear()

    def rollback(self) -> None:
        """Marks the mock transaction rolled back."""
        if not self.is_active:
            raise ValueError("Cannot rollback inactive transaction")
        self._state = "ROLLED_BACK"
        self._savepoints.clear()

    def savepoint(self, name: str) -> None:
        """Creates an in-memory savepoint and records the query log length."""
        if not self.is_active:
            raise ValueError("Cannot create savepoint on inactive transaction")
        _validate_savepoint_name(name)
        if name in self._savepoints:
            raise ValueError(f"Savepoint {name!r} already exists")
        self._savepoints[name] = len(self._bridge.executed_queries)

    def rollback_to_savepoint(self, name: str) -> None:
        """Rolls the mock query log back to a savepoint."""
        if not self.is_active:
            raise ValueError("Cannot rollback savepoint on inactive transaction")
        if name not in self._savepoints:
            raise ValueError(f"Savepoint {name!r} does not exist")
        del self._bridge.executed_queries[self._savepoints[name] :]

    def release_savepoint(self, name: str) -> None:
        """Releases an in-memory savepoint."""
        if not self.is_active:
            raise ValueError("Cannot release savepoint on inactive transaction")
        if name not in self._savepoints:
            raise ValueError(f"Savepoint {name!r} does not exist")
        self._savepoints.pop(name, None)


class MockBridge:
    """In-memory Mock Bridge for zero-network testing and query inspection."""

    def __init__(self, graph_name: str | None = None) -> None:
        if graph_name is not None:
            validate_graph_name(graph_name)
        self.graph_name = graph_name
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
        params = unwrap_spatial_param(parameters or {})
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
        params = unwrap_spatial_param(parameters or {})
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

        if isinstance(plan_or_statement, str):
            statement = plan_or_statement
            batch_list = batches or []
            total_batches = len(batch_list)
            for b in batch_list:
                batch_data = b.get("batch", [])
                total_records += len(batch_data)
                self.executed_queries.append((statement, unwrap_spatial_param(b)))
        else:
            statement = plan_or_statement.statement
            for batch_item in plan_or_statement:
                total_batches += 1
                batch_data = batch_item.parameters.get("batch", [])
                total_records += len(batch_data)
                self.executed_queries.append(
                    (statement, unwrap_spatial_param(batch_item.parameters))
                )

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def begin_transaction(self) -> MockTransaction:
        """Opens an in-memory mock transaction."""
        return MockTransaction(self)

    def ping(self) -> bool:
        """Pings the mock bridge."""
        return True

    def close(self) -> None:
        """Closes the bridge."""
        pass


class AsyncMockTransaction:
    """Asynchronous in-memory stand-in for a database transaction."""

    def __init__(self, transaction: MockTransaction) -> None:
        """Initializes the async wrapper around a sync mock transaction."""
        self._sync = transaction

    @property
    def state(self) -> str:
        """Returns the lifecycle state."""
        return self._sync.state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        return self._sync.is_active

    @property
    def supports_savepoints(self) -> bool:
        """Mock transactions support in-memory savepoints."""
        return True

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a statement through the mock bridge."""
        return self._sync.execute(statement, parameters)

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a statement and returns a Polars DataFrame."""
        return self._sync.execute_to_polars(statement, parameters)

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk plan through the mock bridge."""
        return self._sync.execute_bulk(plan_or_statement, batches)

    async def commit(self) -> None:
        """Marks the mock transaction committed."""
        self._sync.commit()

    async def rollback(self) -> None:
        """Marks the mock transaction rolled back."""
        self._sync.rollback()

    async def savepoint(self, name: str) -> None:
        """Creates an in-memory savepoint."""
        self._sync.savepoint(name)

    async def rollback_to_savepoint(self, name: str) -> None:
        """Rolls the mock query log back to a savepoint."""
        self._sync.rollback_to_savepoint(name)

    async def release_savepoint(self, name: str) -> None:
        """Releases an in-memory savepoint."""
        self._sync.release_savepoint(name)


class AsyncMockBridge:
    """Asynchronous in-memory Mock Bridge for testing."""

    def __init__(self, sync_mock: MockBridge | None = None, graph_name: str | None = None) -> None:
        if graph_name is not None:
            validate_graph_name(graph_name)
        self.sync_mock = sync_mock or MockBridge(graph_name=graph_name)
        if graph_name is not None:
            self.sync_mock.graph_name = graph_name

    @property
    def graph_name(self) -> str | None:
        """Returns the configured Apache AGE graph name."""
        return self.sync_mock.graph_name

    @graph_name.setter
    def graph_name(self, value: str | None) -> None:
        if value is not None:
            validate_graph_name(value)
        self.sync_mock.graph_name = value

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

    async def begin_transaction(self) -> AsyncMockTransaction:
        """Opens an in-memory mock transaction."""
        return AsyncMockTransaction(MockTransaction(self.sync_mock))

    async def ping(self) -> bool:
        """Pings the async mock bridge."""
        return True

    async def close(self) -> None:
        """Closes bridge."""
        pass


def _adapt_bolt_value(val: Any) -> Any:
    """Adapts tagged spatial values to Bolt driver spatial objects (WGS84Point / CartesianPoint)."""
    try:
        from neo4j.spatial import CartesianPoint, WGS84Point
    except ImportError:
        if isinstance(val, dict) and "__voyager_spatial__" in val:
            return val["__voyager_spatial__"]
        return val

    from voyager_ogm.types import WGS_84_2D, WGS_84_3D, Point

    if isinstance(val, Point):
        if val.is_geographic:
            lat = float(val.latitude) if val.latitude is not None else float(val.y)
            lon = float(val.longitude) if val.longitude is not None else float(val.x)
            if val.is_3d and val.height is not None:
                return WGS84Point((lon, lat, float(val.height)))
            return WGS84Point((lon, lat))
        else:
            if val.is_3d and val.z is not None:
                return CartesianPoint((float(val.x), float(val.y), float(val.z)))
            return CartesianPoint((float(val.x), float(val.y)))

    if isinstance(val, dict):
        if "__voyager_spatial__" in val:
            sp = val["__voyager_spatial__"]
            if not isinstance(sp, dict):
                return sp
            srid = sp.get("srid")
            is_geo = (
                (srid in (WGS_84_2D, WGS_84_3D))
                if srid
                else ("latitude" in sp or "longitude" in sp)
            )
            if is_geo:
                lat_raw = sp.get("latitude") if "latitude" in sp else sp.get("y")
                lon_raw = sp.get("longitude") if "longitude" in sp else sp.get("x")
                if lat_raw is None or lon_raw is None:
                    raise ValueError(
                        f"Geographic spatial data missing required latitude/longitude: {sp}"
                    )
                lat = float(lat_raw)
                lon = float(lon_raw)
                height = sp.get("height", sp.get("z"))
                if height is not None:
                    return WGS84Point((lon, lat, float(height)))
                return WGS84Point((lon, lat))
            else:
                if "x" not in sp or "y" not in sp or sp["x"] is None or sp["y"] is None:
                    raise ValueError(
                        f"Cartesian spatial data missing required 'x' or 'y' coordinate: {sp}"
                    )
                x = float(sp["x"])
                y = float(sp["y"])
                z = sp.get("z")
                if z is not None:
                    return CartesianPoint((x, y, float(z)))
                return CartesianPoint((x, y))
        return {k: _adapt_bolt_value(v) for k, v in val.items()}

    if isinstance(val, (list, tuple)):
        return [_adapt_bolt_value(x) for x in val]

    return val


def _adapt_bolt_parameters(params: dict[str, Any]) -> dict[str, Any]:
    """Adapts statement and batch parameters for Bolt protocol execution, converting tagged spatial values to native spatial points."""
    return {k: _adapt_bolt_value(v) for k, v in params.items()}


def _normalize_bolt_record(record: Any) -> dict[str, Any]:
    """Extracts record dictionary from Bolt Record, ensuring relationship entities retain properties."""
    if isinstance(record, dict):
        return record
    row = dict(record.data()) if hasattr(record, "data") else dict(record)
    items_fn = getattr(record, "items", None)
    if callable(items_fn):
        for k, v in items_fn():
            if hasattr(v, "start_node") and hasattr(v, "type"):
                row[k] = dict(v)
    return row


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
        params = _adapt_bolt_parameters(parameters or {})
        session_kwargs = {"database": self.database} if self.database else {}
        with self.driver.session(**session_kwargs) as session:
            result = session.run(statement, params)
            return [_normalize_bolt_record(record) for record in result]

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
            if isinstance(plan_or_statement, str):
                statement = plan_or_statement
                batch_list = batches or []
                total_batches = len(batch_list)
                for b in batch_list:
                    batch_data = b.get("batch", [])
                    total_records += len(batch_data)
                    session.run(statement, _adapt_bolt_parameters(b))
            else:
                statement = plan_or_statement.statement
                for batch_item in plan_or_statement:
                    total_batches += 1
                    batch_data = batch_item.parameters.get("batch", [])
                    total_records += len(batch_data)
                    session.run(statement, _adapt_bolt_parameters(batch_item.parameters))

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def ping(self) -> bool:
        """Pings Neo4j connection to verify liveness."""
        if hasattr(self.driver, "verify_connectivity"):
            try:
                self.driver.verify_connectivity()
                return True
            except Exception:
                return False
        try:
            self.execute("RETURN 1")
            return True
        except Exception:
            return False

    def begin_transaction(self) -> Neo4jBoltTransaction:
        """Begins an explicit Bolt transaction and leases one driver session."""
        return Neo4jBoltTransaction(self.driver, self.database)

    def close(self) -> None:
        """Closes the underlying driver."""
        if hasattr(self.driver, "close"):
            self.driver.close()


class Neo4jBoltTransaction:
    """Explicit Bolt transaction holding one Neo4j/Memgraph driver session."""

    _savepoint_error_message = (
        "Neo4j/Memgraph Bolt transactions do not support savepoints; "
        "use Apache AGE/PostgreSQL for real savepoints"
    )

    def __init__(self, driver: Any, database: str | None = None) -> None:
        """Begins a driver transaction.

        Args:
            driver: Neo4j-compatible driver instance.
            database: Optional target database name.
        """
        self._session = driver.session(**({"database": database} if database else {}))
        self._state = "ACTIVE"
        try:
            self._tx = self._session.begin_transaction()
        except Exception:
            self._session.close()
            self._state = "ROLLED_BACK"
            raise

    @property
    def state(self) -> str:
        """Returns the lifecycle state ('ACTIVE', 'FAILED', 'COMMITTED', 'ROLLED_BACK')."""
        return self._state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        if self._state != "ACTIVE":
            return False
        closed = getattr(self._tx, "closed", None)
        if callable(closed):
            try:
                return not bool(closed())
            except Exception:
                return True
        return True

    @property
    def supports_savepoints(self) -> bool:
        """Bolt transactions (Neo4j/Memgraph) have no savepoints."""
        return False

    def _mark_failed(self) -> None:
        """Marks the transaction aborted after a statement/driver error."""
        self._state = "FAILED"

    def _close_session(self) -> None:
        """Closes the leased driver session without masking the active error."""
        try:
            self._session.close()
        except Exception:
            pass

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Runs a Cypher statement inside the transaction."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back before executing more statements")
        if not self.is_active:
            raise ValueError("Cannot execute query on inactive transaction")
        try:
            result = self._tx.run(statement, _adapt_bolt_parameters(parameters or {}))
            return [_normalize_bolt_record(record) for record in result]
        except BaseException:
            self._mark_failed()
            raise

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Runs a Cypher statement and returns a Polars DataFrame."""
        records = self.execute(statement, parameters)
        return pl.DataFrame(records) if records else pl.DataFrame()

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Runs a bulk plan on the transaction's driver transaction."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back before executing more statements")
        if not self.is_active:
            raise ValueError("Cannot execute bulk plan on inactive transaction")
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0

        try:
            if isinstance(plan_or_statement, str):
                statement = plan_or_statement
                batch_list = batches or []
                total_batches = len(batch_list)
                for b in batch_list:
                    total_records += len(b.get("batch", []))
                    self._tx.run(statement, _adapt_bolt_parameters(b))
            else:
                statement = plan_or_statement.statement
                for batch_item in plan_or_statement:
                    total_batches += 1
                    total_records += len(batch_item.parameters.get("batch", []))
                    self._tx.run(statement, _adapt_bolt_parameters(batch_item.parameters))
        except BaseException:
            self._mark_failed()
            raise

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def commit(self) -> None:
        """Commits the Bolt transaction and closes the driver session."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back instead of committing")
        if not self.is_active:
            raise ValueError("Cannot commit inactive transaction")
        try:
            self._tx.commit()
            self._state = "COMMITTED"
        except BaseException:
            self._mark_failed()
            raise
        finally:
            self._close_session()

    def rollback(self) -> None:
        """Rolls the Bolt transaction back and closes the driver session."""
        if self._state not in ("ACTIVE", "FAILED"):
            raise ValueError("Cannot rollback inactive transaction")
        try:
            self._tx.rollback()
            self._state = "ROLLED_BACK"
        except BaseException:
            self._mark_failed()
            raise
        finally:
            self._close_session()

    def savepoint(self, name: str) -> None:
        """Raises because Bolt engines do not support savepoints."""
        raise RuntimeError(
            "Neo4j/Memgraph Bolt transactions do not support savepoints; "
            "use Apache AGE/PostgreSQL for real savepoints"
        )

    def rollback_to_savepoint(self, name: str) -> None:
        """Raises because Bolt engines do not support savepoints."""
        raise RuntimeError(
            "Neo4j/Memgraph Bolt transactions do not support savepoints; "
            "use Apache AGE/PostgreSQL for real savepoints"
        )

    def release_savepoint(self, name: str) -> None:
        """Raises because Bolt engines do not support savepoints."""
        raise RuntimeError(
            "Neo4j/Memgraph Bolt transactions do not support savepoints; "
            "use Apache AGE/PostgreSQL for real savepoints"
        )


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
        params = _adapt_bolt_parameters(parameters or {})
        session_kwargs = {"database": self.database} if self.database else {}
        async with self.driver.session(**session_kwargs) as session:
            result = await session.run(statement, params)
            if hasattr(result, "__aiter__"):
                records = []
                async for record in result:
                    records.append(_normalize_bolt_record(record))
                return records
            raw = await result.data()
            return [_normalize_bolt_record(r) for r in raw]

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
            if isinstance(plan_or_statement, str):
                statement = plan_or_statement
                batch_list = batches or []
                total_batches = len(batch_list)
                for b in batch_list:
                    batch_data = b.get("batch", [])
                    total_records += len(batch_data)
                    await session.run(statement, _adapt_bolt_parameters(b))
            else:
                statement = plan_or_statement.statement
                for batch_item in plan_or_statement:
                    total_batches += 1
                    batch_data = batch_item.parameters.get("batch", [])
                    total_records += len(batch_data)
                    await session.run(statement, _adapt_bolt_parameters(batch_item.parameters))

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    async def ping(self) -> bool:
        """Pings Neo4j connection asynchronously to verify liveness."""
        if hasattr(self.driver, "verify_connectivity"):
            try:
                res = self.driver.verify_connectivity()
                if hasattr(res, "__await__"):
                    await res
                return True
            except Exception:
                return False
        try:
            await self.execute("RETURN 1")
            return True
        except Exception:
            return False

    async def begin_transaction(self) -> AsyncNeo4jBoltTransaction:
        """Begins an explicit async Bolt transaction and leases one driver session."""
        session = self.driver.session(**({"database": self.database} if self.database else {}))
        try:
            tx = await session.begin_transaction()
        except Exception:
            await session.close()
            raise
        return AsyncNeo4jBoltTransaction(session, tx)

    async def close(self) -> None:
        """Closes the underlying async driver."""
        if hasattr(self.driver, "close"):
            if inspect.iscoroutinefunction(self.driver.close):
                await self.driver.close()
            else:
                self.driver.close()


class AsyncNeo4jBoltTransaction:
    """Explicit async Bolt transaction holding one driver session."""

    _savepoint_error_message = (
        "Neo4j/Memgraph Bolt transactions do not support savepoints; "
        "use Apache AGE/PostgreSQL for real savepoints"
    )

    def __init__(self, session: Any, tx: Any) -> None:
        """Initializes the transaction wrapper.

        Args:
            session: Async neo4j driver session.
            tx: Async neo4j transaction.
        """
        self._session = session
        self._tx = tx
        self._state = "ACTIVE"

    @property
    def state(self) -> str:
        """Returns the lifecycle state."""
        return self._state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        if self._state != "ACTIVE":
            return False
        closed = getattr(self._tx, "closed", None)
        if callable(closed):
            try:
                return not bool(closed())
            except Exception:
                return True
        return True

    @property
    def supports_savepoints(self) -> bool:
        """Bolt transactions have no savepoints."""
        return False

    def _mark_failed(self) -> None:
        """Marks the transaction aborted after a statement/driver error."""
        self._state = "FAILED"

    async def _close_session(self) -> None:
        """Closes the leased driver session without masking the active error."""
        try:
            await self._session.close()
        except Exception:
            pass

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Runs a Cypher statement inside the transaction."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back before executing more statements")
        if not self.is_active:
            raise ValueError("Cannot execute query on inactive transaction")
        try:
            result = await self._tx.run(statement, _adapt_bolt_parameters(parameters or {}))
            if hasattr(result, "__aiter__"):
                records = []
                async for record in result:
                    records.append(_normalize_bolt_record(record))
                return records
            raw = await result.data()
            return [_normalize_bolt_record(r) for r in raw]
        except BaseException:
            self._mark_failed()
            raise

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Runs a Cypher statement and returns a Polars DataFrame."""
        records = await self.execute(statement, parameters)
        return pl.DataFrame(records) if records else pl.DataFrame()

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Runs a bulk plan on the transaction's driver transaction."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back before executing more statements")
        if not self.is_active:
            raise ValueError("Cannot execute bulk plan on inactive transaction")
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0

        try:
            if isinstance(plan_or_statement, str):
                statement = plan_or_statement
                batch_list = batches or []
                total_batches = len(batch_list)
                for b in batch_list:
                    total_records += len(b.get("batch", []))
                    await self._tx.run(statement, _adapt_bolt_parameters(b))
            else:
                statement = plan_or_statement.statement
                for batch_item in plan_or_statement:
                    total_batches += 1
                    total_records += len(batch_item.parameters.get("batch", []))
                    await self._tx.run(statement, _adapt_bolt_parameters(batch_item.parameters))
        except BaseException:
            self._mark_failed()
            raise

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    async def commit(self) -> None:
        """Commits the transaction and closes the driver session."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back instead of committing")
        if not self.is_active:
            raise ValueError("Cannot commit inactive transaction")
        try:
            await self._tx.commit()
            self._state = "COMMITTED"
        except BaseException:
            self._mark_failed()
            raise
        finally:
            await self._close_session()

    async def rollback(self) -> None:
        """Rolls the transaction back and closes the driver session."""
        if self._state not in ("ACTIVE", "FAILED"):
            raise ValueError("Cannot rollback inactive transaction")
        try:
            await self._tx.rollback()
            self._state = "ROLLED_BACK"
        except BaseException:
            self._mark_failed()
            raise
        finally:
            await self._close_session()

    async def savepoint(self, name: str) -> None:
        """Raises because Bolt engines do not support savepoints."""
        raise RuntimeError(
            "Neo4j/Memgraph Bolt transactions do not support savepoints; "
            "use Apache AGE/PostgreSQL for real savepoints"
        )

    async def rollback_to_savepoint(self, name: str) -> None:
        """Raises because Bolt engines do not support savepoints."""
        raise RuntimeError(
            "Neo4j/Memgraph Bolt transactions do not support savepoints; "
            "use Apache AGE/PostgreSQL for real savepoints"
        )

    async def release_savepoint(self, name: str) -> None:
        """Raises because Bolt engines do not support savepoints."""
        raise RuntimeError(
            "Neo4j/Memgraph Bolt transactions do not support savepoints; "
            "use Apache AGE/PostgreSQL for real savepoints"
        )


class DuckDbBridge:
    """Synchronous DuckDB driver bridge with zero-copy Polars / Arrow output."""

    def __init__(self, connection: Any) -> None:
        """Initializes DuckDbBridge.

        Args:
            connection: DuckDB connection object.
        """
        self.con = connection

    def _format_pgq_statement(
        self, statement: str, parameters: dict[str, Any]
    ) -> tuple[str, dict[str, Any]]:
        """Interpolates parameters for DuckPGQ GRAPH_TABLE queries where native parameter binding is unsupported."""
        if "GRAPH_TABLE" in statement and parameters:
            import re

            def _make_repl(r: str) -> Callable[[re.Match[str]], str]:
                return lambda _m: r

            stmt = statement
            for k in sorted(parameters.keys(), key=len, reverse=True):
                v = parameters[k]
                if isinstance(v, str):
                    escaped = v.replace("'", "''")
                    replacement = f"'{escaped}'"
                elif v is None:
                    replacement = "NULL"
                elif isinstance(v, bool):
                    replacement = "TRUE" if v else "FALSE"
                else:
                    replacement = str(v)
                pattern = r"\$" + re.escape(k) + r"\b"
                stmt = re.sub(pattern, _make_repl(replacement), stmt)
            return stmt, {}
        return statement, parameters

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
        stmt, params = self._format_pgq_statement(statement, parameters or {})
        rel = self.con.execute(stmt, params)
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
        stmt, params = self._format_pgq_statement(statement, parameters or {})
        if hasattr(self.con, "pl"):
            return self.con.execute(stmt, params).pl()
        records = self.execute(stmt, params)
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

        if isinstance(plan_or_statement, str):
            statement = plan_or_statement
            batch_list = batches or []
            total_batches = len(batch_list)
            for b in batch_list:
                batch_data = b.get("batch", [])
                total_records += len(batch_data)
                try:
                    self.con.execute(statement, b)
                except Exception:
                    self._fallback_bulk_ingest(statement, batch_data)
        else:
            statement = plan_or_statement.statement
            for batch_item in plan_or_statement:
                total_batches += 1
                batch_data = batch_item.parameters.get("batch", [])
                total_records += len(batch_data)
                try:
                    self.con.execute(statement, batch_item.parameters)
                except Exception:
                    self._fallback_bulk_ingest(statement, batch_data)

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def _fallback_bulk_ingest(self, statement: str, batch_data: list[dict[str, Any]]) -> None:
        """Fallback for relational graph tables in DuckDB when raw Cypher UNWIND is executed."""
        if not batch_data:
            return
        import re

        import pyarrow as pa

        match = re.search(r":([A-Za-z0-9_]+)", statement)
        table_name = match.group(1) if match else "entities"
        tbl = pa.Table.from_pylist(batch_data)
        self.con.register("_voyager_temp_batch", tbl)
        try:
            tables = [r[0] for r in self.con.execute("SHOW TABLES").fetchall()]
            if table_name not in tables:
                cols = tbl.column_names
                col_defs = []
                for col in cols:
                    if col == "id":
                        col_defs.append(f"{col} VARCHAR PRIMARY KEY")
                    else:
                        col_defs.append(f"{col} VARCHAR")
                self.con.execute(f"CREATE TABLE IF NOT EXISTS {table_name} ({', '.join(col_defs)})")

            self.con.execute(
                f"INSERT OR REPLACE INTO {table_name} BY NAME SELECT * FROM _voyager_temp_batch"
            )
        finally:
            self.con.unregister("_voyager_temp_batch")

    def ping(self) -> bool:
        """Pings DuckDB to verify connection liveness."""
        try:
            self.con.execute("SELECT 1")
            return True
        except Exception:
            return False

    def begin_transaction(self) -> DuckDbBridgeTransaction:
        """Begins an explicit DuckDB transaction on the bridge connection."""
        return DuckDbBridgeTransaction(self)

    def close(self) -> None:
        """Closes connection."""
        if hasattr(self.con, "close"):
            self.con.close()


class DuckDbBridgeTransaction:
    """Explicit DuckDB / DuckPGQ transaction holding one connection.

    DuckDB has no savepoints and no nested transactions: ``BEGIN`` inside an
    existing transaction fails. A failing statement aborts the whole DuckDB
    transaction, so this wrapper marks the transaction ``FAILED`` and rejects
    further statements until it is rolled back.
    """

    _savepoint_error_message = (
        "DuckDB transactions do not support savepoints; "
        "use Apache AGE/PostgreSQL for real savepoints"
    )

    def __init__(self, bridge: DuckDbBridge) -> None:
        """Starts an explicit transaction on the bridge's connection.

        Args:
            bridge: Parent DuckDB bridge.

        Raises:
            RuntimeError: If the connection is already inside a transaction.
        """
        self._bridge = bridge
        try:
            bridge.con.execute("BEGIN")
        except Exception as exc:
            raise RuntimeError(
                "cannot begin a DuckDB transaction: the connection may already be inside "
                f"a transaction (DuckDB has no nested transactions): {exc}"
            ) from exc
        self._state = "ACTIVE"

    @property
    def state(self) -> str:
        """Returns the lifecycle state ('ACTIVE', 'FAILED', 'COMMITTED', 'ROLLED_BACK')."""
        return self._state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        return self._state == "ACTIVE"

    @property
    def supports_savepoints(self) -> bool:
        """DuckDB has no savepoints."""
        return False

    def _mark_failed(self) -> None:
        """Marks the transaction aborted after a statement/driver error."""
        self._state = "FAILED"

    def _ensure_executable(self) -> None:
        """Rejects further statements once the transaction has failed."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back before executing more statements")
        if not self.is_active:
            raise ValueError("Cannot execute query on inactive transaction")

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a statement inside the transaction through the DuckPGQ bridge."""
        self._ensure_executable()
        try:
            return self._bridge.execute(statement, parameters)
        except BaseException:
            self._mark_failed()
            raise

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a statement through the DuckPGQ bridge and returns Polars."""
        self._ensure_executable()
        try:
            return self._bridge.execute_to_polars(statement, parameters)
        except BaseException:
            self._mark_failed()
            raise

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk plan inside the transaction."""
        self._ensure_executable()
        try:
            return self._bridge.execute_bulk(plan_or_statement, batches)
        except BaseException:
            self._mark_failed()
            raise

    def commit(self) -> None:
        """Commits the transaction."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back instead of committing")
        if not self.is_active:
            raise ValueError("Cannot commit inactive transaction")
        try:
            self._bridge.con.execute("COMMIT")
            self._state = "COMMITTED"
        except BaseException:
            self._mark_failed()
            raise

    def rollback(self) -> None:
        """Rolls the transaction back."""
        if self._state not in ("ACTIVE", "FAILED"):
            raise ValueError("Cannot rollback inactive transaction")
        try:
            self._bridge.con.execute("ROLLBACK")
            self._state = "ROLLED_BACK"
        except BaseException:
            self._mark_failed()
            raise

    def savepoint(self, name: str) -> None:
        """Raises because DuckDB does not support savepoints."""
        raise RuntimeError(self._savepoint_error_message)

    def rollback_to_savepoint(self, name: str) -> None:
        """Raises because DuckDB does not support savepoints."""
        raise RuntimeError(self._savepoint_error_message)

    def release_savepoint(self, name: str) -> None:
        """Raises because DuckDB does not support savepoints."""
        raise RuntimeError(self._savepoint_error_message)


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

    async def ping(self) -> bool:
        """Pings DuckDB asynchronously to verify connection liveness."""
        return await asyncio.to_thread(self.sync_bridge.ping)

    async def begin_transaction(self) -> AsyncDuckDbBridgeTransaction:
        """Begins an explicit DuckDB transaction asynchronously."""
        sync_tx = await asyncio.to_thread(self.sync_bridge.begin_transaction)
        return AsyncDuckDbBridgeTransaction(sync_tx)

    async def close(self) -> None:
        """Closes connection asynchronously."""
        await asyncio.to_thread(self.sync_bridge.close)


class AsyncDuckDbBridgeTransaction:
    """Asynchronous wrapper around a synchronous DuckDB transaction."""

    def __init__(self, transaction: DuckDbBridgeTransaction) -> None:
        """Initializes the async transaction wrapper."""
        self._sync = transaction

    @property
    def state(self) -> str:
        """Returns the lifecycle state."""
        return self._sync.state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        return self._sync.is_active

    @property
    def supports_savepoints(self) -> bool:
        """DuckDB has no savepoints."""
        return False

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a statement inside the transaction."""
        try:
            return await asyncio.to_thread(self._sync.execute, statement, parameters)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a statement and returns a Polars DataFrame."""
        try:
            return await asyncio.to_thread(self._sync.execute_to_polars, statement, parameters)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk plan inside the transaction."""
        try:
            return await asyncio.to_thread(self._sync.execute_bulk, plan_or_statement, batches)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def commit(self) -> None:
        """Commits the transaction."""
        try:
            await asyncio.to_thread(self._sync.commit)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def rollback(self) -> None:
        """Rolls the transaction back."""
        try:
            await asyncio.to_thread(self._sync.rollback)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def savepoint(self, name: str) -> None:
        """Raises because DuckDB does not support savepoints."""
        raise RuntimeError(self._sync._savepoint_error_message)

    async def rollback_to_savepoint(self, name: str) -> None:
        """Raises because DuckDB does not support savepoints."""
        raise RuntimeError(self._sync._savepoint_error_message)

    async def release_savepoint(self, name: str) -> None:
        """Raises because DuckDB does not support savepoints."""
        raise RuntimeError(self._sync._savepoint_error_message)


def mask_uri_password(uri: str) -> str:
    """Masks password credentials in a database URI for safe error messages and logging.

    Replaces password with '***' while retaining username, host, port, path, and query.
    For non-string or non-URI inputs, returns the input converted to string.
    """
    if not isinstance(uri, str) or "://" not in uri:
        return str(uri)
    try:
        scheme, rest = uri.split("://", 1)
        slash_idx = rest.find("/")
        query_idx = rest.find("?")
        limit = len(rest)
        if slash_idx != -1:
            limit = min(limit, slash_idx)
        if query_idx != -1:
            limit = min(limit, query_idx)

        authority = rest[:limit]
        if "@" in authority:
            userinfo, hostport = authority.rsplit("@", 1)
            if ":" in userinfo:
                user, _ = userinfo.split(":", 1)
                masked_authority = f"{user}:***@{hostport}"
            else:
                masked_authority = f"{userinfo}@{hostport}"
            return f"{scheme}://{masked_authority}{rest[limit:]}"
    except Exception:
        pass
    return uri


def setup_postgres_age(conn: Any, force: bool = False) -> None:
    """Configures a PostgreSQL connection for Apache AGE if required or installed.

    Executes `LOAD 'age'; SET search_path = ag_catalog, "$user", public;`.
    If `force` is True (e.g. for `age://` schemes), the setup runs unconditionally and raises on failure.
    If `force` is False (e.g. for `postgresql://` schemes or user-supplied connections), it first verifies
    whether the 'age' extension is installed via `SELECT 1 FROM pg_extension WHERE extname = 'age'`;
    if the extension is absent, plain PostgreSQL remains untouched.
    """
    if conn is None or not hasattr(conn, "cursor"):
        return
    if getattr(conn, "_voyager_age_initialized", False):
        return

    should_setup = force
    if not should_setup:
        try:
            with conn.cursor() as cur:
                if not hasattr(cur, "fetchone"):
                    return
                cur.execute("SELECT 1 FROM pg_extension WHERE extname = 'age'")
                row = cur.fetchone()
                if row:
                    should_setup = True
        except Exception:
            return

    if should_setup:
        with conn.cursor() as cur:
            cur.execute("LOAD 'age'; SET search_path = ag_catalog, \"$user\", public;")
        try:
            conn._voyager_age_initialized = True
        except Exception:
            pass


class PostgresBridge:
    """Synchronous PostgreSQL driver bridge (psycopg 3 / psycopg2) with zero-copy Polars output."""

    def __init__(self, connection: Any, graph_name: str | None = None) -> None:
        """Initializes PostgresBridge.

        Args:
            connection: PostgreSQL connection instance (e.g. psycopg.Connection).
            graph_name: Optional Apache AGE graph name to target.
        """
        if graph_name is not None:
            validate_graph_name(graph_name)
        self.conn = connection
        self._conn = connection
        self.graph_name = graph_name
        setup_postgres_age(self.conn, force=False)

    @property
    def connection(self) -> Any:
        """Returns underlying PostgreSQL connection instance."""
        return self.conn

    def _format_pgq_statement(
        self, statement: str, parameters: dict[str, Any]
    ) -> tuple[str, list[Any] | tuple[Any, ...]]:
        """Formats statement and parameters for PostgreSQL execution."""
        if not parameters:
            return statement, ()

        import re

        def _make_repl(r: str) -> Callable[[re.Match[str]], str]:
            return lambda _m: r

        if "cypher(" in statement:
            if not getattr(self.conn, "_voyager_age_initialized", False):
                setup_postgres_age(self.conn, force=True)
            import json

            return statement, (json.dumps(parameters),)

        if "GRAPH_TABLE" in statement:
            # PG19 GRAPH_TABLE does not support prepared parameters inside table function AST
            stmt = statement
            for k in sorted(parameters.keys(), key=len, reverse=True):
                v = parameters[k]
                if isinstance(v, str):
                    escaped = v.replace("'", "''")
                    replacement = f"'{escaped}'"
                elif v is None:
                    replacement = "NULL"
                elif isinstance(v, bool):
                    replacement = "TRUE" if v else "FALSE"
                else:
                    replacement = str(v)
                pattern = r"\$" + re.escape(k) + r"\b"
                stmt = re.sub(pattern, _make_repl(replacement), stmt)
            return stmt, ()
        else:
            ordered_params: list[Any] = []
            regex = re.compile(r"\$([a-zA-Z0-9_]+)\b")

            def repl(m: re.Match[str]) -> str:
                key = m.group(1)
                if key in parameters:
                    ordered_params.append(parameters[key])
                    return "%s"
                return m.group(0)

            stmt = regex.sub(repl, statement)
            return stmt, ordered_params

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes query on PostgreSQL.

        Args:
            statement: Query statement.
            parameters: Query parameters.

        Returns:
            List of record dictionaries.
        """
        stmt, params = self._format_pgq_statement(statement, parameters or {})
        with self.conn.cursor() as cur:
            cur.execute(stmt, params)
            if cur.description:
                cols = [d[0] for d in cur.description]
                rows = cur.fetchall()
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
        stmt, params = self._format_pgq_statement(statement, parameters or {})
        with self.conn.cursor() as cur:
            cur.execute(stmt, params)
            if cur.description:
                cols = [d[0] for d in cur.description]
                rows = cur.fetchall()
                return pl.DataFrame(rows, schema=cols, orient="row")
            return pl.DataFrame()

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes bulk ingestion plan in PostgreSQL.

        Args:
            plan_or_statement: BulkIngestionPlan or statement string.
            batches: Batch parameter sequence when a statement string is provided.

        Returns:
            BulkExecutionResult metrics.
        """
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0
        with self.conn.cursor() as cur:
            if isinstance(plan_or_statement, str):
                statement = plan_or_statement
                batch_list = batches or []
                total_batches = len(batch_list)
                for b in batch_list:
                    batch_data = b.get("batch", [])
                    total_records += len(batch_data)
                    stmt, params = self._format_pgq_statement(statement, b)
                    cur.execute(stmt, params)
            else:
                statement = plan_or_statement.statement
                for batch_item in plan_or_statement:
                    total_batches += 1
                    batch_data = batch_item.parameters.get("batch", [])
                    total_records += len(batch_data)
                    stmt, params = self._format_pgq_statement(statement, batch_item.parameters)
                    cur.execute(stmt, params)
        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def ping(self) -> bool:
        """Pings PostgreSQL to verify connection liveness."""
        try:
            with self.conn.cursor() as cur:
                cur.execute("SELECT 1")
            return True
        except Exception:
            return False

    def begin_transaction(self) -> PostgresBridgeTransaction:
        """Begins an explicit PostgreSQL/AGE transaction on the bridge connection."""
        return PostgresBridgeTransaction(self)

    def close(self) -> None:
        """Closes connection."""
        if hasattr(self.conn, "close"):
            self.conn.close()


class PostgresBridgeTransaction:
    """Explicit PostgreSQL / Apache AGE transaction holding one connection."""

    def __init__(self, bridge: PostgresBridge) -> None:
        """Starts a transaction on the bridge's connection.

        The AGE setup (`LOAD 'age'`, `SET search_path`) runs before `BEGIN`, so a
        rollback cannot undo it.

        Args:
            bridge: Parent PostgreSQL bridge.
        """
        self._bridge = bridge
        conn = bridge.conn
        setup_postgres_age(conn, force=False)

        self._original_autocommit = getattr(conn, "autocommit", None)
        if self._original_autocommit is False:
            # Commit any pending work (including AGE setup) and take explicit
            # control of BEGIN/COMMIT so psycopg does not start its own block.
            conn.commit()
            conn.autocommit = True

        try:
            self._execute_sql("BEGIN")
        except Exception:
            self._restore_autocommit()
            raise
        self._state = "ACTIVE"

    @property
    def state(self) -> str:
        """Returns the lifecycle state ('ACTIVE', 'FAILED', 'COMMITTED', 'ROLLED_BACK')."""
        return self._state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        if self._state != "ACTIVE":
            return False
        conn = self._bridge.conn
        if _pg_connection_closed(conn):
            return False
        status = _pg_transaction_status(conn)
        if status is None:
            return True
        return status in ("INTRANS", "ACTIVE")

    @property
    def supports_savepoints(self) -> bool:
        """PostgreSQL/AGE supports real savepoints."""
        return True

    def _mark_failed(self) -> None:
        """Marks the transaction aborted after a statement/driver error."""
        self._state = "FAILED"

    def _ensure_executable(self) -> None:
        """Rejects further statements once the transaction has failed."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back before executing more statements")
        if not self.is_active:
            raise ValueError("Cannot execute query on inactive transaction")

    def _restore_autocommit(self) -> None:
        """Restores the connection's original autocommit mode."""
        if self._original_autocommit is not None:
            try:
                self._bridge.conn.autocommit = self._original_autocommit
            except Exception:
                pass

    def _execute_sql(self, statement: str, parameters: Sequence[Any] = ()) -> str | None:
        """Executes a control statement and returns its command tag."""
        with self._bridge.conn.cursor() as cur:
            cur.execute(statement, parameters)
            return getattr(cur, "statusmessage", None)

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a statement inside the transaction."""
        self._ensure_executable()
        stmt, params = self._bridge._format_pgq_statement(statement, parameters or {})
        try:
            with self._bridge.conn.cursor() as cur:
                cur.execute(stmt, params)
                if cur.description:
                    cols = [d[0] for d in cur.description]
                    rows = cur.fetchall()
                    return [dict(zip(cols, row, strict=False)) for row in rows]
                return []
        except BaseException:
            self._mark_failed()
            raise

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a statement and returns a Polars DataFrame."""
        self._ensure_executable()
        stmt, params = self._bridge._format_pgq_statement(statement, parameters or {})
        try:
            with self._bridge.conn.cursor() as cur:
                cur.execute(stmt, params)
                if cur.description:
                    cols = [d[0] for d in cur.description]
                    rows = cur.fetchall()
                    return pl.DataFrame(rows, schema=cols, orient="row")
                return pl.DataFrame()
        except BaseException:
            self._mark_failed()
            raise

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk plan inside the transaction."""
        self._ensure_executable()
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0

        try:
            with self._bridge.conn.cursor() as cur:
                if isinstance(plan_or_statement, str):
                    statement = plan_or_statement
                    batch_list = batches or []
                    total_batches = len(batch_list)
                    for b in batch_list:
                        total_records += len(b.get("batch", []))
                        stmt, params = self._bridge._format_pgq_statement(statement, b)
                        cur.execute(stmt, params)
                else:
                    statement = plan_or_statement.statement
                    for batch_item in plan_or_statement:
                        total_batches += 1
                        total_records += len(batch_item.parameters.get("batch", []))
                        stmt, params = self._bridge._format_pgq_statement(
                            statement, batch_item.parameters
                        )
                        cur.execute(stmt, params)
        except BaseException:
            self._mark_failed()
            raise

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def commit(self) -> None:
        """Commits the transaction."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back instead of committing")
        if not self.is_active:
            raise ValueError("Cannot commit inactive transaction")
        try:
            status = _pg_transaction_status(self._bridge.conn)
            if status == "INERROR":
                raise RuntimeError("transaction is aborted; PostgreSQL is in a failed state")
            tag = self._execute_sql("COMMIT")
            if tag is not None and tag.upper().startswith("ROLLBACK"):
                raise RuntimeError(
                    "transaction is aborted; PostgreSQL converted COMMIT into ROLLBACK"
                )
            self._state = "COMMITTED"
        except BaseException:
            self._mark_failed()
            raise
        finally:
            self._restore_autocommit()

    def rollback(self) -> None:
        """Rolls the transaction back."""
        if self._state not in ("ACTIVE", "FAILED"):
            raise ValueError("Cannot rollback inactive transaction")
        try:
            self._execute_sql("ROLLBACK")
            self._state = "ROLLED_BACK"
        except BaseException:
            self._mark_failed()
            raise
        finally:
            self._restore_autocommit()

    def savepoint(self, name: str) -> None:
        """Creates a real SAVEPOINT."""
        self._ensure_executable()
        name = _validate_savepoint_name(name)
        self._execute_sql(f"SAVEPOINT {name}")

    def rollback_to_savepoint(self, name: str) -> None:
        """Rolls back to a real SAVEPOINT."""
        self._ensure_executable()
        name = _validate_savepoint_name(name)
        self._execute_sql(f"ROLLBACK TO SAVEPOINT {name}")

    def release_savepoint(self, name: str) -> None:
        """Releases a real SAVEPOINT."""
        self._ensure_executable()
        name = _validate_savepoint_name(name)
        self._execute_sql(f"RELEASE SAVEPOINT {name}")


class AsyncPostgresBridge:
    """Asynchronous PostgreSQL driver bridge using asyncio worker threads."""

    def __init__(self, connection: Any, graph_name: str | None = None) -> None:
        """Initializes AsyncPostgresBridge.

        Args:
            connection: PostgreSQL connection instance.
            graph_name: Optional Apache AGE graph name to target.
        """
        if graph_name is not None:
            validate_graph_name(graph_name)
        self.sync_bridge = PostgresBridge(connection, graph_name=graph_name)
        self.conn = connection
        self._conn = connection

    @property
    def graph_name(self) -> str | None:
        """The configured Apache AGE graph name, or None."""
        return self.sync_bridge.graph_name

    @graph_name.setter
    def graph_name(self, value: str | None) -> None:
        if value is not None:
            validate_graph_name(value)
        self.sync_bridge.graph_name = value

    @property
    def connection(self) -> Any:
        """Returns underlying PostgreSQL connection instance."""
        return self.conn

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Asynchronously executes query."""
        return await asyncio.to_thread(self.sync_bridge.execute, statement, parameters)

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Asynchronously streams query results to Polars DataFrame."""
        return await asyncio.to_thread(self.sync_bridge.execute_to_polars, statement, parameters)

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Asynchronously executes bulk ingestion plan."""
        return await asyncio.to_thread(self.sync_bridge.execute_bulk, plan_or_statement, batches)

    async def ping(self) -> bool:
        """Pings PostgreSQL asynchronously to verify connection liveness."""
        return await asyncio.to_thread(self.sync_bridge.ping)

    async def begin_transaction(self) -> AsyncPostgresBridgeTransaction:
        """Begins an explicit PostgreSQL/AGE transaction asynchronously."""
        sync_tx = await asyncio.to_thread(self.sync_bridge.begin_transaction)
        return AsyncPostgresBridgeTransaction(sync_tx)

    async def close(self) -> None:
        """Closes connection asynchronously."""
        await asyncio.to_thread(self.sync_bridge.close)


class AsyncPostgresBridgeTransaction:
    """Asynchronous wrapper around a synchronous PostgreSQL/AGE transaction."""

    def __init__(self, transaction: PostgresBridgeTransaction) -> None:
        """Initializes the async transaction wrapper."""
        self._sync = transaction

    @property
    def state(self) -> str:
        """Returns the lifecycle state."""
        return self._sync.state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        return self._sync.is_active

    @property
    def supports_savepoints(self) -> bool:
        """PostgreSQL/AGE supports real savepoints."""
        return True

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a statement inside the transaction."""
        try:
            return await asyncio.to_thread(self._sync.execute, statement, parameters)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a statement and returns a Polars DataFrame."""
        try:
            return await asyncio.to_thread(self._sync.execute_to_polars, statement, parameters)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk plan inside the transaction."""
        try:
            return await asyncio.to_thread(self._sync.execute_bulk, plan_or_statement, batches)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def commit(self) -> None:
        """Commits the transaction."""
        try:
            await asyncio.to_thread(self._sync.commit)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def rollback(self) -> None:
        """Rolls the transaction back."""
        try:
            await asyncio.to_thread(self._sync.rollback)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def savepoint(self, name: str) -> None:
        """Creates a real SAVEPOINT."""
        await asyncio.to_thread(self._sync.savepoint, name)

    async def rollback_to_savepoint(self, name: str) -> None:
        """Rolls back to a real SAVEPOINT."""
        await asyncio.to_thread(self._sync.rollback_to_savepoint, name)

    async def release_savepoint(self, name: str) -> None:
        """Releases a real SAVEPOINT."""
        await asyncio.to_thread(self._sync.release_savepoint, name)


def _adapt_falkordb_value(val: Any) -> Any:
    """Adapts rich parameter values (temporal types, Point, etc.) for FalkorDB query execution."""
    if isinstance(val, (datetime.date, datetime.time, datetime.datetime)):
        return val.isoformat()
    if isinstance(val, datetime.timedelta):
        from voyager_ogm.types import to_iso_duration

        return to_iso_duration(val)
    if isinstance(val, dict):
        if "__voyager_spatial__" in val:
            sp = val["__voyager_spatial__"]
            unwrapped = unwrap_spatial_param(sp) if isinstance(sp, dict) else sp
            return json.dumps(unwrapped)
        return {k: _adapt_falkordb_value(v) for k, v in val.items()}
    if isinstance(val, (list, tuple)):
        return [_adapt_falkordb_value(x) for x in val]
    return val


def _adapt_falkordb_parameters(params: dict[str, Any]) -> dict[str, Any]:
    """Adapts query parameters for FalkorDB execution."""
    return {k: _adapt_falkordb_value(v) for k, v in params.items()}


class FalkorDBBridge:
    """FalkorDB driver bridge for graph execution."""

    def __init__(self, connection: Any, graph_name: str = "voyager") -> None:
        """Initializes FalkorDBBridge.

        Args:
            connection: FalkorDB client or Graph instance.
            graph_name: Target graph key name if a FalkorDB instance is provided.
        """
        if hasattr(connection, "select_graph"):
            self.client = connection
            self.graph = connection.select_graph(graph_name)
        elif hasattr(connection, "query"):
            self.graph = connection
            self.client = getattr(connection, "falkordb", None)
        else:
            self.graph = connection
            self.client = None

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a Cypher statement on FalkorDB.

        Args:
            statement: Cypher statement.
            parameters: Query parameters.

        Returns:
            List of record dictionaries.
        """
        params = _adapt_falkordb_parameters(parameters or {})
        try:
            res = self.graph.query(statement, params)
        except Exception as e:
            stmt_lower = statement.strip().lower()
            is_index_ddl = stmt_lower.startswith("create index") or stmt_lower.startswith(
                "drop index"
            )
            err_msg = str(e).lower()
            if is_index_ddl and ("already indexed" in err_msg or "no such index" in err_msg):
                return []
            raise

        if (
            not hasattr(res, "header")
            or not res.header
            or not hasattr(res, "result_set")
            or not res.result_set
        ):
            return []

        col_names = [
            h[1] if isinstance(h, (list, tuple)) and len(h) > 1 else str(h) for h in res.header
        ]
        records: list[dict[str, Any]] = []
        for row in res.result_set:
            rec: dict[str, Any] = {}
            for col, val in zip(col_names, row, strict=False):
                if hasattr(val, "properties"):
                    rec[col] = val.properties
                else:
                    rec[col] = val
            records.append(rec)
        return records

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
        """Executes bulk batch ingestion on FalkorDB.

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

        if isinstance(plan_or_statement, str):
            statement = plan_or_statement
            batch_list = batches or []
            total_batches = len(batch_list)
            for b in batch_list:
                batch_data = b.get("batch", [])
                total_records += len(batch_data)
                self.graph.query(statement, _adapt_falkordb_parameters(b))
        else:
            statement = plan_or_statement.statement
            for batch_item in plan_or_statement:
                total_batches += 1
                batch_data = batch_item.parameters.get("batch", [])
                total_records += len(batch_data)
                self.graph.query(statement, _adapt_falkordb_parameters(batch_item.parameters))

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def begin_transaction(self) -> Any:
        """Raises because FalkorDB/Redis cannot provide transactional rollback."""
        raise RuntimeError(
            "FalkorDB does not support transactions: Redis MULTI/EXEC cannot provide "
            "rollback or read-your-writes"
        )

    def ping(self) -> bool:
        """Pings FalkorDB to verify connection liveness."""
        try:
            self.graph.query("RETURN 1")
            return True
        except Exception:
            return False

    def close(self) -> None:
        """Closes FalkorDB connection."""
        if self.client and hasattr(self.client, "close"):
            try:
                self.client.close()
            except Exception:
                pass
        elif hasattr(self.graph, "close"):
            try:
                self.graph.close()
            except Exception:
                pass


class AsyncFalkorDBBridge:
    """Asynchronous FalkorDB driver bridge using asyncio worker threads."""

    def __init__(self, connection: Any, graph_name: str = "voyager") -> None:
        """Initializes AsyncFalkorDBBridge.

        Args:
            connection: FalkorDB client or Graph instance.
            graph_name: Target graph key name if a FalkorDB instance is provided.
        """
        self.sync_bridge = FalkorDBBridge(connection, graph_name=graph_name)

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Asynchronously executes query."""
        return await asyncio.to_thread(self.sync_bridge.execute, statement, parameters)

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Asynchronously streams query results to Polars DataFrame."""
        return await asyncio.to_thread(self.sync_bridge.execute_to_polars, statement, parameters)

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Asynchronously executes bulk ingestion plan."""
        return await asyncio.to_thread(self.sync_bridge.execute_bulk, plan_or_statement, batches)

    async def begin_transaction(self) -> Any:
        """Raises because FalkorDB/Redis cannot provide transactional rollback."""
        raise RuntimeError(
            "FalkorDB does not support transactions: Redis MULTI/EXEC cannot provide "
            "rollback or read-your-writes"
        )

    async def ping(self) -> bool:
        """Pings FalkorDB asynchronously to verify connection liveness."""
        return await asyncio.to_thread(self.sync_bridge.ping)

    async def close(self) -> None:
        """Closes FalkorDB connection asynchronously."""
        await asyncio.to_thread(self.sync_bridge.close)


def _normalize_relate_value(val: Any) -> Any:
    """Normalizes Relate native objects (Node, Relationship, Point, Duration) to standard Python dicts and types."""
    if hasattr(val, "properties") and hasattr(val, "labels"):
        d = dict(val.properties)
        d.setdefault("id", val.id)
        d.setdefault("labels", list(val.labels))
        d.setdefault("element_id", val.element_id)
        return d
    elif hasattr(val, "type") and hasattr(val, "start_node"):
        d = dict(val.properties)
        d.setdefault("type", val.type)
        d.setdefault("id", val.id)
        d.setdefault("start", val.start_node)
        d.setdefault("end", val.end_node)
        d.setdefault("element_id", val.element_id)
        return d
    elif hasattr(val, "srid") and hasattr(val, "x") and hasattr(val, "y"):
        from voyager_ogm.types import Point

        return Point(
            x=val.x,
            y=val.y,
            z=getattr(val, "z", None),
            srid=val.srid,
        )
    elif hasattr(val, "months") and hasattr(val, "seconds") and hasattr(val, "nanoseconds"):
        try:
            return val.to_timedelta()
        except Exception:
            return str(val)
    elif isinstance(val, dict):
        return {k: _normalize_relate_value(v) for k, v in val.items()}
    elif isinstance(val, (list, tuple)):
        return [_normalize_relate_value(x) for x in val]
    return val


def _normalize_relate_record(record: Any) -> dict[str, Any]:
    """Normalizes a single record returned by Relate Cursor or QueryResult."""
    if isinstance(record, dict):
        return {k: _normalize_relate_value(v) for k, v in record.items()}
    if hasattr(record, "items"):
        return {k: _normalize_relate_value(v) for k, v in record.items()}
    return record


def _adapt_relate_value(val: Any) -> Any:
    """Adapts Voyager spatial types and nested structures to Relate-compatible values."""
    from voyager_ogm.types import Point

    if isinstance(val, Point):
        try:
            import relate

            if val.is_3d:
                z_raw = val.z if val.z is not None else val.height
                if z_raw is not None:
                    z_val = float(z_raw)
                    return relate.Point(val.srid, float(val.x), float(val.y), z_val)
            return relate.Point(val.srid, float(val.x), float(val.y))
        except (ImportError, Exception):
            return {"srid": val.srid, "x": float(val.x), "y": float(val.y)}

    if isinstance(val, dict):
        if "__voyager_spatial__" in val:
            sp = val["__voyager_spatial__"]
            if isinstance(sp, dict):
                srid = int(sp.get("srid", 4326))
                x_raw = sp.get("longitude")
                if x_raw is None:
                    x_raw = sp.get("x", 0.0)
                y_raw = sp.get("latitude")
                if y_raw is None:
                    y_raw = sp.get("y", 0.0)
                x = float(x_raw if x_raw is not None else 0.0)
                y = float(y_raw if y_raw is not None else 0.0)
                z = sp.get("height")
                if z is None:
                    z = sp.get("z")
                try:
                    import relate

                    if z is not None:
                        return relate.Point(srid, x, y, float(z))
                    return relate.Point(srid, x, y)
                except (ImportError, Exception):
                    return {"srid": srid, "x": x, "y": y}
        return {k: _adapt_relate_value(v) for k, v in val.items()}

    if isinstance(val, (list, tuple)):
        return [_adapt_relate_value(x) for x in val]

    return val


def _adapt_relate_parameters(params: dict[str, Any]) -> dict[str, Any]:
    return {k: _adapt_relate_value(v) for k, v in params.items()}


class RelateBridge:
    """Synchronous Relate embedded graph database driver bridge (https://relatedb.ai).

    Connects in-process to Relate databases (via relate-db), enabling zero-network,
    in-memory or single-file (.rl) property graph execution with openCypher / Cypher 25
    and zero-copy Apache Arrow / Polars streaming.
    """

    def __init__(self, connection: Any) -> None:
        """Initializes RelateBridge.

        Args:
            connection: A relate.Connection or relate.Database instance.
        """
        self.con = connection

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a Cypher statement against the embedded Relate database.

        Args:
            statement: Cypher statement string.
            parameters: Query parameters dictionary.

        Returns:
            List of normalized record dictionaries.
        """
        params = _adapt_relate_parameters(parameters or {})
        res = self.con.execute(statement, params)
        if hasattr(res, "fetchall"):
            rows = res.fetchall()
        elif hasattr(res, "data"):
            rows = res.data
        else:
            rows = list(res)
        return [_normalize_relate_record(row) for row in rows]

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a Cypher statement and streams results directly into a Polars DataFrame via zero-copy Arrow IPC.

        Args:
            statement: Cypher statement.
            parameters: Query parameters.

        Returns:
            Polars DataFrame containing result records.
        """
        params = _adapt_relate_parameters(parameters or {})
        if hasattr(self.con, "execute"):
            res = self.con.execute(statement, params)
            if hasattr(res, "to_arrow"):
                try:
                    tbl = res.to_arrow()
                    df_out = pl.from_arrow(tbl)
                    if isinstance(df_out, pl.DataFrame):
                        return df_out
                    return pl.DataFrame(df_out)
                except Exception:
                    pass
            records = getattr(res, "fetchall", lambda: getattr(res, "data", []))()
            normalized = [_normalize_relate_record(r) for r in records]
            return pl.DataFrame(normalized) if normalized else pl.DataFrame()
        return pl.DataFrame()

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes bulk batch ingestion against the embedded Relate engine.

        Args:
            plan_or_statement: BulkIngestionPlan or Cypher statement.
            batches: Batch parameter sequence.

        Returns:
            BulkExecutionResult summary metrics.
        """
        start_time = time.perf_counter()
        statement = ""
        total_records = 0
        total_batches = 0

        if isinstance(plan_or_statement, str):
            statement = plan_or_statement
            batch_list = batches or []
            total_batches = len(batch_list)
            for b in batch_list:
                batch_data = b.get("batch", [])
                total_records += len(batch_data)
                self.con.execute(statement, _adapt_relate_parameters(b))
        else:
            statement = plan_or_statement.statement
            for batch_item in plan_or_statement:
                total_batches += 1
                batch_data = batch_item.parameters.get("batch", [])
                total_records += len(batch_data)
                self.con.execute(statement, _adapt_relate_parameters(batch_item.parameters))

        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def ping(self) -> bool:
        """Pings Relate to verify liveness."""
        try:
            self.con.execute("RETURN 1")
            return True
        except Exception:
            return False

    def begin_transaction(self) -> RelateBridgeTransaction:
        """Begins an explicit Relate transaction on the bridge connection."""
        return RelateBridgeTransaction(self)

    def close(self) -> None:
        """Closes the underlying Relate database connection."""
        if hasattr(self.con, "close"):
            self.con.close()


class RelateBridgeTransaction:
    """Explicit Relate transaction driven by the connection's autocommit flag.

    Relate is DB-API based: setting ``autocommit = False`` groups statements
    into a transaction that ``commit()`` / ``rollback()`` ends. The original
    autocommit value is restored on every completion path. A failing statement
    leaves the engine transaction in a failed state, so further statements are
    rejected until rollback.
    """

    _savepoint_error_message = (
        "Relate transactions do not support savepoints; "
        "use Apache AGE/PostgreSQL for real savepoints"
    )

    def __init__(self, bridge: RelateBridge) -> None:
        """Starts an explicit transaction on the bridge's connection.

        Args:
            bridge: Parent Relate bridge.

        Raises:
            RuntimeError: If the connection already has autocommit disabled and
                an open transaction with pending work. That work belongs to the
                caller, so it is never silently committed or discarded.
        """
        self._bridge = bridge
        conn = bridge.con
        self._original_autocommit = getattr(conn, "autocommit", True)
        if self._original_autocommit is False:
            if self._transaction_open(conn):
                raise RuntimeError(
                    "cannot begin a Relate transaction: the connection already has an "
                    "explicit transaction with pending work; commit or roll it back first"
                )
        else:
            try:
                conn.autocommit = False
            except Exception as exc:
                self._restore_autocommit()
                raise RuntimeError(f"cannot begin a Relate transaction: {exc}") from exc
        self._state = "ACTIVE"

    @property
    def state(self) -> str:
        """Returns the lifecycle state ('ACTIVE', 'FAILED', 'COMMITTED', 'ROLLED_BACK')."""
        return self._state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        return self._state == "ACTIVE"

    @property
    def supports_savepoints(self) -> bool:
        """Relate has no savepoints."""
        return False

    @staticmethod
    def _transaction_open(conn: Any) -> bool:
        """Returns whether the Relate connection has an open transaction."""
        probe = getattr(conn, "_transaction_open", None)
        if callable(probe):
            try:
                return bool(probe())
            except Exception:
                return True
        # Without a public probe, assume pending work rather than risk it.
        return True

    def _restore_autocommit(self) -> None:
        """Restores the connection's original autocommit mode (best effort)."""
        original = self._original_autocommit
        if original is None:
            return
        conn = self._bridge.con
        try:
            if bool(getattr(conn, "autocommit", original)) == bool(original):
                return
            if original and self._transaction_open(conn):
                # Relate's autocommit setter commits; end any failed/open
                # transaction first so the setter cannot raise or commit it.
                try:
                    conn.rollback()
                except Exception:
                    pass
            conn.autocommit = original
        except Exception:
            try:
                if original and self._transaction_open(conn):
                    conn.rollback()
            except Exception:
                pass
            try:
                conn.autocommit = original
            except Exception:
                pass

    def _mark_failed(self) -> None:
        """Marks the transaction aborted after a statement/driver error."""
        self._state = "FAILED"

    def _ensure_executable(self) -> None:
        """Rejects further statements once the transaction has failed."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back before executing more statements")
        if not self.is_active:
            raise ValueError("Cannot execute query on inactive transaction")

    def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a statement inside the transaction."""
        self._ensure_executable()
        try:
            return self._bridge.execute(statement, parameters)
        except BaseException:
            self._mark_failed()
            raise

    def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a statement and returns a Polars DataFrame."""
        self._ensure_executable()
        try:
            return self._bridge.execute_to_polars(statement, parameters)
        except BaseException:
            self._mark_failed()
            raise

    def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk plan inside the transaction."""
        self._ensure_executable()
        try:
            return self._bridge.execute_bulk(plan_or_statement, batches)
        except BaseException:
            self._mark_failed()
            raise

    def commit(self) -> None:
        """Commits the transaction."""
        if self._state == "FAILED":
            raise RuntimeError("transaction is aborted; roll back instead of committing")
        if not self.is_active:
            raise ValueError("Cannot commit inactive transaction")
        try:
            self._bridge.con.commit()
            self._state = "COMMITTED"
        except BaseException:
            self._mark_failed()
            raise
        finally:
            self._restore_autocommit()

    def rollback(self) -> None:
        """Rolls the transaction back."""
        if self._state not in ("ACTIVE", "FAILED"):
            raise ValueError("Cannot rollback inactive transaction")
        try:
            self._bridge.con.rollback()
            self._state = "ROLLED_BACK"
        except BaseException:
            self._mark_failed()
            raise
        finally:
            self._restore_autocommit()

    def savepoint(self, name: str) -> None:
        """Raises because Relate does not support savepoints."""
        raise RuntimeError(self._savepoint_error_message)

    def rollback_to_savepoint(self, name: str) -> None:
        """Raises because Relate does not support savepoints."""
        raise RuntimeError(self._savepoint_error_message)

    def release_savepoint(self, name: str) -> None:
        """Raises because Relate does not support savepoints."""
        raise RuntimeError(self._savepoint_error_message)


class AsyncRelateBridge:
    """Asynchronous Relate driver bridge.

    Because embedded Relate connections are thread-affine (DB-API threadsafety = 1),
    queries are executed cooperatively on the active event loop thread without
    violating thread-affinity boundaries.
    """

    def __init__(self, connection: Any) -> None:
        """Initializes AsyncRelateBridge.

        Args:
            connection: Relate connection or database instance.
        """
        self.sync_bridge = RelateBridge(connection)

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Asynchronously executes query on Relate."""
        await asyncio.sleep(0)
        return self.sync_bridge.execute(statement, parameters)

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Asynchronously streams query results to Polars DataFrame."""
        await asyncio.sleep(0)
        return self.sync_bridge.execute_to_polars(statement, parameters)

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Asynchronously executes bulk ingestion plan."""
        await asyncio.sleep(0)
        return self.sync_bridge.execute_bulk(plan_or_statement, batches)

    async def ping(self) -> bool:
        """Pings Relate asynchronously to verify connection liveness."""
        await asyncio.sleep(0)
        return self.sync_bridge.ping()

    async def begin_transaction(self) -> AsyncRelateBridgeTransaction:
        """Begins an explicit Relate transaction on the event-loop thread."""
        await asyncio.sleep(0)
        return AsyncRelateBridgeTransaction(self.sync_bridge.begin_transaction())

    async def close(self) -> None:
        """Closes Relate connection asynchronously."""
        await asyncio.sleep(0)
        self.sync_bridge.close()


class AsyncRelateBridgeTransaction:
    """Asynchronous wrapper around a synchronous Relate transaction.

    Relate connections are thread-affine, so every call stays on the event-loop
    thread, matching :class:`AsyncRelateBridge`.
    """

    def __init__(self, transaction: RelateBridgeTransaction) -> None:
        """Initializes the async transaction wrapper."""
        self._sync = transaction

    @property
    def state(self) -> str:
        """Returns the lifecycle state."""
        return self._sync.state

    @property
    def is_active(self) -> bool:
        """Returns whether the transaction accepts statements."""
        return self._sync.is_active

    @property
    def supports_savepoints(self) -> bool:
        """Relate has no savepoints."""
        return False

    async def execute(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> list[dict[str, Any]]:
        """Executes a statement inside the transaction."""
        await asyncio.sleep(0)
        try:
            return self._sync.execute(statement, parameters)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def execute_to_polars(
        self, statement: str, parameters: dict[str, Any] | None = None
    ) -> pl.DataFrame:
        """Executes a statement and returns a Polars DataFrame."""
        await asyncio.sleep(0)
        try:
            return self._sync.execute_to_polars(statement, parameters)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def execute_bulk(
        self,
        plan_or_statement: BulkIngestionPlan | str,
        batches: Sequence[dict[str, Any]] | None = None,
    ) -> BulkExecutionResult:
        """Executes a bulk plan inside the transaction."""
        await asyncio.sleep(0)
        try:
            return self._sync.execute_bulk(plan_or_statement, batches)
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def commit(self) -> None:
        """Commits the transaction."""
        await asyncio.sleep(0)
        try:
            self._sync.commit()
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def rollback(self) -> None:
        """Rolls the transaction back."""
        await asyncio.sleep(0)
        try:
            self._sync.rollback()
        except asyncio.CancelledError:
            self._sync._mark_failed()
            raise

    async def savepoint(self, name: str) -> None:
        """Raises because Relate does not support savepoints."""
        raise RuntimeError(self._sync._savepoint_error_message)

    async def rollback_to_savepoint(self, name: str) -> None:
        """Raises because Relate does not support savepoints."""
        raise RuntimeError(self._sync._savepoint_error_message)

    async def release_savepoint(self, name: str) -> None:
        """Raises because Relate does not support savepoints."""
        raise RuntimeError(self._sync._savepoint_error_message)


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
    matcher: Callable[[Any], bool]
    if isinstance(predicate_or_type, type):
        target_cls = predicate_or_type

        def _matcher(obj: Any) -> bool:
            return isinstance(obj, target_cls)

        matcher = _matcher
    else:
        matcher = predicate_or_type

    _BRIDGE_REGISTRY.insert(0, (matcher, bridge_class, is_async))


def _is_neo4j_sync_driver(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    return "neo4j" in type_name and "Async" not in type_name and hasattr(obj, "session")


def _is_neo4j_async_driver(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    return "neo4j" in type_name and "Async" in type_name and hasattr(obj, "session")


def _is_duckdb_conn(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    return "duckdb" in type_name and hasattr(obj, "execute")


def _is_postgres_conn(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    return "psycopg" in type_name and hasattr(obj, "cursor")


def _is_falkordb_conn(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    return "falkordb" in type_name and (hasattr(obj, "query") or hasattr(obj, "select_graph"))


def _is_relate_conn(obj: Any) -> bool:
    type_name = f"{type(obj).__module__}.{type(obj).__qualname__}"
    if "relate" in type_name and hasattr(obj, "execute"):
        return True
    if hasattr(obj, "execute_arrow") and hasattr(obj, "begin_transaction"):
        return True
    return False


register_bridge(_is_neo4j_sync_driver, Neo4jBoltBridge, is_async=False)
register_bridge(_is_neo4j_async_driver, AsyncNeo4jBoltBridge, is_async=True)
register_bridge(_is_duckdb_conn, DuckDbBridge, is_async=False)
register_bridge(_is_duckdb_conn, AsyncDuckDbBridge, is_async=True)
register_bridge(_is_postgres_conn, PostgresBridge, is_async=False)
register_bridge(_is_postgres_conn, AsyncPostgresBridge, is_async=True)
register_bridge(_is_falkordb_conn, FalkorDBBridge, is_async=False)
register_bridge(_is_falkordb_conn, AsyncFalkorDBBridge, is_async=True)
register_bridge(_is_relate_conn, RelateBridge, is_async=False)
register_bridge(_is_relate_conn, AsyncRelateBridge, is_async=True)


@overload
def create_bridge(
    driver_or_connection: Any = None,
    is_async: Literal[False] = False,
    fallback: str | None = None,
    *,
    auth: tuple[str, str] | Any | None = None,
    graph_name: str | None = None,
) -> DatabaseBridge: ...


@overload
def create_bridge(
    driver_or_connection: Any = None,
    is_async: Literal[True] = True,
    fallback: str | None = None,
    *,
    auth: tuple[str, str] | Any | None = None,
    graph_name: str | None = None,
) -> AsyncDatabaseBridge: ...


@overload
def create_bridge(
    driver_or_connection: Any = None,
    is_async: bool = False,
    fallback: str | None = None,
    *,
    auth: tuple[str, str] | Any | None = None,
    graph_name: str | None = None,
) -> DatabaseBridge | AsyncDatabaseBridge: ...


def create_bridge(
    driver_or_connection: Any = None,
    is_async: bool = False,
    fallback: str | None = None,
    *,
    auth: tuple[str, str] | Any | None = None,
    graph_name: str | None = None,
) -> DatabaseBridge | AsyncDatabaseBridge:
    """Creates or adapts a database bridge from a user-supplied driver, connection, or URI.

    Args:
        driver_or_connection: Database driver, connection instance, MockBridge, or custom bridge.
        is_async: Whether an async bridge is required.
        fallback: Fallback strategy when driver import fails or URI scheme is unrecognized.
            Pass ``fallback="mock"`` to explicitly opt into returning an in-memory MockBridge
            for testing, mocking, or demonstrations.
        auth: Explicit authentication credentials. For Bolt schemes, accepts a (username, password)
            tuple or a neo4j auth token (e.g. bearer_auth). For PostgreSQL/AGE schemes, accepts
            a (username, password) tuple passed as connect kwargs. For FalkorDB/Redis schemes,
            accepts a (username, password) tuple. Explicit auth overrides credentials present
            in the URI.
        graph_name: Optional Apache AGE graph name to target when connecting to Apache AGE
            or PostgreSQL. If specified, validated as a safe identifier.

    Returns:
        Configured database bridge adapter.

    Raises:
        ValueError: If no driver/URI was provided, or if the URI scheme is unrecognized and
            fallback="mock" was not explicitly requested, or if graph_name is invalid.
        ImportError: If the official driver package required for the URI scheme is not installed
            and fallback="mock" was not explicitly requested.
        TypeError: If the driver_or_connection object type is unsupported, or if an invalid
            auth type was provided for the scheme.
    """
    if graph_name is not None:
        validate_graph_name(graph_name)

    if driver_or_connection is None:
        if fallback == "mock":
            return (
                AsyncMockBridge(graph_name=graph_name)
                if is_async
                else MockBridge(graph_name=graph_name)
            )
        raise ValueError(
            "No database driver, connection, or URI provided. "
            "Provide a valid driver instance, connection URI, or pass fallback='mock' (or use a 'mock://' URI) for offline testing."
        )

    if isinstance(driver_or_connection, MockBridge):
        if graph_name is not None:
            driver_or_connection.graph_name = graph_name
        return (
            AsyncMockBridge(driver_or_connection, graph_name=driver_or_connection.graph_name)
            if is_async
            else driver_or_connection
        )
    if isinstance(driver_or_connection, AsyncMockBridge):
        if graph_name is not None:
            driver_or_connection.graph_name = graph_name
        return driver_or_connection if is_async else driver_or_connection.sync_mock

    if (
        is_async
        and isinstance(driver_or_connection, AsyncDatabaseBridge)
        and inspect.iscoroutinefunction(getattr(driver_or_connection, "execute", None))
    ):
        if graph_name is not None and hasattr(driver_or_connection, "graph_name"):
            driver_or_connection.graph_name = graph_name
        return driver_or_connection
    if (
        not is_async
        and isinstance(driver_or_connection, DatabaseBridge)
        and not inspect.iscoroutinefunction(getattr(driver_or_connection, "execute", None))
    ):
        if graph_name is not None and hasattr(driver_or_connection, "graph_name"):
            driver_or_connection.graph_name = graph_name
        return driver_or_connection

    for matcher, bridge_cls, reg_is_async in _BRIDGE_REGISTRY:
        if reg_is_async == is_async and matcher(driver_or_connection):
            bridge_inst = bridge_cls(driver_or_connection)
            if graph_name is not None and hasattr(bridge_inst, "graph_name"):
                bridge_inst.graph_name = graph_name
            return bridge_inst

    if is_async:
        for matcher, bridge_cls, reg_is_async in _BRIDGE_REGISTRY:
            if not reg_is_async and matcher(driver_or_connection):
                sync_inst = bridge_cls(driver_or_connection)
                if isinstance(sync_inst, DuckDbBridge):
                    return AsyncDuckDbBridge(driver_or_connection)
                elif isinstance(sync_inst, FalkorDBBridge):
                    return AsyncFalkorDBBridge(driver_or_connection)
                elif isinstance(sync_inst, RelateBridge):
                    return AsyncRelateBridge(driver_or_connection)
                elif isinstance(sync_inst, PostgresBridge):
                    async_inst = AsyncPostgresBridge(driver_or_connection)
                    if graph_name is not None:
                        async_inst.graph_name = graph_name
                    return async_inst

    # If a database connection URI string was provided, attempt to auto-instantiate the official driver
    if isinstance(driver_or_connection, str):
        if driver_or_connection.startswith("mock://"):
            from urllib.parse import parse_qs, urlsplit

            parsed_mock = urlsplit(driver_or_connection)
            qs_mock = parse_qs(parsed_mock.query)
            uri_mock_graph = qs_mock.get("graph", qs_mock.get("graph_name", [None]))[0]
            resolved_mock_graph = graph_name if graph_name is not None else uri_mock_graph
            if resolved_mock_graph is not None:
                validate_graph_name(resolved_mock_graph)
            return (
                AsyncMockBridge(graph_name=resolved_mock_graph)
                if is_async
                else MockBridge(graph_name=resolved_mock_graph)
            )

        if "://" not in driver_or_connection:
            if fallback == "mock":
                return AsyncMockBridge() if is_async else MockBridge()
            raise ValueError(
                f"Invalid connection URI: '{mask_uri_password(driver_or_connection)}'. Expected a valid URI scheme (e.g. 'bolt://', 'duckdb://', 'postgresql://', 'falkordb://', or 'mock://')."
            )

        scheme = driver_or_connection.split("://", 1)[0].lower()
        if scheme in (
            "bolt",
            "neo4j",
            "memgraph",
            "bolt+s",
            "neo4j+s",
            "memgraph+s",
            "bolt+ssc",
            "neo4j+ssc",
            "memgraph+ssc",
        ):
            try:
                import neo4j
            except ImportError as e:
                if fallback == "mock":
                    return AsyncMockBridge() if is_async else MockBridge()
                raise ImportError(
                    f"The 'neo4j' Python package is required to connect to URI '{mask_uri_password(driver_or_connection)}'. "
                    f"Install it with 'pip install neo4j', or pass fallback='mock' for offline testing."
                ) from e
            from urllib.parse import parse_qs, unquote, urlsplit, urlunsplit

            parsed_bolt = urlsplit(driver_or_connection)
            netloc = parsed_bolt.netloc
            hostport = netloc.rsplit("@", 1)[-1]
            bolt_clean_uri = urlunsplit(
                (
                    parsed_bolt.scheme,
                    hostport,
                    parsed_bolt.path,
                    "",
                    "",
                )
            )
            bolt_kwargs: dict[str, Any] = {}
            if parsed_bolt.query:
                qs = parse_qs(parsed_bolt.query)
                if "connect_timeout" in qs:
                    try:
                        bolt_kwargs["connection_timeout"] = float(qs["connect_timeout"][0])
                    except ValueError:
                        pass
                if "connection_timeout" in qs:
                    try:
                        bolt_kwargs["connection_timeout"] = float(qs["connection_timeout"][0])
                    except ValueError:
                        pass

            effective_auth = None
            if auth is not None:
                effective_auth = auth
            elif "@" in netloc:
                userinfo = netloc.rsplit("@", 1)[0]
                if ":" in userinfo:
                    u_raw, p_raw = userinfo.split(":", 1)
                    effective_auth = (unquote(u_raw), unquote(p_raw))
                else:
                    effective_auth = (unquote(userinfo), "")

            if effective_auth is not None:
                bolt_kwargs["auth"] = effective_auth

            if is_async and hasattr(neo4j, "AsyncGraphDatabase"):
                async_drv = neo4j.AsyncGraphDatabase.driver(bolt_clean_uri, **bolt_kwargs)
                return AsyncNeo4jBoltBridge(async_drv)
            elif hasattr(neo4j, "GraphDatabase"):
                sync_drv = neo4j.GraphDatabase.driver(bolt_clean_uri, **bolt_kwargs)
                return Neo4jBoltBridge(sync_drv)
            else:
                if fallback == "mock":
                    return AsyncMockBridge() if is_async else MockBridge()
                raise RuntimeError(
                    f"Unable to instantiate neo4j driver from '{mask_uri_password(driver_or_connection)}'."
                )
        elif scheme == "duckdb":
            if auth is not None:
                raise TypeError("DuckDB does not accept auth credentials")
            try:
                import duckdb
            except ImportError as e:
                if fallback == "mock":
                    return AsyncMockBridge() if is_async else MockBridge()
                raise ImportError(
                    f"The 'duckdb' Python package is required to connect to URI '{mask_uri_password(driver_or_connection)}'. "
                    f"Install it with 'pip install duckdb', or pass fallback='mock' for offline testing."
                ) from e
            path = driver_or_connection.replace("duckdb://", "") or ":memory:"
            duck_conn = duckdb.connect(path)
            duck_bridge = DuckDbBridge(duck_conn)
            return AsyncDuckDbBridge(duck_conn) if is_async else duck_bridge
        elif scheme in (
            "postgresql",
            "postgres",
            "age",
            "postgresql+s",
            "postgres+s",
            "age+s",
            "postgresql+ssc",
            "postgres+ssc",
            "age+ssc",
        ):
            try:
                import psycopg
            except ImportError as e:
                if fallback == "mock":
                    return AsyncMockBridge() if is_async else MockBridge()
                raise ImportError(
                    f"The 'psycopg' Python package is required to connect to URI '{mask_uri_password(driver_or_connection)}'. "
                    f"Install it with 'pip install psycopg[binary]', or pass fallback='mock' for offline testing."
                ) from e
            from urllib.parse import parse_qs, urlsplit, urlunsplit

            is_age_scheme = scheme in ("age", "age+s", "age+ssc")
            is_ssl_scheme = scheme in (
                "postgresql+s",
                "postgres+s",
                "age+s",
                "postgresql+ssc",
                "postgres+ssc",
                "age+ssc",
            )

            parsed_pg = urlsplit(driver_or_connection)
            target_scheme = "postgresql" if (is_age_scheme or is_ssl_scheme) else parsed_pg.scheme
            new_query = parsed_pg.query
            qs = parse_qs(parsed_pg.query)
            uri_graph = qs.get("graph", qs.get("graph_name", [None]))[0]
            resolved_graph = graph_name if graph_name is not None else uri_graph
            if resolved_graph is not None:
                validate_graph_name(resolved_graph)

            if is_ssl_scheme:
                if "sslmode" not in qs and "ssl" not in qs:
                    new_query = (
                        f"{parsed_pg.query}&sslmode=require"
                        if parsed_pg.query
                        else "sslmode=require"
                    )

            clean_pg_uri = urlunsplit(
                (target_scheme, parsed_pg.netloc, parsed_pg.path, new_query, parsed_pg.fragment)
            )

            pg_kwargs: dict[str, Any] = {"autocommit": True}
            if is_ssl_scheme:
                pg_kwargs["sslmode"] = "require"

            if auth is not None:
                if not isinstance(auth, tuple) or len(auth) != 2:
                    raise TypeError(
                        f"Postgres/AGE auth must be a (username, password) tuple, got {type(auth).__name__}"
                    )
                pg_kwargs["user"] = auth[0]
                pg_kwargs["password"] = auth[1]

            pg_conn = psycopg.connect(clean_pg_uri, **pg_kwargs)
            setup_postgres_age(pg_conn, force=is_age_scheme)
            pg_bridge = PostgresBridge(pg_conn, graph_name=resolved_graph)
            return (
                AsyncPostgresBridge(pg_conn, graph_name=resolved_graph) if is_async else pg_bridge
            )
        elif scheme in (
            "falkordb",
            "falkordbs",
            "falkor",
            "redis",
            "rediss",
            "valkey",
            "valkeys",
        ):
            try:
                from falkordb import FalkorDB
            except ImportError as e:
                if fallback == "mock":
                    return AsyncMockBridge() if is_async else MockBridge()
                raise ImportError(
                    f"The 'falkordb' Python package is required to connect to URI '{mask_uri_password(driver_or_connection)}'. "
                    f"Install it with 'pip install falkordb', or pass fallback='mock' for offline testing."
                ) from e
            from urllib.parse import parse_qs, urlencode, urlsplit, urlunsplit

            parsed_url = urlsplit(driver_or_connection)
            redis_scheme = "rediss" if scheme in ("rediss", "falkordbs", "valkeys") else "redis"
            qs = parse_qs(parsed_url.query)
            redis_qs = {}
            for k, vals in qs.items():
                if k == "connect_timeout":
                    redis_qs["socket_connect_timeout"] = vals[0]
                elif k in (
                    "socket_timeout",
                    "socket_connect_timeout",
                    "password",
                    "db",
                    "username",
                    "decode_responses",
                    "ssl",
                    "ssl_cert_reqs",
                ):
                    redis_qs[k] = vals[0]

            falkor_kwargs: dict[str, Any] = {}
            if auth is not None:
                if not isinstance(auth, tuple) or len(auth) != 2:
                    raise TypeError(
                        f"FalkorDB/Redis auth must be a (username, password) tuple, got {type(auth).__name__}"
                    )
                falkor_kwargs["username"] = auth[0]
                falkor_kwargs["password"] = auth[1]

            clean_url = urlunsplit(
                (
                    redis_scheme,
                    parsed_url.netloc,
                    parsed_url.path,
                    urlencode(redis_qs),
                    parsed_url.fragment,
                )
            )
            parsed = FalkorDB.from_url(clean_url, **falkor_kwargs)
            f_bridge = FalkorDBBridge(parsed)
            return AsyncFalkorDBBridge(parsed) if is_async else f_bridge
        elif scheme in ("relate", "relatedb"):
            if auth is not None:
                raise TypeError("Relate does not accept auth credentials")
            try:
                import relate
            except ImportError as e:
                if fallback == "mock":
                    return AsyncMockBridge() if is_async else MockBridge()
                raise ImportError(
                    f"The 'relate-db' Python package is required to connect to URI '{mask_uri_password(driver_or_connection)}'. "
                    f"Install it with 'pip install relate-db', or pass fallback='mock' for offline testing."
                ) from e
            path = driver_or_connection.split("://", 1)[1]
            target_path = ":memory:" if (not path or path == ":memory:") else path
            relate_conn = relate.connect(target_path)
            r_bridge = RelateBridge(relate_conn)
            return AsyncRelateBridge(relate_conn) if is_async else r_bridge
        else:
            if fallback == "mock":
                return AsyncMockBridge() if is_async else MockBridge()
            raise ValueError(
                f"No bridge available for URI '{mask_uri_password(driver_or_connection)}' (unrecognized scheme: '{scheme}'). "
                f"Tried adapters: bolt, duckdb, postgres, falkordb, relate. "
                f"Install required driver or check the URI, or pass fallback='mock' (or use a 'mock://' URI) for offline testing."
            )

    if fallback == "mock":
        return AsyncMockBridge() if is_async else MockBridge()
    raise TypeError(
        f"Unsupported database driver or connection object of type {type(driver_or_connection).__name__}. "
        f"Expected a recognized driver connection instance, DatabaseBridge, or URI string."
    )
