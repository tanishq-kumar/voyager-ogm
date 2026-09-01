"""Voyager OGM High-Level Graph Entity Models, Decorators & Type Annotations.

Provides declarative schema modeling for Graph Nodes and Relationships with
automatic descriptor binding, base class injection, and type reflection.
"""

from __future__ import annotations

import inspect
import threading
from collections import defaultdict
from typing import Any, ClassVar, Generic, TypeVar

_T = TypeVar("_T")

# Thread-local alias counter for deterministic auto-aliasing
_alias_state = threading.local()

_BUILTIN_TYPES = {
    "str": str,
    "int": int,
    "float": float,
    "bool": bool,
    "list": list,
    "dict": dict,
}


def _get_next_alias(label: str) -> str:
    if not hasattr(_alias_state, "counters"):
        _alias_state.counters = defaultdict(int)
    key = label.lower()
    idx = _alias_state.counters[key]
    _alias_state.counters[key] += 1
    return f"_{key}_{idx}"


def reset_alias_counters() -> None:
    """Resets the thread-local auto-alias counters.

    Useful in unit tests and deterministic query generation scenarios to ensure
    starting aliases always begin from index 0.

    Example:
        >>> reset_alias_counters()
        >>> p = Person()
        >>> p.alias
        '_person_0'
    """
    _alias_state.counters = defaultdict(int)


class Field(Generic[_T]):
    """Property descriptor supporting type-safe predicate expressions and schema reflection.

    Attributes:
        default: Default value if not assigned during model instantiation.
        name: Property key name in the underlying graph database.
        unique: Flag indicating a UNIQUE property database constraint.
        index: Flag indicating a database index should be created on this property.
        type_annotation: The resolved Python type class (e.g. `str`, `int`).

    Example:
        >>> class Person(Node):
        ...     name: str = Field(index=True)
        ...     email: str = Field(unique=True)
    """

    def __init__(
        self,
        default: Any = ...,
        *,
        name: str | None = None,
        unique: bool = False,
        index: bool = False,
        primary_key: bool = False,
        type_annotation: Any = None,
    ) -> None:
        """Initializes a graph property Field descriptor.

        Args:
            default: Default fallback value for this property.
            name: Custom database property name (defaults to attribute name).
            unique: Whether to enforce a unique constraint.
            index: Whether to create a search index on this property.
            primary_key: Convenience flag setting both unique=True and index=True.
            type_annotation: Python type annotation class.
        """
        self.default = default
        self.name = name
        self.primary_key = primary_key
        self.unique = unique or primary_key
        self.index = index or primary_key
        if isinstance(type_annotation, str) and type_annotation in _BUILTIN_TYPES:
            self.type_annotation = _BUILTIN_TYPES[type_annotation]
        else:
            self.type_annotation = type_annotation

    def __class_getitem__(cls, item: Any) -> Any:
        """Enables generic typing support like `Field[str]` or `Field[int]`.

        Args:
            item: Generic type parameter.

        Returns:
            A Field descriptor initialized with the type annotation.
        """
        return cls(type_annotation=item)

    def __set_name__(self, owner: type, name: str) -> None:
        """Captures the assigned class attribute name.

        Args:
            owner: The owning class object.
            name: The attribute name string.
        """
        if self.name is None:
            self.name = name

    def __get__(self, instance: Any, owner: type | None = None) -> Any:
        """Returns the BoundField descriptor for an instance or the descriptor itself.

        Args:
            instance: Instance of Node or Relationship, or None when accessed from class.
            owner: Owning class.

        Returns:
            BoundField bound to instance's alias, or Field on class-level access.
        """
        if instance is None:
            return self
        alias = getattr(instance, "alias", None)
        if alias is None:
            return self
        assert self.name is not None
        return BoundField(alias, self.name)

    def bind(self, alias: str) -> BoundField:
        """Explicitly binds this property field to a target variable alias.

        Args:
            alias: Target query variable alias string (e.g. 'p', 'actor').

        Returns:
            A BoundField instance referencing the variable and property.
        """
        assert self.name is not None
        return BoundField(alias, self.name)


