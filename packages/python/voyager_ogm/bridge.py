"""Database bridging layer and driver adapters for Voyager OGM.

Provides vendor-neutral sync and async bridge protocols, enabling applications
to connect existing database drivers (Neo4j/Memgraph Bolt, DuckDB DuckPGQ, etc.)
or mock bridges without hard coupling to specific driver packages.
"""

from __future__ import annotations

import asyncio
import datetime
import inspect
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

    def ping(self) -> bool:
        """Pings the mock bridge."""
        return True

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
        params = _adapt_bolt_parameters(parameters or {})
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

    async def ping(self) -> bool:
        """Pings DuckDB asynchronously to verify connection liveness."""
        return await asyncio.to_thread(self.sync_bridge.ping)

    async def close(self) -> None:
        """Closes connection asynchronously."""
        await asyncio.to_thread(self.sync_bridge.close)


class PostgresBridge:
    """Synchronous PostgreSQL driver bridge (psycopg 3 / psycopg2) with zero-copy Polars output."""

    def __init__(self, connection: Any) -> None:
        """Initializes PostgresBridge.

        Args:
            connection: PostgreSQL connection instance (e.g. psycopg.Connection).
        """
        self.conn = connection

    def _format_pgq_statement(
        self, statement: str, parameters: dict[str, Any]
    ) -> tuple[str, list[Any] | tuple[Any, ...]]:
        """Formats statement and parameters for PostgreSQL execution."""
        if not parameters:
            return statement, ()

        import re

        def _make_repl(r: str) -> Callable[[re.Match[str]], str]:
            return lambda _m: r

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

    def close(self) -> None:
        """Closes connection."""
        if hasattr(self.conn, "close"):
            self.conn.close()


class AsyncPostgresBridge:
    """Asynchronous PostgreSQL driver bridge using asyncio worker threads."""

    def __init__(self, connection: Any) -> None:
        """Initializes AsyncPostgresBridge.

        Args:
            connection: PostgreSQL connection instance.
        """
        self.sync_bridge = PostgresBridge(connection)

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

    async def close(self) -> None:
        """Closes connection asynchronously."""
        await asyncio.to_thread(self.sync_bridge.close)


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
            return unwrap_spatial_param(sp) if isinstance(sp, dict) else sp
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

    async def ping(self) -> bool:
        """Pings FalkorDB asynchronously to verify connection liveness."""
        return await asyncio.to_thread(self.sync_bridge.ping)

    async def close(self) -> None:
        """Closes FalkorDB connection asynchronously."""
        await asyncio.to_thread(self.sync_bridge.close)


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


register_bridge(_is_neo4j_sync_driver, Neo4jBoltBridge, is_async=False)
register_bridge(_is_neo4j_async_driver, AsyncNeo4jBoltBridge, is_async=True)
register_bridge(_is_duckdb_conn, DuckDbBridge, is_async=False)
register_bridge(_is_duckdb_conn, AsyncDuckDbBridge, is_async=True)
register_bridge(_is_postgres_conn, PostgresBridge, is_async=False)
register_bridge(_is_postgres_conn, AsyncPostgresBridge, is_async=True)
register_bridge(_is_falkordb_conn, FalkorDBBridge, is_async=False)
register_bridge(_is_falkordb_conn, AsyncFalkorDBBridge, is_async=True)


@overload
def create_bridge(
    driver_or_connection: Any, is_async: Literal[False] = False
) -> DatabaseBridge: ...


@overload
def create_bridge(driver_or_connection: Any, is_async: Literal[True]) -> AsyncDatabaseBridge: ...


