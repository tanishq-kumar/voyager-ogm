"""SQLAlchemy Native Property Graph & Hybrid Integration for Voyager OGM.

Provides first-class SQLAlchemy 2.0 integrations:
1. `PropertyGraph`: Auto-derives property graph catalog definitions and DDL from SQLAlchemy `MetaData` / `DeclarativeBase`.
2. `graph_table()` / `GraphTableClause`: First-class SQLAlchemy `FromClause` compiling into ISO SQL:2023 / DuckPGQ `GRAPH_TABLE (...)`.
3. `as_cte()`: Transpiles multi-hop graph traversals into native recursive Common Table Expressions (`WITH RECURSIVE`) for standard SQL engines (SQLite, PostgreSQL < 19).
4. `graph_relationship()`: Declarative ORM model descriptor for graph neighbor queries.
"""

from __future__ import annotations

import re
from collections.abc import Sequence
from typing import TYPE_CHECKING, Any

from voyager_ogm.models import BoundField, Field, Node, Relationship, node, relationship
from voyager_ogm.query import CompiledQuery, Query

if TYPE_CHECKING:
    from sqlalchemy import Select, select, text
    from sqlalchemy.ext.compiler import compiles
    from sqlalchemy.schema import MetaData
    from sqlalchemy.sql.base import ColumnCollection
    from sqlalchemy.sql.elements import ColumnClause, ColumnElement
    from sqlalchemy.sql.selectable import CTE, FromClause

    HAS_SQLALCHEMY = True
else:
    try:
        from sqlalchemy import select, text
        from sqlalchemy.ext.compiler import compiles
        from sqlalchemy.sql.base import ColumnCollection
        from sqlalchemy.sql.elements import ColumnClause, ColumnElement
        from sqlalchemy.sql.selectable import CTE, FromClause

        HAS_SQLALCHEMY = True
    except ImportError:
        HAS_SQLALCHEMY = False

        class FromClause:
            """Fallback FromClause when SQLAlchemy is not installed."""

            pass


class VertexTableDef:
    """Descriptor for a vertex table in a Property Graph catalog."""

    def __init__(
        self,
        table_name: str,
        label: str | None = None,
        key_column: str | None = None,
        properties: Sequence[str] | None = None,
    ) -> None:
        self.table_name = table_name
        self.label = label or self._infer_label(table_name)
        self.key_column = key_column
        self.properties = list(properties) if properties else None

    @staticmethod
    def _infer_label(table_name: str) -> str:
        clean = table_name.rstrip("s").replace("_", " ").title().replace(" ", "")
        return clean or table_name


class EdgeTableDef:
    """Descriptor for an edge table in a Property Graph catalog."""

    def __init__(
        self,
        table_name: str,
        source_table: str,
        source_key: str,
        destination_table: str,
        destination_key: str,
        label: str | None = None,
        properties: Sequence[str] | None = None,
    ) -> None:
        self.table_name = table_name
        self.source_table = source_table
        self.source_key = source_key
        self.destination_table = destination_table
        self.destination_key = destination_key
        self.label = (label or table_name).upper()
        self.properties = list(properties) if properties else None