class BoundField:
    """A field bound to a specific node or relationship alias instance.

    Overloads comparison operators (`==`, `>`, `<`, `>=`, `<=`, `.contains()`)
    to produce AST `PredicateExpr` objects for query building.

    Attributes:
        target_alias: Variable alias of the node or edge (e.g. 'p', 'm').
        field_name: Property name on the entity.
    """

    def __init__(self, target_alias: str, field_name: str) -> None:
        """Initializes a BoundField.

        Args:
            target_alias: Entity variable alias in the query.
            field_name: Property name on the entity.
        """
        self.target_alias = target_alias
        self.field_name = field_name

    def __eq__(self, other: Any) -> PredicateExpr:  # type: ignore[override]
        """Creates an equality predicate `alias.prop = value`.

        Args:
            other: Literal comparison value.

        Returns:
            PredicateExpr with operator 'eq'.
        """
        return PredicateExpr(self.target_alias, self.field_name, "eq", other)

    def __gt__(self, other: Any) -> PredicateExpr:
        """Creates a greater-than predicate `alias.prop > value`.

        Args:
            other: Literal comparison value.

        Returns:
            PredicateExpr with operator 'gt'.
        """
        return PredicateExpr(self.target_alias, self.field_name, "gt", other)

    def __ge__(self, other: Any) -> PredicateExpr:
        """Creates a greater-than-or-equal predicate `alias.prop >= value`.

        Args:
            other: Literal comparison value.

        Returns:
            PredicateExpr with operator 'gte'.
        """
        return PredicateExpr(self.target_alias, self.field_name, "gte", other)

    def __lt__(self, other: Any) -> PredicateExpr:
        """Creates a less-than predicate `alias.prop < value`.

        Args:
            other: Literal comparison value.

        Returns:
            PredicateExpr with operator 'lt'.
        """
        return PredicateExpr(self.target_alias, self.field_name, "lt", other)

    def __le__(self, other: Any) -> PredicateExpr:
        """Creates a less-than-or-equal predicate `alias.prop <= value`.

        Args:
            other: Literal comparison value.

        Returns:
            PredicateExpr with operator 'lte'.
        """
        return PredicateExpr(self.target_alias, self.field_name, "lte", other)

    def contains(self, substring: str) -> PredicateExpr:
        """Creates a string substring predicate `alias.prop CONTAINS value`.

        Args:
            substring: Substring to search for.

        Returns:
            PredicateExpr with operator 'contains'.
        """
        return PredicateExpr(self.target_alias, self.field_name, "contains", substring)

    def count(self) -> AggregationExpr:
        """Returns a `COUNT(alias.prop)` aggregation expression."""
        return AggregationExpr(self.target_alias, self.field_name, "count")

    def avg(self) -> AggregationExpr:
        """Returns an `AVG(alias.prop)` aggregation expression."""
        return AggregationExpr(self.target_alias, self.field_name, "avg")

    def sum(self) -> AggregationExpr:
        """Returns a `SUM(alias.prop)` aggregation expression."""
        return AggregationExpr(self.target_alias, self.field_name, "sum")

    def min(self) -> AggregationExpr:
        """Returns a `MIN(alias.prop)` aggregation expression."""
        return AggregationExpr(self.target_alias, self.field_name, "min")

    def max(self) -> AggregationExpr:
        """Returns a `MAX(alias.prop)` aggregation expression."""
        return AggregationExpr(self.target_alias, self.field_name, "max")

    def collect(self) -> AggregationExpr:
        """Returns a `COLLECT(alias.prop)` aggregation expression."""
        return AggregationExpr(self.target_alias, self.field_name, "collect")


class AggregationExpr:
    """Container for AST column aggregation expressions (e.g. COUNT, AVG)."""

    def __init__(self, target: str, field: str, func: str) -> None:
        self.target = target
        self.target_alias = target
        self.field = field
        self.field_name = field
        self.func = func


class PredicateExpr:
    """Container for AST predicate conditions.

    Attributes:
        target: Variable alias of the entity.
        field: Property name.
        op: Comparison operator string ('eq', 'gt', 'gte', 'lt', 'lte', 'contains').
        value: Right-hand side literal comparison value.
    """

    def __init__(self, target: str, field: str, op: str, value: Any) -> None:
        """Initializes a PredicateExpr.

        Args:
            target: Entity variable alias.
            field: Property name.
            op: Operator string.
            value: Right-hand comparison value.
        """
        self.target = target
        self.field = field
        self.op = op
        self.value = value


def _process_type_annotations(cls: type) -> dict[str, Field]:
    """Extracts type annotations and injects Field descriptors automatically.

    Args:
        cls: Class to inspect.

    Returns:
        Dictionary mapping field attribute names to Field descriptors.
    """
    fields_map: dict[str, Field] = {}

    try:
        annotations = inspect.get_annotations(cls, eval_str=True)
    except Exception:
        annotations = getattr(cls, "__annotations__", {})

    for attr_name, type_hint in annotations.items():
        if attr_name.startswith("_"):
            continue
        if isinstance(type_hint, str) and type_hint in _BUILTIN_TYPES:
            type_hint = _BUILTIN_TYPES[type_hint]

        existing_val = getattr(cls, attr_name, None)
        if isinstance(existing_val, Field):
            existing_val.type_annotation = type_hint
            if existing_val.name is None:
                existing_val.name = attr_name
            fields_map[attr_name] = existing_val
        elif not inspect.isroutine(existing_val):
            field_desc = Field(
                default=existing_val if existing_val is not None else ...,
                name=attr_name,
                type_annotation=type_hint,
            )
            setattr(cls, attr_name, field_desc)
            fields_map[attr_name] = field_desc

    cls._schema_fields = fields_map  # type: ignore[attr-defined]
    return fields_map


