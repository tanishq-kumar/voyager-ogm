"""Voyager OGM Schema & Graph Types DDL Engine.

Provides automated constraint generation, schema reflection, index management,
and Graph Types validation for openCypher (Neo4j / Memgraph), ISO GQL, and SQL:2023 PGQ.
All DDL statements are deterministically emitted via native voyager-core Rust emitters.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from voyager_ogm._voyager_rs import (
    NativeSchemaRegistry,
    emit_cypher25_drop_graph_type_ddl,
    emit_cypher25_graph_type_ddl,
    emit_cypher_drop_node_ddl,
    emit_cypher_drop_rel_ddl,
    emit_cypher_node_ddl,
    emit_cypher_rel_ddl,
    emit_gql_alter_node_ddl,
    emit_gql_alter_rel_ddl,
    emit_gql_drop_graph_type_ddl,
    emit_gql_graph_type_ddl,
    emit_pgq_drop_property_graph_ddl,
    emit_pgq_property_graph_ddl,
)

if TYPE_CHECKING:
    from voyager_ogm.models import Node, Relationship
    from voyager_ogm.session import Session

# Ergonomic alias for the centralized thread-safe native schema registry
SchemaRegistry = NativeSchemaRegistry


class SchemaManager:
    """Manages schema constraints, indexes, graph types, and DDL migrations."""

    @staticmethod
    def generate_cypher_ddl(
        model: type[Node] | type[Relationship] | dict[str, Any],
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates openCypher / Neo4j constraint and index creation DDL statements.

        Supports:
        - Unique constraints (REQUIRE n.prop IS UNIQUE)
        - Not Null / Existence constraints (REQUIRE n.prop IS NOT NULL)
        - Property Type Constraints (Neo4j 5.x Graph Types: REQUIRE n.prop :: STRING)
        - Secondary Indexes (FOR (n:Label) ON (n.prop))

        Args:
            model: Target Node or Relationship model class, or schema dict.
            include_type_constraints: Whether to emit Neo4j 5.x Enterprise Property Type
                constraints (:: STRING, :: INTEGER). Defaults to False.

        Returns:
            List of executable DDL Cypher statement strings.
        """
        if (
            hasattr(model, "__type__")
            or (
                hasattr(model, "__mro__")
                and any(b.__name__ == "Relationship" for b in getattr(model, "__mro__", []))
            )
            or (isinstance(model, dict) and ("type_name" in model or "type" in model))
        ):
            return emit_cypher_rel_ddl(model, include_type_constraints)
        return emit_cypher_node_ddl(model, include_type_constraints)

    @staticmethod
    def generate_drop_ddl(
        model: type[Node] | type[Relationship] | dict[str, Any],
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates drop statements for constraints and indexes.

        Args:
            model: Target Node or Relationship model class, or schema dict.
            include_type_constraints: Whether to emit drop statements for Property Type constraints.

        Returns:
            List of executable DROP statement strings.
        """
        if (
            hasattr(model, "__type__")
            or (
                hasattr(model, "__mro__")
                and any(b.__name__ == "Relationship" for b in getattr(model, "__mro__", []))
            )
            or (isinstance(model, dict) and ("type_name" in model or "type" in model))
        ):
            return emit_cypher_drop_rel_ddl(model, include_type_constraints)
        return emit_cypher_drop_node_ddl(model, include_type_constraints)

    @classmethod
    def create_all(
        cls,
        session: Session,
        *models: type[Node] | type[Relationship],
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Applies all generated DDL constraint statements to the database session.

        Args:
            session: Active Voyager database session.
            *models: Model classes to generate and apply constraints for.
            include_type_constraints: Whether to include Property Type constraints.

        Returns:
            List of executed DDL queries.

        Raises:
            NotImplementedError: If the session dialect does not support openCypher constraint DDL.
        """
        dialect = getattr(session, "dialect", "cypher").lower()
        if dialect in ("sql_pgq", "pgq", "duckpgq", "duckdb", "postgres", "postgresql"):
            raise NotImplementedError(
                f"SchemaManager.create_all() generates openCypher constraint DDL. "
                f"For {dialect.upper()} (SQL:2023 PGQ), use SchemaManager.create_property_graph(session, graph_name, *models) "
                f"or SchemaManager.generate_pgq_ddl(graph_name, *models)."
            )
        if dialect in ("falkordb", "falkor"):
            raise NotImplementedError(
                "FalkorDB does not support openCypher 'CREATE CONSTRAINT' queries in GRAPH.QUERY. "
                "Constraints in FalkorDB must be created via native Redis commands ('GRAPH.CONSTRAINT CREATE')."
            )
        if dialect in ("age", "apache_age"):
            raise NotImplementedError(
                "Apache AGE does not support Cypher 'CREATE CONSTRAINT' queries. "
                "Constraints in Apache AGE must be defined on the underlying PostgreSQL relational tables."
            )

        applied: list[str] = []
        for model in models:
            for stmt in cls.generate_cypher_ddl(
                model, include_type_constraints=include_type_constraints
            ):
                session.execute(stmt)
                applied.append(stmt)
        return applied

    @classmethod
    def drop_all(
        cls,
        session: Session,
        *models: type[Node] | type[Relationship],
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Drops all constraints and indexes for the specified models.

        Args:
            session: Active Voyager database session.
            *models: Model classes to drop constraints for.
            include_type_constraints: Whether to drop Property Type constraints.

        Returns:
            List of executed DROP queries.

        Raises:
            NotImplementedError: If the session dialect does not support openCypher DROP constraint DDL.
        """
        dialect = getattr(session, "dialect", "cypher").lower()
        if dialect in ("sql_pgq", "pgq", "duckpgq", "duckdb", "postgres", "postgresql"):
            raise NotImplementedError(
                f"SchemaManager.drop_all() generates openCypher DROP statements. "
                f"For {dialect.upper()} (SQL:2023 PGQ), use SchemaManager.drop_property_graph(session, graph_name) "
                f"or SchemaManager.generate_pgq_drop_ddl(graph_name)."
            )
        if dialect in ("falkordb", "falkor"):
            raise NotImplementedError(
                "FalkorDB does not support openCypher 'DROP CONSTRAINT' queries. "
                "Drop constraints via native Redis commands ('GRAPH.CONSTRAINT DROP')."
            )
        if dialect in ("age", "apache_age"):
            raise NotImplementedError(
                "Apache AGE does not support Cypher 'DROP CONSTRAINT' queries. "
                "Constraints in Apache AGE must be dropped on the underlying PostgreSQL relational tables."
            )

        dropped: list[str] = []
        for model in models:
            for stmt in cls.generate_drop_ddl(
                model, include_type_constraints=include_type_constraints
            ):
                session.execute(stmt)
                dropped.append(stmt)
        return dropped

    @classmethod
    def create_property_graph(
        cls,
        session: Session,
        graph_name: str,
        *models: type[Node] | type[Relationship],
    ) -> str:
        """Applies SQL:2023 PGQ `CREATE PROPERTY GRAPH` DDL to the active session.

        Args:
            session: Active database session (PostgreSQL 19 / DuckPGQ).
            graph_name: Identifier name for the property graph.
            *models: Model classes to register.

        Returns:
            The executed DDL statement.
        """
        ddl = cls.generate_pgq_ddl(graph_name, *models)
        session.execute(ddl)
        return ddl

    @classmethod
    def drop_property_graph(
        cls,
        session: Session,
        graph_name: str,
    ) -> str:
        """Drops a SQL:2023 PGQ property graph from the active session.

        Args:
            session: Active database session.
            graph_name: Identifier name of property graph to drop.

        Returns:
            The executed DROP statement.
        """
        ddl = cls.generate_pgq_drop_ddl(graph_name)
        session.execute(ddl)
        return ddl

    @staticmethod
    def generate_alter_graph_type_ddl(
        model: type[Node] | type[Relationship],
        source_node: type[Node] | str | None = None,
        target_node: type[Node] | str | None = None,
    ) -> str:
        """[Experimental] Generates Cypher 25 / ISO GQL `ALTER CURRENT GRAPH TYPE ADD ...` DDL statement.

        Note: Cypher 25 Graph Types are an experimental draft feature currently
        previewed in Neo4j 5.26+.

        Supports:
        - `ALTER CURRENT GRAPH TYPE ADD NODE TYPE (:Label {prop :: TYPE, ...})`
        - `ALTER CURRENT GRAPH TYPE ADD RELATIONSHIP TYPE (:Source)-[:TYPE {prop :: TYPE}]->(:Target)`

        Args:
            model: Target Node or Relationship model class.
            source_node: Source Node class or label string (for Relationship types).
            target_node: Target Node class or label string (for Relationship types).

        Returns:
            Executable `ALTER CURRENT GRAPH TYPE` DDL string.
        """
        if hasattr(model, "__type__") or (
            hasattr(model, "__mro__")
            and any(b.__name__ == "Relationship" for b in getattr(model, "__mro__", []))
        ):
            src_str = getattr(source_node, "__name__", str(source_node)) if source_node else None
            tgt_str = getattr(target_node, "__name__", str(target_node)) if target_node else None
            return emit_gql_alter_rel_ddl(model, src_str, tgt_str)
        return emit_gql_alter_node_ddl(model)

    @classmethod
    def generate_cypher25_graph_type_ddl(
        cls,
        *models: type[Node] | type[Relationship],
    ) -> str:
        """Generates Neo4j Cypher 25 `ALTER CURRENT GRAPH TYPE SET { ... }` DDL statement.

        Emits declarative open graph type schema blocks enforcing node element types,
        relationship element types, implied labels, property types, keys (IS KEY),
        uniqueness (IS UNIQUE), and existence (NOT NULL).

        Args:
            *models: Node and Relationship model classes to include in the graph type.

        Returns:
            Executable Cypher 25 `ALTER CURRENT GRAPH TYPE SET { ... }` statement.
        """
        nodes: list[Any] = []
        rels: list[Any] = []
        for m in models:
            if hasattr(m, "__type__") or (
                hasattr(m, "__mro__")
                and any(b.__name__ == "Relationship" for b in getattr(m, "__mro__", []))
            ):
                rels.append(m)
            else:
                nodes.append(m)
        return emit_cypher25_graph_type_ddl(nodes, rels)

    @staticmethod
    def generate_cypher25_drop_graph_type_ddl() -> str:
        """Emits Neo4j Cypher 25 `ALTER CURRENT GRAPH TYPE SET {}` statement to reset the graph type."""
        return emit_cypher25_drop_graph_type_ddl()

    @classmethod
    def generate_gql_graph_type_ddl(
        cls,
        graph_type_name: str,
        *models: type[Node] | type[Relationship],
    ) -> str:
        """Generates standard ISO GQL `CREATE GRAPH TYPE <name> AS { ... }` definition.

        Args:
            graph_type_name: Identifier name for the Graph Type.
            *models: Node and Relationship model classes to include in the schema.

        Returns:
            Executable ISO GQL `CREATE GRAPH TYPE` statement.
        """
        nodes: list[Any] = []
        rels: list[Any] = []
        for m in models:
            if hasattr(m, "__type__") or (
                hasattr(m, "__mro__")
                and any(b.__name__ == "Relationship" for b in getattr(m, "__mro__", []))
            ):
                rels.append(m)
            else:
                nodes.append(m)
        return emit_gql_graph_type_ddl(graph_type_name, nodes, rels)

    @staticmethod
    def generate_gql_drop_graph_type_ddl(graph_type_name: str) -> str:
        """Emits an ISO GQL `DROP GRAPH TYPE` DDL statement.

        Args:
            graph_type_name: Identifier name for the Graph Type to drop.

        Returns:
            Executable ISO GQL `DROP GRAPH TYPE` statement.
        """
        return emit_gql_drop_graph_type_ddl(graph_type_name)

    @classmethod
    def generate_pgq_ddl(
        cls,
        graph_name: str,
        *models: type[Node] | type[Relationship],
    ) -> str:
        """Generates standard SQL:2023 PGQ / DuckPGQ `CREATE PROPERTY GRAPH` statement.

        Args:
            graph_name: Identifier name for the Property Graph.
            *models: Node and Relationship model classes to include.

        Returns:
            Executable `CREATE PROPERTY GRAPH` statement.
        """
        nodes: list[Any] = []
        rels: list[Any] = []
        for m in models:
            if hasattr(m, "__type__") or (
                hasattr(m, "__mro__")
                and any(b.__name__ == "Relationship" for b in getattr(m, "__mro__", []))
            ):
                rels.append(m)
            else:
                nodes.append(m)
        return emit_pgq_property_graph_ddl(graph_name, nodes, rels)

    @staticmethod
    def generate_pgq_drop_ddl(graph_name: str) -> str:
        """Emits a SQL:2023 PGQ / DuckPGQ `DROP PROPERTY GRAPH` DDL statement.

        Args:
            graph_name: Identifier name for the Property Graph to drop.

        Returns:
            Executable `DROP PROPERTY GRAPH` statement.
        """
        return emit_pgq_drop_property_graph_ddl(graph_name)
