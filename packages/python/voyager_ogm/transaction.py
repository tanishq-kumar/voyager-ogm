"""Real database transactions for Voyager OGM sessions.

A :class:`Transaction` begins a database transaction on exactly one backend
(the native Rust client or the Python driver bridge), routes every statement
issued through the session while it is active through that transaction, and
commits or rolls back on context exit. The in-memory unit of work lives in
:mod:`voyager_ogm.unit_of_work`.
"""

from __future__ import annotations

import asyncio
import time
from typing import TYPE_CHECKING, Any

from voyager_ogm.bridge import BulkExecutionResult
from voyager_ogm.unit_of_work import UnitOfWork

if TYPE_CHECKING:
    from types import TracebackType

__all__ = ["AsyncSavepointContext", "AsyncTransaction", "Transaction", "TransactionSavepoint"]

_ABORTED_MESSAGE = "transaction is aborted; roll back before executing more statements"


def _select_route(session: Any) -> str:
    """Selects the backend a transaction will use for its whole lifetime.

    Args:
        session: Session or AsyncSession whose configured backends are inspected.

    Returns:
        Either ``"native"`` or ``"bridge"``.
    """
    native = getattr(session, "_native_client", None)
    requested = getattr(session, "_requested_backend", "auto")
    if native is not None and requested in ("native", "auto"):
        router = getattr(session, "_circuit_router", None)
        if router is None or router.should_route_native():
            return "native"
    return "bridge"


def _begin_error(session: Any, route: str, bridge: Any) -> NotImplementedError:
    """Builds a clear error for a backend without transaction support."""
    name = type(bridge).__name__ if bridge is not None else route
    return NotImplementedError(
        f"{name} does not support transactions; use Neo4j/Memgraph (Bolt), "
        "Apache AGE/PostgreSQL, DuckDB/DuckPGQ, or Relate for real database "
        "transactions (FalkorDB is the only built-in backend that raises, because "
        "Redis MULTI/EXEC cannot roll back)"
    )


def _savepoint_error(driver_tx: Any) -> RuntimeError:
    """Builds the backend-specific savepoint error for a transaction object."""
    message = getattr(driver_tx, "_savepoint_error_message", None)
    if message is None:
        message = (
            "Savepoints are not supported by this backend; "
            "use Apache AGE/PostgreSQL for real savepoints"
        )
    return RuntimeError(message)


def _check_no_active_transaction(session: Any) -> None:
    """Raises when the session already has an attached database transaction."""
    active = getattr(session, "_active_transaction", None)
    if active is not None:
        raise RuntimeError(
            "A session transaction is already active; nested transactions are not supported"
        )


class TransactionSavepoint:
    """Context manager for a real database savepoint.

    Rolls back to the savepoint on exception, or releases it on clean exit.

    Attributes:
        transaction: Parent real transaction.
        name: Savepoint identifier.
    """

    def __init__(self, transaction: Transaction, name: str) -> None:
        """Initializes the savepoint context.

        Args:
            transaction: Parent real database transaction.
            name: Savepoint identifier.
        """
        self.transaction = transaction
        self.name = name

    def __enter__(self) -> TransactionSavepoint:
        """Creates the database savepoint."""
        self.transaction.create_savepoint(self.name)
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        """Releases on success or rolls back to the savepoint on exception."""
        if exc_type is not None:
            self.transaction.rollback_to_savepoint(self.name)
            return
        self.transaction.release_savepoint(self.name)