class Node:
    """Base class for Voyager OGM Graph Node Entities.

    Can be directly subclassed or automatically injected via the `@node` decorator.

    Attributes:
        __labels__: Class-level list of graph database labels.
        _schema_fields: Dictionary of property field descriptors for schema reflection.

    Example:
        >>> class Developer(Node, label=["Person", "Engineer"]):
        ...     name: str
        ...     age: int
    """

    __labels__: ClassVar[list[str]] = []
    _schema_fields: ClassVar[dict[str, Field]] = {}

    def __init_subclass__(cls, label: str | list[str] | None = None, **kwargs: Any) -> None:
        """Initializes Node subclass metadata and labels.

        Args:
            label: Optional custom label or list of labels.
            **kwargs: Extra keyword arguments passed to super.
        """
        super().__init_subclass__(**kwargs)
        if isinstance(label, str):
            cls.__labels__ = [label]
        elif isinstance(label, list):
            cls.__labels__ = label
        elif not getattr(cls, "__labels__", None):
            cls.__labels__ = [cls.__name__]

        _process_type_annotations(cls)

    def __init__(self, alias: str | None = None, **values: Any) -> None:
        """Instantiates a Node entity with a unique query alias.

        Args:
            alias: Optional explicit variable alias. If omitted, an auto-alias
                (e.g. `_person_0`) is deterministically generated.
            **values: Property key-value pairs.
        """
        label = self.__labels__[0] if self.__labels__ else self.__class__.__name__
        self._alias = alias or _get_next_alias(label)
        self._bound_fields: dict[str, BoundField] = {}
        self._values: dict[str, Any] = dict(values)
        self._dirty_fields: dict[str, Any] = dict(values)

    @property
    def alias(self) -> str:
        """Returns the variable alias assigned to this node instance."""
        return self._alias

    @property
    def labels(self) -> list[str]:
        """Returns the list of graph labels associated with this node."""
        return self.__labels__ if self.__labels__ else [self.__class__.__name__]

    @property
    def dirty_fields(self) -> dict[str, Any]:
        """Returns a dictionary of modified property names and their updated values."""
        return dict(self._dirty_fields)

    def clear_dirty(self) -> None:
        """Clears recorded dirty fields after a commit or save."""
        self._dirty_fields.clear()

    def get(self, name: str, default: Any = None) -> Any:
        """Retrieves an in-memory property value."""
        return self._values.get(name, default)

    def __setattr__(self, name: str, value: Any) -> None:
        if name.startswith("_"):
            super().__setattr__(name, value)
        else:
            if not hasattr(self, "_values"):
                self._values = {}
            if not hasattr(self, "_dirty_fields"):
                self._dirty_fields = {}
            self._values[name] = value
            self._dirty_fields[name] = value

    def count(self) -> AggregationExpr:
        """Returns a `COUNT(alias)` node aggregation expression."""
        return AggregationExpr(self._alias, "*", "count")

    def __getattr__(self, name: str) -> BoundField:
        """Dynamically resolves unknown property names into BoundField descriptors.

        Args:
            name: Property name accessed on the node.

        Returns:
            BoundField bound to this node's alias and property name.

        Raises:
            AttributeError: If name starts with an underscore.
        """
        if name.startswith("_"):
            raise AttributeError(name)
        if name not in self._bound_fields:
            self._bound_fields[name] = BoundField(self._alias, name)
        return self._bound_fields[name]


