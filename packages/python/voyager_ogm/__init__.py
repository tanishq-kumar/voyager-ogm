"""Voyager OGM: High-Performance, Vendor-Neutral Object-Graph Mapper."""

from voyager_ogm._voyager_rs import (
    ArrowStream,
    NativeQueryBuilder,
    generate_synthetic_stream,
    version,
)
from voyager_ogm.bridge import (
    AsyncDatabaseBridge,
    AsyncDuckDbBridge,
    AsyncMockBridge,
    AsyncNeo4jBoltBridge,
    BulkExecutionResult,
    DatabaseBridge,
    DuckDbBridge,
    MockBridge,
    Neo4jBoltBridge,
    create_bridge,
    register_bridge,
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
from voyager_ogm.query import CompiledQuery, Query, load_csv, unwind
from voyager_ogm.schema import SchemaManager
from voyager_ogm.session import AsyncSession, Session
from voyager_ogm.streaming import QueryResult, to_arrow, to_polars
from voyager_ogm.transaction import SavepointContext, Transaction

__version__ = version()

__all__ = [
    "ArrowStream",
    "AsyncDatabaseBridge",
    "AsyncDuckDbBridge",
    "AsyncMockBridge",
    "AsyncNeo4jBoltBridge",
    "AsyncSession",
    "BoundField",
    "BulkExecutionResult",
    "BulkIngestionBatch",
    "BulkIngestionPlan",
    "CompiledQuery",
    "DatabaseBridge",
    "DuckDbBridge",
    "Field",
    "MockBridge",
    "NativeQueryBuilder",
    "Neo4jBoltBridge",
    "Node",
    "PredicateExpr",
    "Query",
    "QueryResult",
    "Relationship",
    "SavepointContext",
    "SchemaManager",
    "Session",
    "Transaction",
    "chunk_dataframe",
    "chunk_records",
    "create_bridge",
    "create_bulk_create_plan",
    "create_bulk_create_rel_plan",
    "create_bulk_merge_plan",
    "generate_synthetic_stream",
    "load_csv",
    "node",
    "register_bridge",
    "relationship",
    "reset_alias_counters",
    "to_arrow",
    "to_polars",
    "unwind",
    "version",
]