class Transaction:
    """Real database transaction bound to one session backend.

    ``session.transaction()`` begins a transaction immediately. The session routes
    ``execute``, ``execute_to_polars``, ``execute_to_arrow``, ``run_bulk`` and
    ``flush`` through it until it commits or rolls back. When the identity map is
    enabled, an in-memory :class:`UnitOfWork` is opened alongside it so a database
    rollback also restores tracked entity values.

    Attributes:
        state: Lifecycle state from the backend transaction ('ACTIVE',
            'FAILED', 'COMMITTED', or 'ROLLED_BACK').
        backend: Backend selected at begin ('native' or 'bridge').
    """

    def __init__(self, session: Any) -> None:
        """Begins a database transaction on the session's selected backend.

        Args:
            session: Session that will own and route statements through this transaction.
        """
        self._session = session
        self._driver_tx: Any = None
        self._route: str | None = None
        self._unit_of_work: UnitOfWork | None = None
        self._begun = False
        self.begin()

    def begin(self) -> Transaction:
        """Begins the database transaction.

        Returns:
            This transaction.

        Raises:
            RuntimeError: If the session already has an active transaction.
            NotImplementedError: If the selected backend has no transaction support.
        """
        _check_no_active_transaction(self._session)
        route = _select_route(self._session)
        bridge = getattr(self._session, "_bridge", None)
        if route == "native":
            begin = getattr(self._session._native_client, "begin_transaction_sync", None)
            if not callable(begin):
                raise _begin_error(self._session, route, bridge)
            self._driver_tx = begin()
        else:
            begin = getattr(bridge, "begin_transaction", None)
            if not callable(begin):
                raise _begin_error(self._session, route, bridge)
            self._driver_tx = begin()

        self._route = route
        if getattr(self._session, "identity_map_enabled", False):
            self._unit_of_work = UnitOfWork(session=self._session)
        self._session._active_transaction = self
        self._begun = True
        return self

    @property
    def state(self) -> str:
        """Returns the lifecycle state reported by the backend transaction."""
        if not self._begun:
            return "PENDING"
        return str(self._driver_tx.state)

    @property
    def is_active(self) -> bool:
        """Returns whether the backend transaction is still active."""
        return bool(self._begun and self._driver_tx is not None and self._driver_tx.is_active)

    @property
    def backend(self) -> str:
        """Returns the backend chosen when the transaction began."""
        return self._route or "bridge"

    @property
    def supports_savepoints(self) -> bool:
        """Returns whether the backend supports real savepoints."""
        return bool(getattr(self._driver_tx, "supports_savepoints", False))

    def _ensure_can_execute(self) -> None:
        """Raises when the transaction cannot accept another statement."""
        if self.state == "FAILED":
            raise RuntimeError(_ABORTED_MESSAGE)
        if not self.is_active:
            raise ValueError("Transaction is not active")

    def _ensure_can_rollback(self) -> None:
        """Raises only when the transaction already completed."""
        if self.state not in ("ACTIVE", "FAILED"):
            raise ValueError("Transaction is not active")

    def execute(self, query_or_statement: Any, parameters: dict[str, Any] | None = None) -> Any:
        """Executes a query through this transaction.

        Args:
            query_or_statement: Query, CompiledQuery, or raw statement string.
            parameters: Query parameters for a raw statement.

        Returns:
            The session execution result.
        """
        self._ensure_can_execute()
        return self._session.execute(query_or_statement, parameters)

    def execute_to_polars(
        self, query_or_statement: Any, parameters: dict[str, Any] | None = None
    ) -> Any:
        """Executes a query through this transaction and returns a Polars DataFrame."""
        self._ensure_can_execute()
        return self._session.execute_to_polars(query_or_statement, parameters)

    def execute_to_arrow(
        self, query_or_statement: Any, parameters: dict[str, Any] | None = None
    ) -> Any:
        """Executes a query through this transaction and returns an Arrow Table."""
        self._ensure_can_execute()
        return self._session.execute_to_arrow(query_or_statement, parameters)

    def _execute_sync(self, statement: str, parameters: dict[str, Any] | None) -> Any:
        """Executes a statement through the selected backend transaction."""
        self._ensure_can_execute()
        if self.backend == "native":
            return self._driver_tx.execute_sync(statement, parameters)
        return self._driver_tx.execute(statement, parameters)

    def _execute_to_polars_sync(self, statement: str, parameters: dict[str, Any] | None) -> Any:
        """Executes a statement and returns a Polars DataFrame."""
        self._ensure_can_execute()
        if self.backend == "native":
            import polars as pl

            result = self._driver_tx.execute_sync(statement, parameters)
            return pl.DataFrame(result.stream)
        return self._driver_tx.execute_to_polars(statement, parameters)

    def _execute_to_arrow_sync(self, statement: str, parameters: dict[str, Any] | None) -> Any:
        """Executes a statement and returns an Arrow Table."""
        self._ensure_can_execute()
        if self.backend == "native":
            import pyarrow as pa

            result = self._driver_tx.execute_sync(statement, parameters)
            return pa.RecordBatchReader.from_stream(result.stream).read_all()
        return self._driver_tx.execute_to_polars(statement, parameters).to_arrow()

    def _execute_bulk_sync(self, plan: Any) -> BulkExecutionResult:
        """Executes every batch of a bulk plan through the transaction."""
        self._ensure_can_execute()
        if self.backend == "bridge":
            return self._driver_tx.execute_bulk(plan)

        start_time = time.perf_counter()
        total_batches = 0
        total_records = 0
        statement = plan.statement
        for batch_item in plan:
            total_batches += 1
            total_records += len(batch_item.parameters.get("batch", []))
            self._driver_tx.execute_sync(statement, batch_item.parameters)
        return BulkExecutionResult(
            total_batches=total_batches,
            total_records=total_records,
            duration_seconds=time.perf_counter() - start_time,
            statement=statement,
        )

    def _release(self) -> None:
        """Clears the session's active-transaction pointer."""
        if getattr(self._session, "_active_transaction", None) is self:
            self._session._active_transaction = None

    def _commit_driver(self) -> None:
        """Commits on the selected backend's synchronous API."""
        if self.backend == "native":
            self._driver_tx.commit_sync()
        else:
            self._driver_tx.commit()

    def _rollback_driver(self) -> None:
        """Rolls back on the selected backend's synchronous API."""
        if self.backend == "native":
            self._driver_tx.rollback_sync()
        else:
            self._driver_tx.rollback()

    def commit(self) -> None:
        """Commits the database transaction and the paired unit of work."""
        try:
            if self.state == "FAILED":
                # Fail closed, but still end the server-side transaction so the
                # connection (especially psycopg after INERROR) is reusable.
                try:
                    self._rollback_driver()
                except BaseException:
                    pass
                raise RuntimeError(_ABORTED_MESSAGE)
            self._ensure_can_execute()
            try:
                self._commit_driver()
            except BaseException:
                # Best-effort rollback so a failed commit cannot leave an open
                # server transaction behind. Never mask the original error.
                try:
                    self._rollback_driver()
                except BaseException:
                    pass
                raise
            if self._unit_of_work is not None:
                self._unit_of_work.commit()
        finally:
            self._release()

    def rollback(self) -> None:
        """Rolls back the database transaction and restores tracked entities."""
        try:
            self._ensure_can_rollback()
            self._rollback_driver()
        finally:
            if self._unit_of_work is not None:
                self._unit_of_work.rollback()
            self._release()

    def create_savepoint(self, name: str) -> None:
        """Creates a real database savepoint (PostgreSQL/AGE only)."""
        self._ensure_can_execute()
        if not self.supports_savepoints:
            raise _savepoint_error(self._driver_tx)
        if self.backend == "native":
            self._driver_tx.savepoint_sync(name)
        else:
            self._driver_tx.savepoint(name)
        if self._unit_of_work is not None:
            self._unit_of_work.create_savepoint(name)

    def rollback_to_savepoint(self, name: str) -> None:
        """Rolls the database and unit of work back to a named savepoint."""
        self._ensure_can_execute()
        if not self.supports_savepoints:
            raise _savepoint_error(self._driver_tx)
        if self.backend == "native":
            self._driver_tx.rollback_to_savepoint_sync(name)
        else:
            self._driver_tx.rollback_to_savepoint(name)
        if self._unit_of_work is not None:
            self._unit_of_work.rollback_to_savepoint(name)

    def release_savepoint(self, name: str) -> None:
        """Releases a named database savepoint."""
        self._ensure_can_execute()
        if not self.supports_savepoints:
            raise _savepoint_error(self._driver_tx)
        if self.backend == "native":
            self._driver_tx.release_savepoint_sync(name)
        else:
            self._driver_tx.release_savepoint(name)
        if self._unit_of_work is not None:
            self._unit_of_work.release_savepoint(name)

    def savepoint(self, name: str) -> TransactionSavepoint:
        """Creates a savepoint context manager.

        Args:
            name: Savepoint identifier.

        Returns:
            Context manager that creates and releases/rolls back the savepoint.
        """
        self._ensure_can_execute()
        if not self.supports_savepoints:
            raise _savepoint_error(self._driver_tx)
        return TransactionSavepoint(self, name)

    def __enter__(self) -> Transaction:
        """Enters the transaction context."""
        if not self._begun:
            self.begin()
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        """Commits on clean exit, or rolls back and re-raises on exception."""
        if exc_type is not None:
            if self.state in ("ACTIVE", "FAILED"):
                try:
                    self.rollback()
                except Exception:
                    # The original statement/body exception must surface.
                    pass
            return
        if self.state in ("ACTIVE", "FAILED"):
            self.commit()