class PropertyGraph:
    """Represents a Property Graph schema catalog spanning relational tables.

    Can be defined manually or automatically extracted from SQLAlchemy `MetaData` / `DeclarativeBase`.

    Example:
        ```python
        pg = PropertyGraph.from_metadata(Base.metadata, name="social_graph")
        pg.create_all(engine)
        ```
    """

    def __init__(
        self,
        name: str,
        vertex_tables: Sequence[VertexTableDef] | None = None,
        edge_tables: Sequence[EdgeTableDef] | None = None,
    ) -> None:
        self.name = name
        self.vertex_tables = list(vertex_tables) if vertex_tables else []
        self.edge_tables = list(edge_tables) if edge_tables else []

    @classmethod
    def from_metadata(
        cls,
        metadata_or_base: Any,
        name: str = "default_property_graph",
        label_overrides: dict[str, str] | None = None,
    ) -> PropertyGraph:
        """Auto-derives a PropertyGraph schema by introspecting SQLAlchemy `MetaData` or `DeclarativeBase`.

        Tables with primary keys are recognized as Vertex Tables.
        Tables containing foreign key references to vertex tables are mapped as Edge Tables.

        Args:
            metadata_or_base: SQLAlchemy `MetaData` instance or `DeclarativeBase` subclass.
            name: Name of the property graph catalog.
            label_overrides: Optional dict mapping table names to custom Graph labels.

        Returns:
            Configured PropertyGraph instance.
        """
        if not HAS_SQLALCHEMY:
            raise ImportError("SQLAlchemy is required to use PropertyGraph.from_metadata")

        metadata: MetaData = (
            metadata_or_base.metadata if hasattr(metadata_or_base, "metadata") else metadata_or_base
        )

        overrides = label_overrides or {}
        vertices: dict[str, VertexTableDef] = {}
        candidate_edges: list[EdgeTableDef] = []

        for table_name, table in metadata.tables.items():
            pk_cols = [c.name for c in table.primary_key.columns]
            key_col = pk_cols[0] if pk_cols else None
            lbl = overrides.get(table_name, VertexTableDef._infer_label(table_name))
            prop_cols = [c.name for c in table.columns]
            vertices[table_name] = VertexTableDef(
                table_name=table_name,
                label=lbl,
                key_column=key_col,
                properties=prop_cols,
            )

        for table_name, table in metadata.tables.items():
            col_order = {c.name: idx for idx, c in enumerate(table.columns)}
            fks = sorted(table.foreign_keys, key=lambda fk: col_order.get(fk.parent.name, 999))
            if len(fks) >= 2:
                fk_src = fks[0]
                fk_dst = fks[1]
                edge_def = EdgeTableDef(
                    table_name=table_name,
                    source_table=fk_src.column.table.name,
                    source_key=fk_src.parent.name,
                    destination_table=fk_dst.column.table.name,
                    destination_key=fk_dst.parent.name,
                    label=overrides.get(table_name, table_name.upper()),
                    properties=[c.name for c in table.columns],
                )
                candidate_edges.append(edge_def)
            elif len(fks) == 1:
                fk = fks[0]
                pk_cols = [c.name for c in table.primary_key.columns]
                edge_def = EdgeTableDef(
                    table_name=table_name,
                    source_table=fk.column.table.name,
                    source_key=fk.parent.name,
                    destination_table=table_name,
                    destination_key=pk_cols[0] if pk_cols else fk.parent.name,
                    label=overrides.get(table_name, f"HAS_{table_name.upper()}"),
                    properties=[c.name for c in table.columns],
                )
                candidate_edges.append(edge_def)

        return cls(
            name=name,
            vertex_tables=list(vertices.values()),
            edge_tables=candidate_edges,
        )

    def generate_create_ddl(self, dialect: str = "sql_pgq") -> str:
        """Generates SQL:2023 PGQ / DuckPGQ `CREATE PROPERTY GRAPH` DDL statement.

        Args:
            dialect: Target dialect ('sql_pgq' or 'duckpgq').

        Returns:
            SQL DDL query string.
        """
        v_clauses: list[str] = []
        for v in self.vertex_tables:
            v_clauses.append(f"    {v.table_name} LABEL {v.label}")

        e_clauses: list[str] = []
        for e in self.edge_tables:
            e_clauses.append(
                f"    {e.table_name}\n"
                f"      SOURCE KEY ({e.source_key}) REFERENCES {e.source_table}\n"
                f"      DESTINATION KEY ({e.destination_key}) REFERENCES {e.destination_table}\n"
                f"      LABEL {e.label}"
            )

        vertices_block = ",\n".join(v_clauses)
        edges_block = ",\n".join(e_clauses)

        ddl = f"CREATE PROPERTY GRAPH {self.name}\n"
        if vertices_block:
            ddl += f"  VERTEX TABLES (\n{vertices_block}\n  )\n"
        if edges_block:
            ddl += f"  EDGE TABLES (\n{edges_block}\n  )"
        ddl += ";"
        return ddl

    def generate_drop_ddl(self) -> str:
        """Generates `DROP PROPERTY GRAPH` DDL statement."""
        return f"DROP PROPERTY GRAPH IF EXISTS {self.name};"

    def create_all(self, engine_or_conn: Any) -> None:
        """Executes the `CREATE PROPERTY GRAPH` DDL against a live database engine or connection."""
        ddl = self.generate_create_ddl()
        if hasattr(engine_or_conn, "execute"):
            engine_or_conn.execute(text(ddl) if HAS_SQLALCHEMY else ddl)
        else:
            with engine_or_conn.connect() as conn:
                conn.execute(text(ddl))

    def drop_all(self, engine_or_conn: Any) -> None:
        """Executes the `DROP PROPERTY GRAPH` DDL against a live database engine or connection."""
        ddl = self.generate_drop_ddl()
        if hasattr(engine_or_conn, "execute"):
            engine_or_conn.execute(text(ddl) if HAS_SQLALCHEMY else ddl)
        else:
            with engine_or_conn.connect() as conn:
                conn.execute(text(ddl))

    def to_voyager_models(self) -> tuple[dict[str, type[Node]], dict[str, type[Relationship]]]:
        """Dynamically instantiates Voyager `@node` and `@relationship` classes reflecting this graph schema."""
        node_models: dict[str, type[Node]] = {}
        rel_models: dict[str, type[Relationship]] = {}

        for v in self.vertex_tables:
            attrs: dict[str, Any] = {"__annotations__": {}}
            if v.properties:
                for p in v.properties:
                    is_pk = p == v.key_column
                    attrs["__annotations__"][p] = Any
                    attrs[p] = Field(primary_key=is_pk)
            model_cls = type(v.label, (Node,), attrs)
            node_models[v.label] = node(label=v.label)(model_cls)

        for e in self.edge_tables:
            attrs = {"__annotations__": {}}
            if e.properties:
                for p in e.properties:
                    attrs["__annotations__"][p] = Any
                    attrs[p] = Field()
            rel_cls = type(e.label, (Relationship,), attrs)
            rel_models[e.label] = relationship(type_name=e.label)(rel_cls)

        return node_models, rel_models


