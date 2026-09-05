"""SQLAlchemy Hybrid Bridge for Voyager OGM.

Enables unified querying and transaction coordination across
relational databases (PostgreSQL, SQLite, DuckDB via SQLAlchemy) and graph engines
(Neo4j, Memgraph, Apache AGE, FalkorDB, and in-memory graph models).
"""

from __future__ import annotations

from collections.abc import Callable
from typing import TYPE_CHECKING, Any

import polars as pl

from voyager_ogm.models import Node
from voyager_ogm.query import CompiledQuery, Query
from voyager_ogm.session import AsyncSession, Session

if TYPE_CHECKING:
    from sqlalchemy.engine import Connection, Engine
    from sqlalchemy.orm import Session as SASession


class HybridQuery:
    """Fluent query descriptor joining SQLAlchemy relational queries with Voyager graph traversals.

    Example:
        ```python
        hq = (
            HybridQuery()
            .relational(select(UserModel).where(UserModel.is_active == True), key="user_id")
            .join_graph(
                Query.match(u).to(follows).node(friend).return_(friend_name=friend.name),
                on=u.user_id,
            )
        )
        df = hybrid_session.execute_to_polars(hq)
        ```
    """

    def __init__(self) -> None:
        """Initializes an empty HybridQuery."""
        self._relational_select: Any = None
        self._relational_key: str | Any = None
        self._graph_query: Query | CompiledQuery | str | None = None
        self._graph_join_key: str | Any = None
        self._direction: str = "relational_first"

    def relational(self, statement: Any, key: str | Any) -> HybridQuery:
        """Specifies the SQLAlchemy SELECT statement and the primary/foreign join key.

        Args:
            statement: SQLAlchemy Select object or raw SQL text construct.
            key: Column name string or SQLAlchemy Column element used for graph joining.

        Returns:
            Self for fluent chaining.
        """
        self._relational_select = statement
        self._relational_key = key
        return self

    def join_graph(
        self,
        query: Query | CompiledQuery | str,
        on: str | Any,
    ) -> HybridQuery:
        """Specifies the Voyager graph query and the join property on the graph entity.

        Args:
            query: Voyager Query builder instance or compiled query.
            on: Property name or BoundField on the graph node to match against the relational key.

        Returns:
            Self for fluent chaining.
        """
        self._graph_query = query
        self._graph_join_key = on
        return self

    def graph_first(self) -> HybridQuery:
        """Sets execution priority to execute the graph query first, filtering relational rows by graph results."""
        self._direction = "graph_first"
        return self

    def relational_first(self) -> HybridQuery:
        """Sets execution priority to execute the relational query first, feeding keys into the graph traversal."""
        self._direction = "relational_first"
        return self

    @property
    def direction(self) -> str:
        """Returns the configured query execution order."""
        return self._direction


