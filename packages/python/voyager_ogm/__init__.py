"""Voyager OGM: High-Performance, Vendor-Neutral Object-Graph Mapper."""

from voyager_ogm._voyager_rs import (
    ArrowStream,
    NativeQueryBuilder,
    generate_synthetic_stream,
    version,
)
from voyager_ogm.ingestion import (
    BulkIngestionBatch,
    BulkIngestionPlan,
    chunk_dataframe,
    chunk_records,
    create_bulk_create_plan,
    create_bulk_create_rel_plan,
    create_bulk_merge_plan,
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
from voyager_ogm.query import CompiledQuery, Query, unwind
from voyager_ogm.session import Session
from voyager_ogm.streaming import QueryResult, to_arrow, to_polars
from voyager_ogm.transaction import SavepointContext, Transaction

__version__ = version()

__all__ = [
    "ArrowStream",
    "BoundField",
    "BulkIngestionBatch",
    "BulkIngestionPlan",
    "CompiledQuery",
    "Field",
    "NativeQueryBuilder",
    "Node",
    "PredicateExpr",
    "Query",
    "QueryResult",
    "Relationship",
    "SavepointContext",
    "Session",
    "Transaction",
    "chunk_dataframe",
    "chunk_records",
    "create_bulk_create_plan",
    "create_bulk_create_rel_plan",
    "create_bulk_merge_plan",
    "generate_synthetic_stream",
    "node",
    "relationship",
    "reset_alias_counters",
    "to_arrow",
    "to_polars",
    "unwind",
    "version",
]