if HAS_SQLALCHEMY:

    class GraphTableClause(FromClause):
        """SQLAlchemy FromClause representing an ISO SQL:2023 / DuckPGQ `GRAPH_TABLE` table expression."""

        named_with_column = True
        _is_from_container = False

        def __init__(
            self,
            graph_name: str,
            pattern_str: str,
            columns: Sequence[tuple[str, Any] | ColumnElement[Any] | BoundField | str],
            alias: str = "gt",
            where_predicate: str | None = None,
        ) -> None:
            super().__init__()
            self.graph_name = graph_name
            self.pattern_str = pattern_str
            self.where_predicate = where_predicate
            self._name = alias

            def _resolve_expr(val: Any) -> str:
                if hasattr(val, "field_name"):
                    alias_prefix = getattr(val, "target_alias", "")
                    return (
                        f"{alias_prefix}.{val.field_name}" if alias_prefix else str(val.field_name)
                    )
                return str(val)

            norm_cols: list[tuple[str, str]] = []
            for col in columns:
                if isinstance(col, tuple):
                    alias_name = str(col[0])
                    expr_str = _resolve_expr(col[1])
                    norm_cols.append((alias_name, expr_str))
                elif hasattr(col, "name") and hasattr(col, "key"):
                    norm_cols.append((str(col.key), str(col.name)))
                elif hasattr(col, "field_name"):
                    alias_prefix = getattr(col, "target_alias", "")
                    expr_name = (
                        f"{alias_prefix}.{col.field_name}" if alias_prefix else str(col.field_name)
                    )
                    norm_cols.append((str(col.field_name), str(expr_name)))
                else:
                    norm_cols.append((str(col), str(col)))

            self.column_specs = norm_cols
            col_clauses = [ColumnClause(c_alias, _selectable=self) for c_alias, _ in norm_cols]
            self._columns = ColumnCollection((c.key, c) for c in col_clauses)

        @property
        def name(self) -> str:
            """Returns the table alias name."""
            return self._name

        @property
        def _from_objects(self) -> list[Any]:
            return [self]

        @property
        def columns(self) -> Any:
            """Returns the read-only column collection."""
            return self._columns.as_readonly()

        @property
        def c(self) -> Any:
            """Alias for columns collection."""
            return self.columns

    @compiles(GraphTableClause)
    def _compile_graph_table_clause(element: GraphTableClause, compiler: Any, **kw: Any) -> str:
        """Compiles GraphTableClause into ISO SQL:2023 / DuckPGQ `GRAPH_TABLE (...) AS alias`."""
        cols_ddl = ", ".join(
            f"{expr} AS {alias_name}" if expr != alias_name else alias_name
            for alias_name, expr in element.column_specs
        )
        where_clause = f" WHERE {element.where_predicate}" if element.where_predicate else ""
        return (
            f"GRAPH_TABLE ({element.graph_name} "
            f"MATCH {element.pattern_str}{where_clause} "
            f"COLUMNS ({cols_ddl})) AS {element.name}"
        )