class Relationship:
    """Base class for Voyager OGM Graph Relationship Entities.

    Can be directly subclassed or automatically injected via `@relationship`.

    Attributes:
        __type__: Relationship type string in the database (e.g. 'ACTED_IN').
        __direction__: Edge traversal direction ('outgoing', 'incoming', 'undirected').
        _schema_fields: Dictionary of property field descriptors.

    Example:
        >>> class ActedIn(Relationship, type_name="ACTED_IN"):
        ...     role: str
        ...     since: int = 2000
    """

    __type__: ClassVar[str] = ""
    __direction__: ClassVar[str] = "outgoing"
    _schema_fields: ClassVar[dict[str, Field]] = {}

    def __init_subclass__(
        cls,
        type_name: str | None = None,
        direction: str = "outgoing",
        **kwargs: Any,
    ) -> None:
        """Initializes Relationship subclass metadata.

        Args:
            type_name: Database edge type string. Defaults to uppercase class name.
            direction: Traversal direction ('outgoing', 'incoming', 'undirected').
            **kwargs: Extra keyword arguments passed to super.
        """
        super().__init_subclass__(**kwargs)
        cls.__type__ = type_name or cls.__name__.upper()
        cls.__direction__ = direction
        _process_type_annotations(cls)

    def __init__(self, alias: str | None = None, **values: Any) -> None:
        """Instantiates a Relationship entity with a unique query alias.

        Args:
            alias: Optional explicit variable alias. If omitted, an auto-alias
                (e.g. `_acted_in_0`) is generated.
            **values: Edge property key-value pairs.
        """
        rel_type = self.__type__ or self.__class__.__name__.upper()
        self._alias = alias or _get_next_alias(rel_type)
        self._bound_fields: dict[str, BoundField] = {}
        self._values = values

    @property
    def alias(self) -> str:
        """Returns the variable alias assigned to this relationship instance."""
        return self._alias

    @property
    def edge_type(self) -> str:
        """Returns the relationship type name (e.g. 'FOLLOWS', 'ACTED_IN')."""
        return self.__type__ or self.__class__.__name__.upper()

    def __getattr__(self, name: str) -> BoundField:
        """Dynamically resolves unknown edge property names into BoundField descriptors.

        Args:
            name: Property name.

        Returns:
            BoundField bound to this edge's alias and property name.

        Raises:
            AttributeError: If name starts with an underscore.
        """
        if name.startswith("_"):
            raise AttributeError(name)
        if name not in self._bound_fields:
            self._bound_fields[name] = BoundField(self._alias, name)
        return self._bound_fields[name]


def node(target: type | str | list[str] | None = None, **kwargs: Any) -> Any:
    """Decorator to mark a Python class as a Voyager Graph Node.

    Automatically injects `Node` inheritance, extracts Python type annotations,
    and sets up schema reflection.

    Args:
        target: Target class when used as `@node` or label when used as `@node(label="...")`.
        **kwargs: Optional configuration parameters (e.g. `label="Person"`).

    Returns:
        The decorated Node class with schema descriptors and auto-aliasing.

    Example:
        >>> @node
        ... class User:
        ...     name: str
        ...     age: int
    """

    def decorator(cls: type) -> type:
        label = kwargs.get("label", target if isinstance(target, (str, list)) else None)
        labels = [label] if isinstance(label, str) else (label if isinstance(label, list) else None)
        cls_labels = labels or [cls.__name__]

        fields_map = _process_type_annotations(cls)

        if not issubclass(cls, Node):
            ns = dict(cls.__dict__)
            ns["__labels__"] = cls_labels
            ns["_schema_fields"] = fields_map
            derived = type(cls.__name__, (cls, Node), ns)
            return derived
        else:
            cls.__labels__ = cls_labels  # type: ignore[attr-defined]
            cls._schema_fields = fields_map  # type: ignore[attr-defined]
            return cls

    if isinstance(target, type):
        return decorator(target)
    return decorator


def relationship(target: type | str | None = None, **kwargs: Any) -> Any:
    """Decorator to mark a Python class as a Voyager Graph Relationship.

    Automatically injects `Relationship` inheritance, extracts type hints,
    and configures edge direction and type reflection.

    Args:
        target: Target class when used as `@relationship` or type_name string.
        **kwargs: Optional configuration parameters (e.g. `type_name="WORKS_AT"`).

    Returns:
        The decorated Relationship class with schema descriptors.

    Example:
        >>> @relationship(type_name="FOLLOWS")
        ... class Follows:
        ...     since: int = 2024
    """
    direction = kwargs.get("direction", "outgoing")

    def decorator(cls: type) -> type:
        type_name = kwargs.get("type_name", target if isinstance(target, str) else None)
        rel_type = type_name or cls.__name__.upper()

        fields_map = _process_type_annotations(cls)

        if not issubclass(cls, Relationship):
            ns = dict(cls.__dict__)
            ns["__type__"] = rel_type
            ns["__direction__"] = direction
            ns["_schema_fields"] = fields_map
            derived = type(cls.__name__, (cls, Relationship), ns)
            return derived
        else:
            cls.__type__ = rel_type  # type: ignore[attr-defined]
            cls.__direction__ = direction  # type: ignore[attr-defined]
            cls._schema_fields = fields_map  # type: ignore[attr-defined]
            return cls

    if isinstance(target, type):
        return decorator(target)
    return decorator