class HybridSession:
    """Coordinates unified relational (SQLAlchemy) and graph (Voyager) execution pipelines."""

    def __init__(
        self,
        sa_session_or_engine: SASession | Engine | Connection,
        graph_session: Session,
    ) -> None:
        """Initializes a HybridSession bridging SQLAlchemy with Voyager.

        Args:
            sa_session_or_engine: Active SQLAlchemy Session, Engine, or Connection.
            graph_session: Active Voyager graph Session.
        """
        self.sa = sa_session_or_engine
        self.graph = graph_session

    def __enter__(self) -> HybridSession:
        """Context manager entry."""
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Context manager exit with transaction coordination."""
        if exc_type is not None:
            self.rollback()
        else:
            self.commit()

    def commit(self) -> None:
        """Commits both relational and graph transactions if commit methods are available."""
        commit_sa = getattr(self.sa, "commit", None)
        if callable(commit_sa):
            commit_sa()
        commit_graph = getattr(self.graph, "commit", None)
        if callable(commit_graph):
            commit_graph()

    def rollback(self) -> None:
        """Rolls back both relational and graph transactions if rollback methods are available."""
        rollback_sa = getattr(self.sa, "rollback", None)
        if callable(rollback_sa):
            rollback_sa()
        rollback_graph = getattr(self.graph, "rollback", None)
        if callable(rollback_graph):
            rollback_graph()

    def query_graph_from_relational(
        self,
        sa_statement: Any,
        relational_key: str | Any,
        graph_query_fn: Callable[[list[Any]], Query | CompiledQuery | str],
        join_column: str | None = None,
    ) -> pl.DataFrame:
        """Executes a SQLAlchemy query first, then passes the extracted key list to a graph query.

        Args:
            sa_statement: SQLAlchemy Select construct.
            relational_key: Attribute name or Column element to extract from relational rows.
            graph_query_fn: Callable receiving `list[keys]` and returning a Voyager `Query` or statement.
            join_column: Column name to join on (defaults to relational_key name).

        Returns:
            Polars DataFrame containing joined relational and graph fields.
        """
        rel_records = self._fetch_relational_dicts(sa_statement)
        if not rel_records:
            return pl.DataFrame()

        key_name = self._resolve_key_name(relational_key)
        join_col_name = join_column or key_name
        keys = [r[key_name] for r in rel_records if key_name in r and r[key_name] is not None]
        if not keys:
            return pl.DataFrame(rel_records)

        g_query = graph_query_fn(keys)
        graph_df = self.graph.execute_to_polars(g_query)

        if graph_df.is_empty():
            rel_df = pl.DataFrame(rel_records)
            return rel_df

        rel_df = pl.DataFrame(rel_records)

        if key_name in rel_df.columns and join_col_name in graph_df.columns:
            return rel_df.join(graph_df, left_on=key_name, right_on=join_col_name, how="inner")
        elif join_col_name in rel_df.columns and join_col_name in graph_df.columns:
            return rel_df.join(graph_df, on=join_col_name, how="inner")
        elif key_name in rel_df.columns and key_name in graph_df.columns:
            return rel_df.join(graph_df, on=key_name, how="inner")
        else:
            return graph_df

    def query_relational_from_graph(
        self,
        graph_query: Query | CompiledQuery | str,
        graph_key: str | Any,
        sa_model_or_table: Any,
        sa_key: str | Any,
        filter_fn: Callable[[Any, list[Any]], Any] | None = None,
    ) -> pl.DataFrame:
        """Executes a Voyager graph traversal first, then enriches nodes with SQLAlchemy relational data.

        Args:
            graph_query: Voyager Query builder, CompiledQuery, or query string.
            graph_key: Field name on graph results to extract IDs from.
            sa_model_or_table: SQLAlchemy ORM model class or Table.
            sa_key: SQLAlchemy Column element or attribute name.
            filter_fn: Optional custom SQLAlchemy filter builder `fn(model, ids)`.

        Returns:
            Polars DataFrame containing enriched graph and relational records.
        """
        graph_df = self.graph.execute_to_polars(graph_query)
        if graph_df.is_empty():
            return pl.DataFrame()

        g_key_name = self._resolve_key_name(graph_key)
        if g_key_name not in graph_df.columns:
            return graph_df

        ids = graph_df[g_key_name].to_list()
        unique_ids = list(dict.fromkeys(ids))

        try:
            from sqlalchemy import select

            col = getattr(sa_model_or_table, sa_key) if isinstance(sa_key, str) else sa_key
            if filter_fn is not None:
                stmt = filter_fn(sa_model_or_table, unique_ids)
            else:
                stmt = select(sa_model_or_table).where(col.in_(unique_ids))

            rel_records = self._fetch_relational_dicts(stmt)
        except Exception:
            rel_records = []

        if not rel_records:
            return graph_df

        rel_df = pl.DataFrame(rel_records)
        sa_key_name = self._resolve_key_name(sa_key)

        return graph_df.join(
            rel_df,
            left_on=g_key_name,
            right_on=sa_key_name,
            how="inner",
        )

    def execute_to_polars(self, hybrid_query: HybridQuery) -> pl.DataFrame:
        """Executes a unified HybridQuery combining relational and graph sources into a Polars DataFrame.

        Args:
            hybrid_query: Configured HybridQuery instance.

        Returns:
            Combined Polars DataFrame.
        """
        if hybrid_query.direction == "relational_first":
            rel_key = hybrid_query._relational_key
            key_name = self._resolve_key_name(rel_key)

            rel_records = self._fetch_relational_dicts(hybrid_query._relational_select)
            if not rel_records:
                return pl.DataFrame()

            keys = [r[key_name] for r in rel_records if key_name in r]
            rel_df = pl.DataFrame(rel_records)

            g_q = hybrid_query._graph_query
            graph_df = self._execute_graph_query_with_keys(g_q, hybrid_query._graph_join_key, keys)
            if graph_df.is_empty():
                return rel_df

            join_k = self._resolve_key_name(hybrid_query._graph_join_key)
            if join_k in graph_df.columns and key_name in rel_df.columns:
                return rel_df.join(graph_df, left_on=key_name, right_on=join_k, how="inner")
            return rel_df
        else:
            if hybrid_query._graph_query is None:
                return pl.DataFrame()
            graph_df = self.graph.execute_to_polars(hybrid_query._graph_query)
            if graph_df.is_empty():
                return pl.DataFrame()

            join_k = self._resolve_key_name(hybrid_query._graph_join_key)
            if join_k not in graph_df.columns:
                return graph_df

            keys = graph_df[join_k].to_list()
            rel_key = hybrid_query._relational_key
            rel_key_name = self._resolve_key_name(rel_key)

            try:
                stmt = hybrid_query._relational_select
                in_fn = getattr(rel_key, "in_", None)
                if callable(in_fn):
                    stmt = stmt.where(in_fn(keys))
                rel_records = self._fetch_relational_dicts(stmt)
                rel_df = pl.DataFrame(rel_records) if rel_records else pl.DataFrame()
            except Exception:
                rel_df = pl.DataFrame()

            if not rel_df.is_empty() and rel_key_name in rel_df.columns:
                return graph_df.join(rel_df, left_on=join_k, right_on=rel_key_name, how="inner")
            return graph_df

    def sync_table_to_graph(
        self,
        sa_statement: Any,
        node_model: type[Node],
        key_field: str | None = None,
        key_mapping: dict[str, str] | None = None,
        batch_size: int = 10_000,
    ) -> int:
        """Synchronizes rows from a SQLAlchemy table into Voyager graph nodes using bulk upserts.

        Args:
            sa_statement: SQLAlchemy SELECT query for sourcing rows.
            node_model: Target Voyager Node model class.
            key_field: Optional primary/unique key field name on node model (auto-detected if omitted).
            key_mapping: Optional dictionary mapping relational column names to node property names.
            batch_size: Number of records per bulk ingestion batch.

        Returns:
            Total count of node entities upserted into the graph database.
        """
        records = self._fetch_relational_dicts(sa_statement)
        target_key = key_field
        if not target_key:
            fields = getattr(node_model, "_schema_fields", {})
            if "id" in fields:
                target_key = "id"
            elif fields:
                target_key = next(iter(fields.keys()))
            else:
                target_key = "id"

        if key_mapping:
            renamed_records = []
            for r in records:
                new_r = {}
                for k, v in r.items():
                    target_k = key_mapping.get(k, k)
                    new_r[target_k] = v
                renamed_records.append(new_r)
            records = renamed_records

        plan = self.graph.bulk_upsert(
            model=node_model,
            data=records,
            key_field=target_key,
            batch_size=batch_size,
        )
        res = self.graph.run_bulk(plan)
        return res.total_records

    def _fetch_relational_dicts(self, statement: Any) -> list[dict[str, Any]]:
        """Executes a SQLAlchemy statement and extracts rows as standard Python dictionaries."""
        exec_fn = getattr(self.sa, "execute", None)
        connect_fn = getattr(self.sa, "connect", None)
        if callable(exec_fn):
            result = exec_fn(statement)
        elif callable(connect_fn):
            with connect_fn() as conn:
                result = conn.execute(statement)
        else:
            raise TypeError(f"Cannot execute statement on {type(self.sa)}")

        records: list[dict[str, Any]] = []
        try:
            # Check for standard DB-API cursor with description (e.g. DuckDB, SQLite cursor)
            desc = getattr(result, "description", None)
            if desc:
                col_names = [d[0] for d in desc]
                fetchall_fn = getattr(result, "fetchall", None)
                rows = fetchall_fn() if callable(fetchall_fn) else list(result)
                for r in rows:
                    if isinstance(r, (tuple, list)):
                        records.append(dict(zip(col_names, r, strict=False)))
                    elif isinstance(r, dict):
                        records.append(r)
                return records

            for row in result:
                row_dict: dict[str, Any] = {}
                if hasattr(row, "_mapping"):
                    for k, val in row._mapping.items():
                        if hasattr(val, "__table__"):
                            for c in val.__table__.columns:
                                row_dict[c.name] = getattr(val, c.name, None)
                        elif hasattr(val, "__dict__") and not isinstance(
                            val, (int, float, str, bool, list, dict)
                        ):
                            for prop_k, prop_v in val.__dict__.items():
                                if not prop_k.startswith("_"):
                                    row_dict[prop_k] = prop_v
                        else:
                            row_dict[k] = val
                elif hasattr(row, "__table__"):
                    for c in row.__table__.columns:
                        row_dict[c.name] = getattr(row, c.name, None)
                elif hasattr(row, "_asdict"):
                    row_dict = row._asdict()
                elif isinstance(row, dict):
                    row_dict = dict(row)
                else:
                    if isinstance(row, (tuple, list)):
                        if len(row) == 1 and hasattr(row[0], "__table__"):
                            for c in row[0].__table__.columns:
                                row_dict[c.name] = getattr(row[0], c.name, None)
                        else:
                            for idx, val in enumerate(row):
                                row_dict[f"col_{idx}"] = val
                    else:
                        row_dict = {"value": row}
                records.append(row_dict)
        except Exception:
            pass

        return records

    @staticmethod
    def _resolve_key_name(key: str | Any) -> str:
        """Resolves the string attribute name of a column, field, or property."""
        if isinstance(key, str):
            return key
        if hasattr(key, "field_name"):
            return str(key.field_name)
        if hasattr(key, "name"):
            return str(key.name)
        if hasattr(key, "key"):
            return str(key.key)
        return str(key)

    def _execute_graph_query_with_keys(
        self,
        query: Any,
        graph_join_key: Any,
        keys: list[Any],
    ) -> pl.DataFrame:
        """Executes a graph query filtering on the supplied key list."""
        if query is None:
            return pl.DataFrame()

        if callable(query):
            built_query = query(keys)
            return self.graph.execute_to_polars(built_query)

        if isinstance(query, (Query, CompiledQuery)):
            return self.graph.execute_to_polars(query)
        else:
            return self.graph.execute_to_polars(str(query), {"keys": keys} if keys else None)


class AsyncHybridSession:
    """Coordinates non-blocking asynchronous relational and graph execution pipelines."""

    def __init__(
        self,
        sa_async_session_or_engine: Any,
        graph_async_session: AsyncSession,
    ) -> None:
        """Initializes an AsyncHybridSession.

        Args:
            sa_async_session_or_engine: SQLAlchemy AsyncSession, AsyncEngine, or standard Session.
            graph_async_session: Voyager AsyncSession.
        """
        self.sa = sa_async_session_or_engine
        self.graph = graph_async_session

    async def __aenter__(self) -> AsyncHybridSession:
        """Async context manager entry."""
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Async context manager exit with transaction coordination."""
        if exc_type is not None:
            await self.rollback()
        else:
            await self.commit()

    async def commit(self) -> None:
        """Asynchronously commits both relational and graph transactions."""
        import inspect

        commit_sa = getattr(self.sa, "commit", None)
        if callable(commit_sa):
            res = commit_sa()
            if inspect.isawaitable(res):
                await res
        commit_graph = getattr(self.graph, "commit", None)
        if callable(commit_graph):
            res = commit_graph()
            if inspect.isawaitable(res):
                await res

    async def rollback(self) -> None:
        """Asynchronously rolls back both relational and graph transactions."""
        import inspect

        rollback_sa = getattr(self.sa, "rollback", None)
        if callable(rollback_sa):
            res = rollback_sa()
            if inspect.isawaitable(res):
                await res
        rollback_graph = getattr(self.graph, "rollback", None)
        if callable(rollback_graph):
            res = rollback_graph()
            if inspect.isawaitable(res):
                await res

    async def query_graph_from_relational(
        self,
        sa_statement: Any,
        relational_key: str | Any,
        graph_query_fn: Callable[[list[Any]], Query | CompiledQuery | str],
        join_column: str | None = None,
    ) -> pl.DataFrame:
        """Asynchronously executes a SQLAlchemy query, then queries the graph with extracted keys."""
        rel_records = await self._fetch_relational_dicts_async(sa_statement)
        if not rel_records:
            return pl.DataFrame()

        key_name = HybridSession._resolve_key_name(relational_key)
        join_col_name = join_column or key_name
        keys = [r[key_name] for r in rel_records if key_name in r and r[key_name] is not None]
        if not keys:
            return pl.DataFrame(rel_records)

        g_query = graph_query_fn(keys)
        graph_df = await self.graph.execute_to_polars(g_query)

        if graph_df.is_empty():
            return pl.DataFrame(rel_records)

        rel_df = pl.DataFrame(rel_records)

        if key_name in rel_df.columns and join_col_name in graph_df.columns:
            return rel_df.join(graph_df, left_on=key_name, right_on=join_col_name, how="inner")
        elif join_col_name in rel_df.columns and join_col_name in graph_df.columns:
            return rel_df.join(graph_df, on=join_col_name, how="inner")
        elif key_name in rel_df.columns and key_name in graph_df.columns:
            return rel_df.join(graph_df, on=key_name, how="inner")
        return graph_df

    async def _fetch_relational_dicts_async(self, statement: Any) -> list[dict[str, Any]]:
        """Asynchronously executes a SQLAlchemy query and extracts records as dicts."""
        if hasattr(self.sa, "execute"):
            exec_res = self.sa.execute(statement)
            result = await exec_res if hasattr(exec_res, "__await__") else exec_res
        else:
            conn_ctx = self.sa.connect()
            if hasattr(conn_ctx, "__aenter__"):
                async with conn_ctx as conn:
                    res = conn.execute(statement)
                    result = await res if hasattr(res, "__await__") else res
            else:
                with conn_ctx as conn:
                    result = conn.execute(statement)

        records: list[dict[str, Any]] = []
        try:
            for row in result:
                row_dict: dict[str, Any] = {}
                if hasattr(row, "_mapping"):
                    for k, val in row._mapping.items():
                        if hasattr(val, "__table__"):
                            for c in val.__table__.columns:
                                row_dict[c.name] = getattr(val, c.name, None)
                        elif hasattr(val, "__dict__") and not isinstance(
                            val, (int, float, str, bool, list, dict)
                        ):
                            for prop_k, prop_v in val.__dict__.items():
                                if not prop_k.startswith("_"):
                                    row_dict[prop_k] = prop_v
                        else:
                            row_dict[k] = val
                elif hasattr(row, "__table__"):
                    for c in row.__table__.columns:
                        row_dict[c.name] = getattr(row, c.name, None)
                elif hasattr(row, "_asdict"):
                    row_dict = row._asdict()
                elif isinstance(row, dict):
                    row_dict = dict(row)
                else:
                    if isinstance(row, (tuple, list)):
                        if len(row) == 1 and hasattr(row[0], "__table__"):
                            for c in row[0].__table__.columns:
                                row_dict[c.name] = getattr(row[0], c.name, None)
                        else:
                            for idx, val in enumerate(row):
                                row_dict[f"col_{idx}"] = val
                    else:
                        row_dict = {"value": row}
                records.append(row_dict)
        except Exception:
            pass
        return records