def graph_table(
    graph: str | PropertyGraph,
    match: Query | CompiledQuery | str,
    columns: Sequence[tuple[str, str] | ColumnElement[Any] | BoundField | str],
    alias: str = "gt",
    where: str | None = None,
) -> GraphTableClause:
    """Constructs a first-class SQLAlchemy FromClause for SQL:2023 `GRAPH_TABLE (...)`.

    Args:
        graph: Name of the property graph or a PropertyGraph instance.
        match: Voyager `Query` builder, CompiledQuery, or graph path pattern string.
        columns: Sequence of column definitions `(alias, graph_expr)` or BoundField descriptors.
        alias: SQL table alias name (default: 'gt').
        where: Optional graph filter expression.

    Returns:
        GraphTableClause instance usable directly in SQLAlchemy `select().join(...)`.

    Example:
        ```python
        gt = graph_table(
            graph="social_graph",
            match=Query.match(u).to(f).node(friend),
            columns=[("user_id", u.user_id), ("friend_name", friend.name)],
        )
        stmt = select(User.name, gt.c.friend_name).join(gt, User.id == gt.c.user_id)
        ```
    """
    if not HAS_SQLALCHEMY:
        raise ImportError("SQLAlchemy is required to use graph_table")

    graph_name = graph.name if isinstance(graph, PropertyGraph) else str(graph)

    pattern_str = ""
    if isinstance(match, Query):
        compiled = match.compile("sql_pgq", graph_name=graph_name)
        m = re.search(
            r"MATCH\s+(.+?)(?:\s+COLUMNS|\s+RETURN|\s+WHERE|$)",
            compiled.statement,
            re.IGNORECASE | re.DOTALL,
        )
        if m:
            pattern_str = m.group(1).strip()
        else:
            pattern_str = str(match)
    elif isinstance(match, CompiledQuery):
        m = re.search(
            r"MATCH\s+(.+?)(?:\s+COLUMNS|\s+RETURN|\s+WHERE|$)",
            match.statement,
            re.IGNORECASE | re.DOTALL,
        )
        pattern_str = m.group(1).strip() if m else match.statement
    else:
        pattern_str = str(match)

    return GraphTableClause(
        graph_name=graph_name,
        pattern_str=pattern_str,
        columns=columns,
        alias=alias,
        where_predicate=where,
    )