class AsyncSavepointContext:
    """Asynchronous context manager for a real database savepoint."""

    def __init__(self, transaction: AsyncTransaction, name: str) -> None:
        """Initializes the async savepoint context."""
        self.transaction = transaction
        self.name = name

    async def __aenter__(self) -> AsyncSavepointContext:
        """Creates the database savepoint."""
        await self.transaction.create_savepoint(self.name)
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        """Releases on success or rolls back to the savepoint on exception."""
        if exc_type is not None:
            await self.transaction.rollback_to_savepoint(self.name)
            return
        await self.transaction.release_savepoint(self.name)


class AsyncTransaction:
    """Real asynchronous database transaction bound to one session backend.

    Use ``async with session.transaction() as tx:``. The transaction begins on
    ``__aenter__`` (or an explicit ``await tx.begin()``) and every session
    execution path issued while active is routed through it.
    """

    def __init__(self, session: Any) -> None:
        """Creates a pending async transaction for the session.

        Args:
            session: AsyncSession that will own and route statements.
        """
        _check_no_active_transaction(session)
        self._session = session
        self._driver_tx: Any = None
        self._route: str | None = None
        self._unit_of_work: UnitOfWork | None = None
        self._begun = False

    async def begin(self) -> AsyncTransaction:
        """Begins the database transaction.

        Returns:
            This transaction.
        """
        _check_no_active_transaction(self._session)
        route = _select_route(self._session)
        bridge = getattr(self._session, "_bridge", None)
        if route == "native":
            begin = getattr(self._session._native_client, "begin_transaction", None)
            if not callable(begin):
                raise _begin_error(self._session, route, bridge)
        else:
            begin = getattr(bridge, "begin_transaction", None)
            if not callable(begin):
                raise _begin_error(self._session, route, bridge)

        self._driver_tx = await begin()
        self._route = route
        if getattr(self._session, "identity_map_enabled", False):
            self._unit_of_work = UnitOfWork(session=self._session)
        self._session._active_transaction = self
        self._begun = True
        return self

    @property
    def state(self) -> str:
        """Returns the lifecycle state reported by the backend transaction."""
        if not self._begun:
            return "PENDING"
        return str(self._driver_tx.state)

    @property
    def is_active(self) -> bool:
        """Returns whether the backend transaction is still active."""
        return bool(self._begun and self._driver_tx is not None and self._driver_tx.is_active)

    @property
    def backend(self) -> str:
        """Returns the backend chosen when the transaction began."""
        return self._route or "bridge"

    @property
    def supports_savepoints(self) -> bool:
        """Returns whether the backend supports real savepoints."""
        return bool(getattr(self._driver_tx, "supports_savepoints", False))

    def _ensure_can_execute(self) -> None:
        """Raises when the transaction cannot accept another statement."""
        if self.state == "FAILED":
            raise RuntimeError(_ABORTED_MESSAGE)
        if not self.is_active:
            raise ValueError("Transaction is not active")

    def _ensure_can_rollback(self) -> None:
        """Raises only when the transaction already completed."""
        if self.state not in ("ACTIVE", "FAILED"):
            raise ValueError("Transaction is not active")

    async def execute(
        self, query_or_statement: Any, parameters: dict[str, Any] | None = None
    ) -> Any:
        """Executes a query through this transaction."""
        self._ensure_can_execute()
        return await self._session.execute(query_or_statement, parameters)

    async def execute_to_polars(
        self, query_or_statement: Any, parameters: dict[str, Any] | None = None
    ) -> Any:
        """Executes a query through this transaction and returns a Polars DataFrame."""
        self._ensure_can_execute()
        return await self._session.execute_to_polars(query_or_statement, parameters)

    async def execute_to_arrow(
        self, query_or_statement: Any, parameters: dict[str, Any] | None = None
    ) -> Any:
        """Executes a query through this transaction and returns an Arrow Table."""
        self._ensure_can_execute()
        return await self._session.execute_to_arrow(query_or_statement, parameters)

    async def _execute(self, statement: str, parameters: dict[str, Any] | None) -> Any:
        """Executes a statement through the selected backend transaction."""
        self._ensure_can_execute()
        try:
            return await self._driver_tx.execute(statement, parameters)
        except asyncio.CancelledError:
            self._release()
            raise

    async def _execute_to_polars(self, statement: str, parameters: dict[str, Any] | None) -> Any:
        """Executes a statement and returns a Polars DataFrame."""
        self._ensure_can_execute()
        try:
            if self.backend == "native":
                import polars as pl

                result = await self._driver_tx.execute(statement, parameters)
                return pl.DataFrame(result.stream)
            return await self._driver_tx.execute_to_polars(statement, parameters)
        except asyncio.CancelledError:
            self._release()
            raise

    async def _execute_to_arrow(self, statement: str, parameters: dict[str, Any] | None) -> Any:
        """Executes a statement and returns an Arrow Table."""
        self._ensure_can_execute()
        try:
            if self.backend == "native":
                import pyarrow as pa

                result = await self._driver_tx.execute(statement, parameters)
                return pa.RecordBatchReader.from_stream(result.stream).read_all()
            return (await self._driver_tx.execute_to_polars(statement, parameters)).to_arrow()
        except asyncio.CancelledError:
            self._release()
            raise

    async def _execute_bulk(self, plan: Any) -> BulkExecutionResult:
        """Executes every batch of a bulk plan through the transaction."""
        self._ensure_can_execute()
        try:
            if self.backend == "bridge":
                return await self._driver_tx.execute_bulk(plan)

            start_time = time.perf_counter()
            total_batches = 0
            total_records = 0
            statement = plan.statement
            for batch_item in plan:
                total_batches += 1
                total_records += len(batch_item.parameters.get("batch", []))
                await self._driver_tx.execute(statement, batch_item.parameters)
            return BulkExecutionResult(
                total_batches=total_batches,
                total_records=total_records,
                duration_seconds=time.perf_counter() - start_time,
                statement=statement,
            )
        except asyncio.CancelledError:
            self._release()
            raise

    def _release(self) -> None:
        """Clears the session's active-transaction pointer."""
        if getattr(self._session, "_active_transaction", None) is self:
            self._session._active_transaction = None

    async def commit(self) -> None:
        """Commits the database transaction and the paired unit of work."""
        try:
            if self.state == "FAILED":
                # Fail closed, but still end the server-side transaction so the
                # connection (especially psycopg after INERROR) is reusable.
                try:
                    await self._driver_tx.rollback()
                except BaseException:
                    pass
                raise RuntimeError(_ABORTED_MESSAGE)
            self._ensure_can_execute()
            try:
                await self._driver_tx.commit()
            except asyncio.CancelledError:
                raise
            except BaseException:
                # Best-effort rollback so a failed commit cannot leave an open
                # server transaction behind. Never mask the original error.
                try:
                    await self._driver_tx.rollback()
                except BaseException:
                    pass
                raise
            if self._unit_of_work is not None:
                self._unit_of_work.commit()
        finally:
            self._release()

    async def rollback(self) -> None:
        """Rolls back the database transaction and restores tracked entities."""
        try:
            self._ensure_can_rollback()
            await self._driver_tx.rollback()
        finally:
            if self._unit_of_work is not None:
                self._unit_of_work.rollback()
            self._release()

    async def create_savepoint(self, name: str) -> None:
        """Creates a real database savepoint (PostgreSQL/AGE only)."""
        self._ensure_can_execute()
        if not self.supports_savepoints:
            raise _savepoint_error(self._driver_tx)
        await self._driver_tx.savepoint(name)
        if self._unit_of_work is not None:
            self._unit_of_work.create_savepoint(name)

    async def rollback_to_savepoint(self, name: str) -> None:
        """Rolls the database and unit of work back to a named savepoint."""
        self._ensure_can_execute()
        if not self.supports_savepoints:
            raise _savepoint_error(self._driver_tx)
        await self._driver_tx.rollback_to_savepoint(name)
        if self._unit_of_work is not None:
            self._unit_of_work.rollback_to_savepoint(name)

    async def release_savepoint(self, name: str) -> None:
        """Releases a named database savepoint."""
        self._ensure_can_execute()
        if not self.supports_savepoints:
            raise _savepoint_error(self._driver_tx)
        await self._driver_tx.release_savepoint(name)
        if self._unit_of_work is not None:
            self._unit_of_work.release_savepoint(name)

    def savepoint(self, name: str) -> AsyncSavepointContext:
        """Creates an async savepoint context manager."""
        self._ensure_can_execute()
        if not self.supports_savepoints:
            raise _savepoint_error(self._driver_tx)
        return AsyncSavepointContext(self, name)

    async def __aenter__(self) -> AsyncTransaction:
        """Begins the transaction and enters the async context."""
        if not self._begun:
            await self.begin()
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        """Commits on clean exit, or rolls back and re-raises on exception."""
        if exc_type is not None:
            if self.state in ("ACTIVE", "FAILED"):
                try:
                    await self.rollback()
                except BaseException:
                    # The original statement/body exception (including task
                    # cancellation) must surface.
                    pass
            return
        if self.state in ("ACTIVE", "FAILED"):
            await self.commit()
