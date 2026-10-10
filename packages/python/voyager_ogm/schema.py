"""Voyager OGM Schema & Graph Types DDL Engine.

Provides automated constraint generation, schema reflection, index management,
and Graph Types validation for openCypher (Neo4j / Memgraph), ISO GQL, and SQL:2023 PGQ.
All DDL statements are deterministically emitted via native voyager-core Rust emitters.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import TYPE_CHECKING, Any, Literal

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
    emit_node_constraint_ddl,
    emit_node_drop_constraint_ddl,
    emit_node_drop_index_ddl,
    emit_node_index_ddl,
    emit_pgq_drop_property_graph_ddl,
    emit_pgq_property_graph_ddl,
    emit_rel_constraint_ddl,
    emit_rel_drop_constraint_ddl,
    emit_rel_drop_index_ddl,
    emit_rel_index_ddl,
)

if TYPE_CHECKING:
    from voyager_ogm.models import Node, Relationship
    from voyager_ogm.session import Session

# Ergonomic alias for the centralized thread-safe native schema registry
SchemaRegistry = NativeSchemaRegistry


def _is_relationship_model(model: Any) -> bool:
    """Returns True if the model class or schema dict describes a Relationship entity."""
    return (
        hasattr(model, "__type__")
        or (
            hasattr(model, "__mro__")
            and any(b.__name__ == "Relationship" for b in getattr(model, "__mro__", []))
        )
        or (
            isinstance(model, dict)
            and any(
                k in model
                for k in (
                    "type_name",
                    "type_",
                    "type",
                    "source_labels",
                    "target_labels",
                    "from_labels",
                    "to_labels",
                )
            )
        )
    )


def _split_models(
    models: Sequence[type[Node] | type[Relationship] | dict[str, Any]],
) -> tuple[list[Any], list[Any]]:
    """Partitions a collection of models into (nodes, relationships)."""
    nodes: list[Any] = []
    rels: list[Any] = []
    for m in models:
        if _is_relationship_model(m):
            rels.append(m)
        else:
            nodes.append(m)
    return nodes, rels


def _resolve_model_specs(
    *models: type[Node] | type[Relationship] | dict[str, Any] | str,
) -> list[tuple[Literal["node", "rel"], Any]]:
    """Resolves model references (classes, registered names, or schema dicts) into (kind, spec) tuples.

    Args:
        *models: Model classes, registered string names, or schema dicts.

    Returns:
        List of ('node' | 'rel', spec) tuples ready for native Rust emitter delegation.
    """
    resolved: list[tuple[Literal["node", "rel"], Any]] = []
    reg = SchemaRegistry.global_registry()
    if not models:
        for ns in reg.node_schemas():
            resolved.append(("node", ns))
        for rs in reg.relationship_schemas():
            resolved.append(("rel", rs))
        return resolved
    for m in models:
        if isinstance(m, str):
            node_schema = reg.get_node(m)
            if node_schema:
                resolved.append(("node", node_schema))
                continue
            rel_schema = reg.get_relationship(m)
            if rel_schema:
                resolved.append(("rel", rel_schema))
                continue
            raise ValueError(f"Model '{m}' is not registered in SchemaRegistry")
        if isinstance(m, dict):
            if any(
                k in m
                for k in (
                    "type_name",
                    "type_",
                    "type",
                    "source_labels",
                    "target_labels",
                    "from_labels",
                    "to_labels",
                )
            ):
                resolved.append(("rel", m))
            else:
                resolved.append(("node", m))
            continue
        if _is_relationship_model(m):
            resolved.append(("rel", m))
        else:
            resolved.append(("node", m))
    return resolved


class SchemaManager:
    """Manages schema constraints, indexes, graph types, and DDL migrations."""

    @staticmethod
    def generate_cypher_ddl(
        model: type[Node] | type[Relationship] | dict[str, Any] | str,
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates openCypher / Neo4j constraint and index creation DDL statements.

        Supports:
        - Unique constraints (REQUIRE n.prop IS UNIQUE)
        - Not Null / Existence constraints (REQUIRE n.prop IS NOT NULL)
        - Property Type Constraints (Neo4j 5.x Graph Types: REQUIRE n.prop :: STRING)
        - Secondary Indexes (FOR (n:Label) ON (n.prop))

        Args:
            model: Target Node or Relationship model class, schema dict, or registered model name.
            include_type_constraints: Whether to emit Neo4j 5.x Enterprise Property Type
                constraints (:: STRING, :: INTEGER). Defaults to False.

        Returns:
            List of executable DDL Cypher statement strings.
        """
        if isinstance(model, str):
            registry = SchemaRegistry.global_registry()
            return registry.generate_cypher_ddl(
                include_type_constraints=include_type_constraints, model_name=model
            )
        if _is_relationship_model(model):
            return emit_cypher_rel_ddl(model, include_type_constraints)
        return emit_cypher_node_ddl(model, include_type_constraints)

    @staticmethod
    def generate_drop_ddl(
        model: type[Node] | type[Relationship] | dict[str, Any] | str,
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates drop statements for constraints and indexes.

        Args:
            model: Target Node or Relationship model class, schema dict, or registered model name.
            include_type_constraints: Whether to emit drop statements for Property Type constraints.

        Returns:
            List of executable DROP statement strings.
        """
        if isinstance(model, str):
            registry = SchemaRegistry.global_registry()
            return registry.generate_cypher_drop_ddl(
                include_type_constraints=include_type_constraints, model_name=model
            )
        if _is_relationship_model(model):
            return emit_cypher_drop_rel_ddl(model, include_type_constraints)
        return emit_cypher_drop_node_ddl(model, include_type_constraints)

    @classmethod
    def _validate_cypher_dialect(cls, session: Session, op_name: str) -> None:
        """Validates that the session dialect supports openCypher constraint/drop DDL."""
        dialect = getattr(session, "dialect", "cypher").lower()
        is_drop = "drop" in op_name.lower()
        verb = "DROP CONSTRAINT" if is_drop else "CREATE CONSTRAINT"
        if dialect in ("sql_pgq", "pgq", "duckpgq", "duckdb", "postgres", "postgresql"):
            action = (
                "drop_property_graph(session, graph_name)"
                if is_drop
                else "create_property_graph(session, graph_name, *models)"
            )
            gen_action = (
                "generate_pgq_drop_ddl(graph_name)"
                if is_drop
                else "generate_pgq_ddl(graph_name, *models)"
            )
            raise NotImplementedError(
                f"SchemaManager.{op_name}() generates openCypher {'DROP' if is_drop else 'constraint'} statements. "
                f"For {dialect.upper()} (SQL:2023 PGQ), use SchemaManager.{action} "
                f"or SchemaManager.{gen_action}."
            )
        if dialect in ("falkordb", "falkor"):
            cmd = "GRAPH.CONSTRAINT DROP" if is_drop else "GRAPH.CONSTRAINT CREATE"
            raise NotImplementedError(
                f"FalkorDB does not support openCypher '{verb}' queries"
                f"{'' if is_drop else ' in GRAPH.QUERY'}. "
                f"{'Drop constraints' if is_drop else 'Constraints in FalkorDB must be created'} "
                f"via native Redis commands ('{cmd}')."
            )
        if dialect in ("age", "apache_age"):
            raise NotImplementedError(
                f"Apache AGE does not support Cypher '{verb}' queries. "
                f"Constraints in Apache AGE must be {'dropped' if is_drop else 'defined'} "
                f"on the underlying PostgreSQL relational tables."
            )

    @classmethod
    def generate_ddl(
        cls,
        model: type[Node] | type[Relationship] | dict[str, Any] | str,
        dialect: str = "cypher",
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates DDL statements for the specified model and dialect.

        Delegates directly to native voyager-core Rust emitters.

        Args:
            model: Target Node or Relationship model class, schema dict, or model name.
            dialect: Target dialect ('cypher', 'gql', 'pgq'). Defaults to 'cypher'.
            include_type_constraints: Whether to include Property Type constraints.

        Returns:
            List of executable DDL statement strings.
        """
        dialect_norm = dialect.lower()
        if dialect_norm in ("cypher", "opencypher", "neo4j", "memgraph"):
            return cls.generate_cypher_ddl(model, include_type_constraints=include_type_constraints)
        if dialect_norm in ("gql", "iso_gql"):
            if isinstance(model, str):
                reg = SchemaRegistry.global_registry()
                if reg.has_node(model):
                    stmt = reg.generate_gql_alter_node_ddl(model)
                    return [stmt] if stmt else []
                if reg.has_relationship(model):
                    stmt = reg.generate_gql_alter_rel_ddl(model)
                    return [stmt] if stmt else []
                return []
            return [cls.generate_alter_graph_type_ddl(model)]
        if dialect_norm in ("pgq", "sql_pgq", "duckpgq", "postgres", "postgresql", "duckdb"):
            if isinstance(model, str):
                reg = SchemaRegistry.global_registry()
                return [reg.generate_pgq_ddl(model, [model])]
            graph_name = getattr(model, "__name__", "voyager_graph")
            return [cls.generate_pgq_ddl(graph_name, model)]
        return cls.generate_cypher_ddl(model, include_type_constraints=include_type_constraints)

    @classmethod
    def generate_index_ddl(
        cls,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        dialect: str = "cypher",
    ) -> list[str]:
        """Generates CREATE INDEX DDL statements without executing them.

        Supports multi-dialect generation via native Rust emitters (RFC-0004 §4.4):
        - 'cypher': openCypher / Neo4j 5+ index syntax
        - 'memgraph': Memgraph vector index and openCypher index syntax
        - 'duckdb': DuckDB vss HNSW vector index and relational SQL index syntax
        - 'postgres' / 'pgq': standard SQL relational index syntax
        - 'falkordb': FalkorDB Cypher index syntax
        - 'age': Apache AGE relational index syntax

        Args:
            *models: Node and Relationship model classes, schema dicts, or registered model names.
            dialect: Target dialect ('cypher', 'memgraph', 'duckdb', 'postgres', 'falkordb', 'age', 'pgq'). Defaults to 'cypher'.

        Returns:
            List of executable CREATE INDEX statement strings.
        """
        stmts: list[str] = []
        for kind, spec in _resolve_model_specs(*models):
            if kind == "node":
                stmts.extend(emit_node_index_ddl(spec, dialect))
            else:
                stmts.extend(emit_rel_index_ddl(spec, dialect))
        return stmts

    @classmethod
    def generate_constraint_ddl(
        cls,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        dialect: str = "cypher",
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates CREATE CONSTRAINT DDL statements without executing them.

        Supports multi-dialect generation via native Rust emitters (RFC-0004 §4.4):
        - 'cypher': openCypher / Neo4j UNIQUE, NOT NULL, and Property Type constraints
        - 'postgres' / 'duckdb' / 'pgq': standard SQL UNIQUE table constraints

        Args:
            *models: Node and Relationship model classes, schema dicts, or registered model names.
            dialect: Target dialect ('cypher', 'postgres', etc.). Defaults to 'cypher'.
            include_type_constraints: Whether to include Property Type constraints (:: STRING).

        Returns:
            List of executable CREATE CONSTRAINT statement strings.
        """
        stmts: list[str] = []
        for kind, spec in _resolve_model_specs(*models):
            if kind == "node":
                stmts.extend(emit_node_constraint_ddl(spec, dialect, include_type_constraints))
            else:
                stmts.extend(emit_rel_constraint_ddl(spec, dialect, include_type_constraints))
        return stmts

    @classmethod
    def generate_drop_index_ddl(
        cls,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        dialect: str = "cypher",
    ) -> list[str]:
        """Generates DROP INDEX DDL statements without executing them.

        Supports multi-dialect generation via native Rust emitters (RFC-0004 §4.4).

        Args:
            *models: Node and Relationship model classes, schema dicts, or registered model names.
            dialect: Target dialect ('cypher', 'postgres', 'falkordb', etc.). Defaults to 'cypher'.

        Returns:
            List of executable DROP INDEX statement strings.
        """
        stmts: list[str] = []
        for kind, spec in _resolve_model_specs(*models):
            if kind == "node":
                stmts.extend(emit_node_drop_index_ddl(spec, dialect))
            else:
                stmts.extend(emit_rel_drop_index_ddl(spec, dialect))
        return stmts

    @classmethod
    def generate_drop_constraint_ddl(
        cls,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        dialect: str = "cypher",
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates DROP CONSTRAINT DDL statements without executing them.

        Supports multi-dialect generation via native Rust emitters (RFC-0004 §4.4).

        Args:
            *models: Node and Relationship model classes, schema dicts, or registered model names.
            dialect: Target dialect ('cypher', 'postgres', etc.). Defaults to 'cypher'.
            include_type_constraints: Whether to include Property Type constraints.

        Returns:
            List of executable DROP CONSTRAINT statement strings.
        """
        stmts: list[str] = []
        for kind, spec in _resolve_model_specs(*models):
            if kind == "node":
                stmts.extend(emit_node_drop_constraint_ddl(spec, dialect, include_type_constraints))
            else:
                stmts.extend(emit_rel_drop_constraint_ddl(spec, dialect, include_type_constraints))
        return stmts

    @classmethod
    def create_constraints(
        cls,
        session: Session,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Applies only CONSTRAINT statements to the database session.

        Args:
            session: Active Voyager database session.
            *models: Model classes, schema dicts, or model names.
            include_type_constraints: Whether to include Property Type constraints.

        Returns:
            List of executed constraint queries.
        """
        if hasattr(session, "create_constraints"):
            return session.create_constraints(
                *models, include_type_constraints=include_type_constraints
            )
        dialect = getattr(session, "dialect", "cypher")
        applied = cls.generate_constraint_ddl(
            *models, dialect=dialect, include_type_constraints=include_type_constraints
        )
        for stmt in applied:
            session.execute(stmt)
        return applied

    @classmethod
    def create_indexes(
        cls,
        session: Session,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
    ) -> list[str]:
        """Applies only INDEX creation statements to the database session.

        Args:
            session: Active Voyager database session.
            *models: Model classes, schema dicts, or model names.

        Returns:
            List of executed index queries.
        """
        if hasattr(session, "create_indexes"):
            return session.create_indexes(*models)
        dialect = getattr(session, "dialect", "cypher")
        applied = cls.generate_index_ddl(*models, dialect=dialect)
        for stmt in applied:
            session.execute(stmt)
        return applied

    @classmethod
    def drop_constraints(
        cls,
        session: Session,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Drops only CONSTRAINT statements from the database session.

        Args:
            session: Active Voyager database session.
            *models: Model classes, schema dicts, or model names.
            include_type_constraints: Whether to drop Property Type constraints.

        Returns:
            List of executed drop constraint queries.
        """
        if hasattr(session, "drop_constraints"):
            return session.drop_constraints(
                *models, include_type_constraints=include_type_constraints
            )
        dialect = getattr(session, "dialect", "cypher")
        dropped = cls.generate_drop_constraint_ddl(
            *models, dialect=dialect, include_type_constraints=include_type_constraints
        )
        for stmt in dropped:
            session.execute(stmt)
        return dropped

    @classmethod
    def drop_indexes(
        cls,
        session: Session,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
    ) -> list[str]:
        """Drops only INDEX statements from the database session.

        Args:
            session: Active Voyager database session.
            *models: Model classes, schema dicts, or model names.

        Returns:
            List of executed drop index queries.
        """
        if hasattr(session, "drop_indexes"):
            return session.drop_indexes(*models)
        dialect = getattr(session, "dialect", "cypher")
        dropped = cls.generate_drop_index_ddl(*models, dialect=dialect)
        for stmt in dropped:
            session.execute(stmt)
        return dropped

    @classmethod
    def apply_schema(
        cls,
        session: Session,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Applies declarative constraints and indexes (including vector indexes) to the session.

        If no models are provided, discovers and applies schema for all models registered
        in the global SchemaRegistry.

        Args:
            session: Active database session.
            *models: Optional Node and Relationship model classes, schema dicts, or model names.
            include_type_constraints: Whether to include Property Type constraints (e.g. :: STRING).

        Returns:
            List of executed DDL query statement strings.
        """
        dialect = getattr(session, "dialect", getattr(session, "_dialect", "cypher"))
        applied: list[str] = []
        try:
            c_stmts = cls.generate_constraint_ddl(
                *models, dialect=dialect, include_type_constraints=include_type_constraints
            )
            for stmt in c_stmts:
                session.execute(stmt)
                applied.append(stmt)
        except NotImplementedError:
            pass

        i_stmts = cls.generate_index_ddl(*models, dialect=dialect)
        for stmt in i_stmts:
            session.execute(stmt)
            applied.append(stmt)
        return applied

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
        cls._validate_cypher_dialect(session, "create_all")
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
        cls._validate_cypher_dialect(session, "drop_all")
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
        model: type[Node] | type[Relationship] | dict[str, Any],
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
        if _is_relationship_model(model):
            src_str = getattr(source_node, "__name__", str(source_node)) if source_node else None
            tgt_str = getattr(target_node, "__name__", str(target_node)) if target_node else None
            return emit_gql_alter_rel_ddl(model, src_str, tgt_str)
        return emit_gql_alter_node_ddl(model)

    @classmethod
    def generate_cypher25_graph_type_ddl(
        cls,
        *models: type[Node] | type[Relationship] | dict[str, Any],
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
        nodes, rels = _split_models(models)
        return emit_cypher25_graph_type_ddl(nodes, rels)

    @staticmethod
    def generate_cypher25_drop_graph_type_ddl() -> str:
        """Emits Neo4j Cypher 25 `ALTER CURRENT GRAPH TYPE SET {}` statement to reset the graph type."""
        return emit_cypher25_drop_graph_type_ddl()

    @classmethod
    def generate_gql_graph_type_ddl(
        cls,
        graph_type_name: str,
        *models: type[Node] | type[Relationship] | dict[str, Any],
    ) -> str:
        """Generates standard ISO GQL `CREATE GRAPH TYPE <name> AS { ... }` definition.

        Args:
            graph_type_name: Identifier name for the Graph Type.
            *models: Node and Relationship model classes to include in the schema.

        Returns:
            Executable ISO GQL `CREATE GRAPH TYPE` statement.
        """
        nodes, rels = _split_models(models)
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
        *models: type[Node] | type[Relationship] | dict[str, Any],
    ) -> str:
        """Generates standard SQL:2023 PGQ / DuckPGQ `CREATE PROPERTY GRAPH` statement.

        Args:
            graph_name: Identifier name for the Property Graph.
            *models: Node and Relationship model classes to include.

        Returns:
            Executable `CREATE PROPERTY GRAPH` statement.
        """
        nodes, rels = _split_models(models)
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