def as_cte(
    edge_table: Any,
    source_col: Any,
    target_col: Any,
    max_hops: int = 3,
    min_hops: int = 1,
    cte_name: str = "graph_traversal_cte",
    weight_col: Any | None = None,
) -> CTE:
    """Transpiles a multi-hop graph traversal into a native SQLAlchemy Recursive CTE (`WITH RECURSIVE`).

    Enables multi-hop graph querying on standard relational engines
    (SQLite, PostgreSQL, MySQL) without requiring native PGQ extensions.

    Args:
        edge_table: SQLAlchemy Table or ORM model representing edges.
        source_col: Column attribute representing edge source ID.
        target_col: Column attribute representing edge target ID.
        max_hops: Maximum traversal depth limit.
        min_hops: Minimum traversal depth (default: 1).
        cte_name: CTE table alias name.
        weight_col: Optional column for computing path weight / costs.

    Returns:
        SQLAlchemy recursive CTE expression with columns `(source_id, target_id, depth)`.

    Example:
        ```python
        cte = as_cte(FollowsTable, FollowsTable.follower_id, FollowsTable.followed_id, max_hops=3)
        stmt = select(User).join(cte, User.id == cte.c.target_id).where(cte.c.source_id == 42)
        ```
    """
    if not HAS_SQLALCHEMY:
        raise ImportError("SQLAlchemy is required to use as_cte")

    from sqlalchemy import Integer, cast, literal

    base_select = (
        select(
            source_col.label("source_id"),
            target_col.label("target_id"),
            cast(literal(1), Integer).label("depth"),
        )
        .select_from(edge_table)
        .cte(name=cte_name, recursive=True)
    )

    recursive_select = (
        select(
            base_select.c.source_id,
            target_col.label("target_id"),
            cast(base_select.c.depth + 1, Integer).label("depth"),
        )
        .select_from(base_select)
        .join(edge_table, base_select.c.target_id == source_col)
        .where(base_select.c.depth < max_hops)
    )

    cte = base_select.union_all(recursive_select)
    return cte


class GraphRelationshipProperty:
    """Declarative descriptor on SQLAlchemy ORM models representing graph relationships."""

    def __init__(
        self,
        target: str | type[Any],
        via_edge_table: Any,
        source_key: Any,
        target_key: Any,
        max_hops: int = 1,
        min_hops: int = 1,
        graph_name: str | None = None,
    ) -> None:
        self.target = target
        self.via_edge_table = via_edge_table
        self.source_key = source_key
        self.target_key = target_key
        self.max_hops = max_hops
        self.min_hops = min_hops
        self.graph_name = graph_name

    def query(
        self,
        instance_or_id: Any,
        target_model: Any | None = None,
    ) -> Select[Any]:
        """Builds a SQLAlchemy `Select` query traversing this graph relationship for a specific entity ID.

        Args:
            instance_or_id: Model instance or scalar ID.
            target_model: Target SQLAlchemy ORM model class.

        Returns:
            SQLAlchemy Select query.
        """
        source_id = getattr(instance_or_id, "id", instance_or_id)
        target_cls = target_model or self.target

        cte = as_cte(
            edge_table=self.via_edge_table,
            source_col=self.source_key,
            target_col=self.target_key,
            max_hops=self.max_hops,
            min_hops=self.min_hops,
        )

        pk_col = getattr(target_cls, "id", self.target_key)
        from typing import cast
        stmt = (
            select(cast(Any, target_cls))
            .join(cte, pk_col == cte.c.target_id)
            .where(cte.c.source_id == source_id)
        )
        return stmt


def graph_relationship(
    target: str | type[Any],
    via_edge_table: Any,
    source_key: Any,
    target_key: Any,
    max_hops: int = 1,
    min_hops: int = 1,
    graph_name: str | None = None,
) -> GraphRelationshipProperty:
    """Defines a declarative graph relationship on a SQLAlchemy ORM model class.

    Args:
        target: Target model class or name.
        via_edge_table: Edge table or ORM class.
        source_key: Column attribute on edge table for source vertex.
        target_key: Column attribute on edge table for target vertex.
        max_hops: Maximum graph traversal hops (default: 1).
        min_hops: Minimum graph traversal hops (default: 1).
        graph_name: Optional Property Graph catalog name.

    Returns:
        GraphRelationshipProperty descriptor.

    Example:
        ```python
        class User(Base):
            __tablename__ = "users"
            id = Column(Integer, primary_key=True)

            friends = graph_relationship(
                target="User",
                via_edge_table=Follows,
                source_key=Follows.follower_id,
                target_key=Follows.followed_id,
                max_hops=2,
            )
        ```
    """
    return GraphRelationshipProperty(
        target=target,
        via_edge_table=via_edge_table,
        source_key=source_key,
        target_key=target_key,
        max_hops=max_hops,
        min_hops=min_hops,
        graph_name=graph_name,
    )
