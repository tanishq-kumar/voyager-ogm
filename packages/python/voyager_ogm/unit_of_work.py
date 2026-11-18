"""Voyager OGM in-memory Unit of Work and savepoint context managers.

The unit of work tracks in-memory entity state and uses the native Rust scope for
lifecycle transitions and savepoint bookkeeping. When bound to a session, it also
snapshots the session identity map so rollback restores entity property values.
"""

from __future__ import annotations

import copy
import itertools
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from voyager_ogm._voyager_rs import NativeUnitOfWorkScope

if TYPE_CHECKING:
    from types import TracebackType

_unit_of_work_counter = itertools.count(1)


@dataclass
class _EntitySnapshot:
    """Captured in-memory state of one identity-mapped entity."""

    entity: Any
    values: dict[str, Any] | None
    dirty_fields: dict[str, Any] | None


_Snapshot = dict[tuple[type[Any], Any], _EntitySnapshot]


def _copy_mapping(values: dict[str, Any]) -> dict[str, Any]:
    """Returns a defensive copy of an entity property mapping.

    Deep copies preserve nested mutable values across a rollback. Values that
    cannot be deep-copied fall back to a shallow copy rather than failing.
    """
    try:
        return copy.deepcopy(values)
    except Exception:
        return dict(values)


class SavepointContext:
    """Context manager for a nested unit-of-work savepoint.

    Automatically releases the savepoint on successful block exit,
    or rolls back in-memory entities to the savepoint if an exception occurs.

    Attributes:
        name: Name of the savepoint.
        tx: Parent UnitOfWork instance.

    Example:
        >>> with uow.savepoint("sp1"):
        ...     # changes here are isolated to sp1
    """

    def __init__(self, tx: UnitOfWork, name: str) -> None:
        """Initializes a SavepointContext.

        Args:
            tx: Parent unit of work.
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
    ) -> None:
        """Releases the savepoint on success or rolls back on exception."""
        if exc_type is not None:
            self.tx.rollback_to_savepoint(self.name)
            return
        self.tx.release_savepoint(self.name)


class UnitOfWork:
    """In-memory Unit of Work with savepoints and identity-map snapshots.

    Can be used explicitly or as a Python context manager (`with UnitOfWork() as uow:`).
    Use `Session.unit_of_work()` / `AsyncSession.unit_of_work()` to bind a unit of work
    to a session so rollback restores its tracked entities.

    Attributes:
        id: Unique unit-of-work scope ID.
        state: Current state string ('ACTIVE', 'COMMITTED', 'ROLLED_BACK').
        is_active: Boolean indicating if the unit of work is open.

    Example:
        >>> with session.unit_of_work() as uow:
        ...     # perform mutations
        ...     pass  # automatically committed on exit
    """

    def __init__(self, tx_id: int | None = None, session: Any = None) -> None:
        """Initializes a new UnitOfWork.

        Args:
            tx_id: Optional explicit unit-of-work scope ID integer.
            session: Optional Session or AsyncSession whose identity map is snapshotted.
        """
        resolved_id = tx_id if tx_id is not None else next(_unit_of_work_counter)
        self._native = NativeUnitOfWorkScope(resolved_id)
        self._session = session
        self._snapshots: dict[str, _Snapshot] = {}
        self._initial_snapshot: _Snapshot | None = (
            self._capture_snapshot() if session is not None else None
        )

    @property
    def id(self) -> int:
        """Returns the unique unit-of-work scope identifier."""
        return self._native.id

    @property
    def state(self) -> str:
        """Returns the lifecycle state ('ACTIVE', 'COMMITTED', 'ROLLED_BACK')."""
        return self._native.state

    @property
    def is_active(self) -> bool:
        """Checks if the unit of work is actively accepting operations."""
        return self._native.is_active

    def savepoint(self, name: str) -> SavepointContext:
        """Creates a nested savepoint context manager.

        Args:
            name: Unique savepoint name.

        Returns:
            SavepointContext for use with `with uow.savepoint("name"):`.

        Raises:
            ValueError: If the unit of work is not active.
        """
        if not self.is_active:
            raise ValueError("Cannot create savepoint on inactive transaction")
        return SavepointContext(self, name)

    def create_savepoint(self, name: str) -> None:
        """Explicitly creates a savepoint in the active unit of work.

        Args:
            name: Unique savepoint name string.

        Raises:
            ValueError: If the unit of work is not active.
        """
        self._native.savepoint(name)
        if self._session is not None:
            self._snapshots[name] = self._capture_snapshot()

    def rollback_to_savepoint(self, name: str) -> None:
        """Rolls back the active unit of work state to a named savepoint.

        Args:
            name: Savepoint name string.

        Raises:
            ValueError: If the savepoint does not exist or the unit of work is inactive.
        """
        self._native.rollback_to_savepoint(name)
        snapshot = self._snapshots.get(name)
        if snapshot is not None:
            self._restore_snapshot(snapshot)

    def release_savepoint(self, name: str) -> None:
        """Releases a named savepoint from the unit of work.

        Args:
            name: Savepoint name string.

        Raises:
            ValueError: If the savepoint does not exist or the unit of work is inactive.
        """
        self._native.release_savepoint(name)
        self._snapshots.pop(name, None)

    def commit(self) -> None:
        """Commits the unit of work, keeping entity state pending for `session.flush()`.

        Raises:
            ValueError: If the unit of work is not active.
        """
        self._native.commit()
        self._snapshots.clear()
        self._initial_snapshot = None

    def rollback(self) -> None:
        """Rolls back the unit of work and restores the session's in-memory entities.

        Raises:
            ValueError: If the unit of work is not active.
        """
        self._native.rollback()
        if self._initial_snapshot is not None:
            self._restore_snapshot(self._initial_snapshot)
        self._snapshots.clear()
        self._initial_snapshot = None

    def __enter__(self) -> UnitOfWork:
        """Enters the unit-of-work context."""
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        """Commits on clean exit, or rolls back if an exception occurred."""
        if exc_type is not None:
            if self.is_active:
                self.rollback()
            return
        if self.is_active:
            self.commit()

    def _capture_snapshot(self) -> _Snapshot:
        """Captures the session identity map's entity values and dirty fields."""
        snapshot: _Snapshot = {}
        for key, entity in list(self._session._identity_map.items()):
            values = getattr(entity, "_values", None)
            dirty_fields = getattr(entity, "_dirty_fields", None)
            snapshot[key] = _EntitySnapshot(
                entity=entity,
                values=_copy_mapping(values) if isinstance(values, dict) else None,
                dirty_fields=(
                    _copy_mapping(dirty_fields) if isinstance(dirty_fields, dict) else None
                ),
            )
        return snapshot

    def _restore_snapshot(self, snapshot: _Snapshot) -> None:
        """Restores session entities to a captured identity-map snapshot."""
        identity_map = self._session._identity_map
        for key in list(identity_map.keys()):
            if key not in snapshot:
                identity_map.pop(key, None)

        for key, entity_snapshot in snapshot.items():
            entity = entity_snapshot.entity
            if entity_snapshot.values is not None:
                entity._values = _copy_mapping(entity_snapshot.values)
            if entity_snapshot.dirty_fields is not None:
                entity._dirty_fields = _copy_mapping(entity_snapshot.dirty_fields)
            if identity_map.get(key) is not entity:
                identity_map[key] = entity
            attach_session = getattr(entity, "_attach_session", None)
            if attach_session is not None:
                attach_session(self._session)
