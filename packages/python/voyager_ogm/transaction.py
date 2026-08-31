"""Voyager OGM Two-Layer Rollback Transaction & Context Manager.

Provides robust transaction and savepoint context managers that automatically
rollback in-memory entity mutations and AST allocations if an exception occurs.
"""

from __future__ import annotations

import itertools
from typing import TYPE_CHECKING

from voyager_ogm._voyager_rs import NativeTransaction

if TYPE_CHECKING:
    from types import TracebackType

_tx_counter = itertools.count(1)


class SavepointContext:
    """Context manager for a nested transaction savepoint.

    Automatically releases the savepoint on successful block exit,
    or rolls back in-memory entities to the savepoint if an exception occurs.

    Attributes:
        name: Name of the savepoint.
        tx: Parent Transaction instance.

    Example:
        >>> with tx.savepoint("sp1"):
        ...     # changes here are isolated to sp1
    """

    def __init__(self, tx: Transaction, name: str) -> None:
        """Initializes a SavepointContext.

        Args:
            tx: Parent transaction.
            name: Savepoint name.
        """
        self.tx = tx
        self.name = name

    def __enter__(self) -> SavepointContext:
        """Creates the named savepoint."""
        self.tx.create_savepoint(self.name)
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> bool:
        """Releases the savepoint on success or rolls back on exception."""
        if exc_type is not None:
            self.tx.rollback_to_savepoint(self.name)
            return False
        self.tx.release_savepoint(self.name)
        return False


class Transaction:
    """Two-Layer Transaction manager with in-memory savepoints and dirty rollback.

    Can be used explicitly or as a Python context manager (`with Transaction() as tx:`).

    Attributes:
        id: Unique transaction ID.
        state: Current state string ('ACTIVE', 'COMMITTED', 'ROLLED_BACK').
        is_active: Boolean indicating if transaction is open.

    Example:
        >>> with Transaction() as tx:
        ...     # perform mutations
        ...     pass # automatically committed on exit
    """

    def __init__(self, tx_id: int | None = None) -> None:
        """Initializes a new Transaction.

        Args:
            tx_id: Optional explicit transaction ID integer.
        """
        resolved_id = tx_id if tx_id is not None else next(_tx_counter)
        self._native = NativeTransaction(resolved_id)

    @property
    def id(self) -> int:
        """Returns the unique transaction identifier."""
        return self._native.id

    @property
    def state(self) -> str:
        """Returns the transaction lifecycle state ('ACTIVE', 'COMMITTED', 'ROLLED_BACK')."""
        return self._native.state

    @property
    def is_active(self) -> bool:
        """Checks if the transaction is actively accepting operations."""
        return self._native.is_active

    def savepoint(self, name: str) -> SavepointContext:
        """Creates a nested savepoint context manager.

        Args:
            name: Unique savepoint name.

        Returns:
            SavepointContext for use with `with tx.savepoint("name"):`.

        Raises:
            ValueError: If the transaction is not active.
        """
        if not self.is_active:
            raise ValueError("Cannot create savepoint on inactive transaction")
        return SavepointContext(self, name)

    def create_savepoint(self, name: str) -> None:
        """Explicitly creates a savepoint in the active transaction.

        Args:
            name: Unique savepoint name string.

        Raises:
            ValueError: If the transaction is not active.
        """
        self._native.savepoint(name)

    def rollback_to_savepoint(self, name: str) -> None:
        """Rolls back the active transaction state to a named savepoint.

        Args:
            name: Savepoint name string.

        Raises:
            ValueError: If the savepoint does not exist or transaction is inactive.
        """
        self._native.rollback_to_savepoint(name)

    def release_savepoint(self, name: str) -> None:
        """Releases a named savepoint from the transaction.

        Args:
            name: Savepoint name string.

        Raises:
            ValueError: If the savepoint does not exist or transaction is inactive.
        """
        self._native.release_savepoint(name)

    def commit(self) -> None:
        """Commits the transaction and clears pending mutations.

        Raises:
            ValueError: If the transaction is not active.
        """
        self._native.commit()

    def rollback(self) -> None:
        """Rolls back the transaction and reverts all in-memory mutations.

        Raises:
            ValueError: If the transaction is not active.
        """
        self._native.rollback()

    def __enter__(self) -> Transaction:
        """Enters the transaction context."""
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> bool:
        """Commits on clean exit, or rolls back if an exception occurred."""
        if exc_type is not None:
            if self.is_active:
                self.rollback()
            return False
        if self.is_active:
            self.commit()
        return False