@overload
def create_bridge(
    driver_or_connection: Any, is_async: bool
) -> DatabaseBridge | AsyncDatabaseBridge: ...


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
    if driver_or_connection is None:
        return AsyncMockBridge() if is_async else MockBridge()

    if isinstance(driver_or_connection, MockBridge):
        return AsyncMockBridge() if is_async else driver_or_connection
    if isinstance(driver_or_connection, AsyncMockBridge):
        return driver_or_connection if is_async else driver_or_connection.sync_mock

    if (
        is_async
        and isinstance(driver_or_connection, AsyncDatabaseBridge)
        and inspect.iscoroutinefunction(getattr(driver_or_connection, "execute", None))
    ):
        return driver_or_connection
    if (
        not is_async
        and isinstance(driver_or_connection, DatabaseBridge)
        and not inspect.iscoroutinefunction(getattr(driver_or_connection, "execute", None))
    ):
        return driver_or_connection

    for matcher, bridge_cls, reg_is_async in _BRIDGE_REGISTRY:
        if reg_is_async == is_async and matcher(driver_or_connection):
            return bridge_cls(driver_or_connection)

    if is_async:
        for matcher, bridge_cls, reg_is_async in _BRIDGE_REGISTRY:
            if not reg_is_async and matcher(driver_or_connection):
                sync_inst = bridge_cls(driver_or_connection)
                if isinstance(sync_inst, DuckDbBridge):
                    return AsyncDuckDbBridge(driver_or_connection)
                elif isinstance(sync_inst, FalkorDBBridge):
                    return AsyncFalkorDBBridge(driver_or_connection)

    # If a database connection URI string was provided, attempt to auto-instantiate the official driver
    if isinstance(driver_or_connection, str):
        if driver_or_connection.startswith("mock://"):
            return AsyncMockBridge() if is_async else MockBridge()

        if "://" not in driver_or_connection:
            raise ValueError(
                f"Invalid connection URI: '{driver_or_connection}'. Expected a valid URI scheme (e.g. 'bolt://', 'duckdb://', 'postgresql://', 'falkordb://', or 'mock://')."
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
                raise ImportError(
                    f"The 'neo4j' Python package is required to connect to URI '{driver_or_connection}'. "
                    f"Install it with 'pip install neo4j'."
                ) from e
            if is_async and hasattr(neo4j, "AsyncGraphDatabase"):
                async_drv = neo4j.AsyncGraphDatabase.driver(driver_or_connection)
                return AsyncNeo4jBoltBridge(async_drv)
            elif hasattr(neo4j, "GraphDatabase"):
                sync_drv = neo4j.GraphDatabase.driver(driver_or_connection)
                return Neo4jBoltBridge(sync_drv)
            else:
                raise RuntimeError(
                    f"Unable to instantiate neo4j driver from '{driver_or_connection}'."
                )
        elif scheme == "duckdb":
            try:
                import duckdb
            except ImportError as e:
                raise ImportError(
                    f"The 'duckdb' Python package is required to connect to URI '{driver_or_connection}'. "
                    f"Install it with 'pip install duckdb'."
                ) from e
            path = driver_or_connection.replace("duckdb://", "") or ":memory:"
            duck_conn = duckdb.connect(path)
            duck_bridge = DuckDbBridge(duck_conn)
            return AsyncDuckDbBridge(duck_conn) if is_async else duck_bridge
        elif scheme in ("postgresql", "postgres", "age", "postgresql+s", "postgres+s", "age+s"):
            try:
                import psycopg
            except ImportError as e:
                raise ImportError(
                    f"The 'psycopg' Python package is required to connect to URI '{driver_or_connection}'. "
                    f"Install it with 'pip install psycopg[binary]'."
                ) from e
            pg_conn = psycopg.connect(driver_or_connection, autocommit=True)
            pg_bridge = PostgresBridge(pg_conn)
            return AsyncPostgresBridge(pg_conn) if is_async else pg_bridge
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
                raise ImportError(
                    f"The 'falkordb' Python package is required to connect to URI '{driver_or_connection}'. "
                    f"Install it with 'pip install falkordb'."
                ) from e
            from urllib.parse import parse_qs, urlencode, urlparse, urlunparse

            parsed_url = urlparse(driver_or_connection)
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
            clean_url = urlunparse(
                (
                    redis_scheme,
                    parsed_url.netloc,
                    parsed_url.path,
                    parsed_url.params,
                    urlencode(redis_qs),
                    parsed_url.fragment,
                )
            )
            parsed = FalkorDB.from_url(clean_url)
            f_bridge = FalkorDBBridge(parsed)
            return AsyncFalkorDBBridge(parsed) if is_async else f_bridge
        else:
            raise ValueError(
                f"Unsupported database URI scheme '{scheme}' in '{driver_or_connection}'. "
                f"Supported schemes: 'bolt', 'neo4j', 'memgraph', 'duckdb', 'postgresql', 'postgres', 'age', 'falkordb', 'redis', or 'mock://'."
            )

    raise TypeError(
        f"Unsupported database driver or connection object of type {type(driver_or_connection).__name__}. "
        f"Expected a recognized driver connection instance, DatabaseBridge, or URI string."
    )
