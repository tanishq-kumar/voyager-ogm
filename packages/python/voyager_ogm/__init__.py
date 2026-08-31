"""Voyager OGM: High-Performance, Vendor-Neutral Object-Graph Mapper."""

from voyager_ogm._voyager_rs import (
    ArrowStream,
    NativeQueryBuilder,
    generate_synthetic_stream,
    version,
)
from voyager_ogm.models import (
    BoundField,
    Field,
    Node,
    PredicateExpr,
    Relationship,
    node,
    relationship,
    reset_alias_counters,
)
from voyager_ogm.query import CompiledQuery, Query
from voyager_ogm.streaming import QueryResult, to_arrow, to_polars
from voyager_ogm.transaction import SavepointContext, Transaction

__version__ = version()

__all__ = [
    "ArrowStream",
    "BoundField",
    "CompiledQuery",
    "Field",
    "NativeQueryBuilder",
    "Node",
    "PredicateExpr",
    "Query",
    "QueryResult",
    "Relationship",
    "SavepointContext",
    "Transaction",
    "generate_synthetic_stream",
    "node",
    "relationship",
    "reset_alias_counters",
    "to_arrow",
    "to_polars",
    "version",
]
