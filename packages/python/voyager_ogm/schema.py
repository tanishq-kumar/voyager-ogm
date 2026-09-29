"""Voyager OGM Schema & Graph Types DDL Engine.

Provides automated constraint generation, schema reflection, index management,
and Graph Types validation for openCypher (Neo4j / Memgraph), ISO GQL, and SQL:2023 PGQ.
All DDL statements are deterministically emitted via native voyager-core Rust emitters.
"""

from __future__ import annotations

from collections.abc import Sequence
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
            and ("type_name" in model or "type" in model or "type_" in model)
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
        if dialect_norm in ("pgq", "sql_pgq", "duckpgq", "postgres", "postgresql"):
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

        Supports multi-dialect generation:
        - 'cypher': openCypher / Neo4j / Memgraph / FalkorDB index syntax
        - 'postgres' / 'duckdb' / 'pgq': standard SQL relational index syntax

        Args:
            *models: Node and Relationship model classes, schema dicts, or registered model names.
            dialect: Target dialect ('cypher', 'postgres', 'duckdb', 'pgq'). Defaults to 'cypher'.

        Returns:
            List of executable CREATE INDEX statement strings.
        """
        dialect_norm = dialect.lower()
        stmts: list[str] = []

        if dialect_norm in ("age", "apache_age"):
            raise NotImplementedError(
                "Apache AGE does not support Cypher 'CREATE INDEX' queries. "
                "Indexes in Apache AGE must be defined directly on the underlying PostgreSQL relational tables "
                '(e.g. CREATE INDEX ON {graph}."Label" USING gin (properties)).'
            )

        if dialect_norm in ("falkordb", "falkor"):
            for model in models:
                if isinstance(model, str):
                    reg = SchemaRegistry.global_registry()
                    node_schema = reg.get_node(model)
                    if node_schema:
                        label = node_schema.get("name", model)
                        for f_name, f_spec in node_schema.get("fields", {}).items():
                            if (
                                f_spec.get("indexed")
                                and not f_spec.get("primary_key")
                                and not f_spec.get("unique")
                            ):
                                stmts.append(f"CREATE INDEX FOR (n:{label}) ON (n.{f_name})")
                        continue
                    rel_schema = reg.get_relationship(model)
                    if rel_schema:
                        rel_type = rel_schema.get("type_name", model)
                        for f_name, f_spec in rel_schema.get("fields", {}).items():
                            if (
                                f_spec.get("indexed")
                                and not f_spec.get("primary_key")
                                and not f_spec.get("unique")
                            ):
                                stmts.append(
                                    f"CREATE INDEX FOR ()-[r:{rel_type}]-() ON (r.{f_name})"
                                )
                        continue
                fields = getattr(model, "_schema_fields", {})
                is_rel = _is_relationship_model(model)
                if is_rel:
                    rel_type = getattr(model, "__type__", None) or getattr(model, "__name__", "REL")
                    for f_name, f_obj in fields.items():
                        if (
                            getattr(f_obj, "index", False)
                            and not getattr(f_obj, "unique", False)
                            and not getattr(f_obj, "primary_key", False)
                        ):
                            col = getattr(f_obj, "name", None) or f_name
                            stmts.append(f"CREATE INDEX FOR ()-[r:{rel_type}]-() ON (r.{col})")
                else:
                    label = getattr(model, "__label__", None) or getattr(model, "__name__", "Node")
                    for f_name, f_obj in fields.items():
                        if (
                            getattr(f_obj, "index", False)
                            and not getattr(f_obj, "unique", False)
                            and not getattr(f_obj, "primary_key", False)
                        ):
                            col = getattr(f_obj, "name", None) or f_name
                            stmts.append(f"CREATE INDEX FOR (n:{label}) ON (n.{col})")
            return stmts

        if dialect_norm in ("postgres", "postgresql", "duckdb", "sql_pgq", "pgq"):
            for model in models:
                if isinstance(model, str):
                    reg = SchemaRegistry.global_registry()
                    node_schema = reg.get_node(model)
                    if node_schema:
                        table = node_schema.get("name", model).lower()
                        for f_name, f_spec in node_schema.get("fields", {}).items():
                            if (
                                f_spec.get("indexed")
                                and not f_spec.get("unique")
                                and not f_spec.get("primary_key")
                            ):
                                stmts.append(
                                    f'CREATE INDEX IF NOT EXISTS idx_{table}_{f_name} ON "{table}" ("{f_name}");'
                                )
                        continue
                    rel_schema = reg.get_relationship(model)
                    if rel_schema:
                        table = rel_schema.get("type_name", model).lower()
                        for f_name, f_spec in rel_schema.get("fields", {}).items():
                            if (
                                f_spec.get("indexed")
                                and not f_spec.get("primary_key")
                                and not f_spec.get("unique")
                            ):
                                stmts.append(
                                    f'CREATE INDEX IF NOT EXISTS idx_{table}_{f_name} ON "{table}" ("{f_name}");'
                                )
                        continue
                fields = getattr(model, "_schema_fields", {})
                is_rel = _is_relationship_model(model)
                raw_name = (
                    getattr(model, "__type__", None) or getattr(model, "__name__", "rel")
                    if is_rel
                    else (
                        getattr(model, "_cached_label", None)
                        or getattr(model, "__label__", None)
                        or getattr(model, "__name__", "node")
                    )
                )
                table = str(raw_name or "entity").lower()
                for f_name, f_obj in fields.items():
                    if (
                        getattr(f_obj, "index", False)
                        and not getattr(f_obj, "unique", False)
                        and not getattr(f_obj, "primary_key", False)
                    ):
                        col = getattr(f_obj, "name", None) or f_name
                        stmts.append(
                            f'CREATE INDEX IF NOT EXISTS idx_{table}_{col} ON "{table}" ("{col}");'
                        )
            return stmts

        for model in models:
            for s in cls.generate_cypher_ddl(model, include_type_constraints=False):
                if "CREATE INDEX" in s:
                    stmts.append(s)
        return stmts

    @classmethod
    def generate_constraint_ddl(
        cls,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        dialect: str = "cypher",
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates CREATE CONSTRAINT DDL statements without executing them.

        Args:
            *models: Node and Relationship model classes, schema dicts, or registered model names.
            dialect: Target dialect ('cypher', 'postgres', etc.). Defaults to 'cypher'.
            include_type_constraints: Whether to include Property Type constraints (:: STRING).

        Returns:
            List of executable CREATE CONSTRAINT statement strings.
        """
        dialect_norm = dialect.lower()
        stmts: list[str] = []

        if dialect_norm in ("age", "apache_age"):
            raise NotImplementedError(
                "Apache AGE does not support Cypher 'CREATE CONSTRAINT' queries. "
                "Constraints in Apache AGE must be defined directly on the underlying PostgreSQL relational tables."
            )

        if dialect_norm in ("falkordb", "falkor"):
            raise NotImplementedError(
                "FalkorDB does not support openCypher 'CREATE CONSTRAINT' queries in GRAPH.QUERY. "
                "Constraints in FalkorDB must be created via native Redis commands "
                "('GRAPH.CONSTRAINT CREATE <graph_name> UNIQUE NODE <label> PROPERTIES 1 <prop>')."
            )

        if dialect_norm in ("postgres", "postgresql", "duckdb", "sql_pgq", "pgq"):
            for model in models:
                if isinstance(model, str):
                    reg = SchemaRegistry.global_registry()
                    node_schema = reg.get_node(model)
                    if node_schema:
                        table = node_schema.get("name", model).lower()
                        for f_name, f_spec in node_schema.get("fields", {}).items():
                            if f_spec.get("unique") and not f_spec.get("primary_key"):
                                stmts.append(
                                    f'ALTER TABLE "{table}" ADD CONSTRAINT uq_{table}_{f_name} UNIQUE ("{f_name}");'
                                )
                        continue
                    rel_schema = reg.get_relationship(model)
                    if rel_schema:
                        table = rel_schema.get("type_name", model).lower()
                        for f_name, f_spec in rel_schema.get("fields", {}).items():
                            if f_spec.get("unique") and not f_spec.get("primary_key"):
                                stmts.append(
                                    f'ALTER TABLE "{table}" ADD CONSTRAINT uq_{table}_{f_name} UNIQUE ("{f_name}");'
                                )
                        continue
                fields = getattr(model, "_schema_fields", {})
                is_rel = _is_relationship_model(model)
                raw_name = (
                    getattr(model, "__type__", None) or getattr(model, "__name__", "rel")
                    if is_rel
                    else (
                        getattr(model, "_cached_label", None)
                        or getattr(model, "__label__", None)
                        or getattr(model, "__name__", "node")
                    )
                )
                table = str(raw_name or "entity").lower()
                for f_name, f_obj in fields.items():
                    col = getattr(f_obj, "name", None) or f_name
                    if getattr(f_obj, "unique", False) and not getattr(f_obj, "primary_key", False):
                        stmts.append(
                            f'ALTER TABLE "{table}" ADD CONSTRAINT uq_{table}_{col} UNIQUE ("{col}");'
                        )
            return stmts

        for model in models:
            for s in cls.generate_cypher_ddl(
                model, include_type_constraints=include_type_constraints
            ):
                if "CREATE CONSTRAINT" in s:
                    stmts.append(s)
        return stmts

    @classmethod
    def generate_drop_index_ddl(
        cls,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        dialect: str = "cypher",
    ) -> list[str]:
        """Generates DROP INDEX DDL statements without executing them."""
        dialect_norm = dialect.lower()
        stmts: list[str] = []

        if dialect_norm in ("age", "apache_age"):
            raise NotImplementedError(
                "Apache AGE does not support Cypher 'DROP INDEX' queries. "
                "Indexes in Apache AGE must be dropped directly on the underlying PostgreSQL relational tables."
            )

        if dialect_norm in ("falkordb", "falkor"):
            for model in models:
                if isinstance(model, str):
                    reg = SchemaRegistry.global_registry()
                    node_schema = reg.get_node(model)
                    if node_schema:
                        label = node_schema.get("name", model)
                        for f_name, f_spec in node_schema.get("fields", {}).items():
                            if (
                                f_spec.get("indexed")
                                and not f_spec.get("primary_key")
                                and not f_spec.get("unique")
                            ):
                                stmts.append(f"DROP INDEX FOR (n:{label}) ON (n.{f_name})")
                        continue
                    rel_schema = reg.get_relationship(model)
                    if rel_schema:
                        rel_type = rel_schema.get("type_name", model)
                        for f_name, f_spec in rel_schema.get("fields", {}).items():
                            if (
                                f_spec.get("indexed")
                                and not f_spec.get("primary_key")
                                and not f_spec.get("unique")
                            ):
                                stmts.append(f"DROP INDEX FOR ()-[r:{rel_type}]-() ON (r.{f_name})")
                        continue
                fields = getattr(model, "_schema_fields", {})
                is_rel = _is_relationship_model(model)
                if is_rel:
                    rel_type = getattr(model, "__type__", None) or getattr(model, "__name__", "REL")
                    for f_name, f_obj in fields.items():
                        if (
                            getattr(f_obj, "index", False)
                            and not getattr(f_obj, "unique", False)
                            and not getattr(f_obj, "primary_key", False)
                        ):
                            col = getattr(f_obj, "name", None) or f_name
                            stmts.append(f"DROP INDEX FOR ()-[r:{rel_type}]-() ON (r.{col})")
                else:
                    label = getattr(model, "__label__", None) or getattr(model, "__name__", "Node")
                    for f_name, f_obj in fields.items():
                        if (
                            getattr(f_obj, "index", False)
                            and not getattr(f_obj, "unique", False)
                            and not getattr(f_obj, "primary_key", False)
                        ):
                            col = getattr(f_obj, "name", None) or f_name
                            stmts.append(f"DROP INDEX FOR (n:{label}) ON (n.{col})")
            return stmts

        if dialect_norm in ("postgres", "postgresql", "duckdb", "sql_pgq", "pgq"):
            for model in models:
                if isinstance(model, str):
                    reg = SchemaRegistry.global_registry()
                    node_schema = reg.get_node(model)
                    if node_schema:
                        table = node_schema.get("name", model).lower()
                        for f_name, f_spec in node_schema.get("fields", {}).items():
                            if (
                                f_spec.get("indexed")
                                and not f_spec.get("unique")
                                and not f_spec.get("primary_key")
                            ):
                                stmts.append(f"DROP INDEX IF EXISTS idx_{table}_{f_name};")
                        continue
                    rel_schema = reg.get_relationship(model)
                    if rel_schema:
                        table = rel_schema.get("type_name", model).lower()
                        for f_name, f_spec in rel_schema.get("fields", {}).items():
                            if (
                                f_spec.get("indexed")
                                and not f_spec.get("primary_key")
                                and not f_spec.get("unique")
                            ):
                                stmts.append(f"DROP INDEX IF EXISTS idx_{table}_{f_name};")
                        continue
                fields = getattr(model, "_schema_fields", {})
                is_rel = _is_relationship_model(model)
                raw_name = (
                    getattr(model, "__type__", None) or getattr(model, "__name__", "rel")
                    if is_rel
                    else (
                        getattr(model, "_cached_label", None)
                        or getattr(model, "__label__", None)
                        or getattr(model, "__name__", "node")
                    )
                )
                table = str(raw_name or "entity").lower()
                for f_name, f_obj in fields.items():
                    if (
                        getattr(f_obj, "index", False)
                        and not getattr(f_obj, "unique", False)
                        and not getattr(f_obj, "primary_key", False)
                    ):
                        col = getattr(f_obj, "name", None) or f_name
                        stmts.append(f"DROP INDEX IF EXISTS idx_{table}_{col};")
            return stmts

        for model in models:
            for s in cls.generate_drop_ddl(model, include_type_constraints=False):
                if "DROP INDEX" in s:
                    stmts.append(s)
        return stmts

    @classmethod
    def generate_drop_constraint_ddl(
        cls,
        *models: type[Node] | type[Relationship] | dict[str, Any] | str,
        dialect: str = "cypher",
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates DROP CONSTRAINT DDL statements without executing them."""
        dialect_norm = dialect.lower()
        stmts: list[str] = []

        if dialect_norm in ("age", "apache_age"):
            raise NotImplementedError(
                "Apache AGE does not support Cypher 'DROP CONSTRAINT' queries. "
                "Constraints in Apache AGE must be dropped directly on the underlying PostgreSQL relational tables."
            )

        if dialect_norm in ("falkordb", "falkor"):
            raise NotImplementedError(
                "FalkorDB does not support openCypher 'DROP CONSTRAINT' queries in GRAPH.QUERY. "
                "Constraints in FalkorDB must be dropped via native Redis commands "
                "('GRAPH.CONSTRAINT DROP <graph_name> UNIQUE NODE <label> PROPERTIES 1 <prop>')."
            )

        if dialect_norm in ("postgres", "postgresql", "duckdb", "sql_pgq", "pgq"):
            for model in models:
                if isinstance(model, str):
                    reg = SchemaRegistry.global_registry()
                    node_schema = reg.get_node(model)
                    if node_schema:
                        table = node_schema.get("name", model).lower()
                        for f_name, f_spec in node_schema.get("fields", {}).items():
                            if f_spec.get("unique") and not f_spec.get("primary_key"):
                                stmts.append(
                                    f'ALTER TABLE "{table}" DROP CONSTRAINT IF EXISTS uq_{table}_{f_name};'
                                )
                        continue
                    rel_schema = reg.get_relationship(model)
                    if rel_schema:
                        table = rel_schema.get("type_name", model).lower()
                        for f_name, f_spec in rel_schema.get("fields", {}).items():
                            if f_spec.get("unique") and not f_spec.get("primary_key"):
                                stmts.append(
                                    f'ALTER TABLE "{table}" DROP CONSTRAINT IF EXISTS uq_{table}_{f_name};'
                                )
                        continue
                fields = getattr(model, "_schema_fields", {})
                is_rel = _is_relationship_model(model)
                raw_name = (
                    getattr(model, "__type__", None) or getattr(model, "__name__", "rel")
                    if is_rel
                    else (
                        getattr(model, "_cached_label", None)
                        or getattr(model, "__label__", None)
                        or getattr(model, "__name__", "node")
                    )
                )
                table = str(raw_name or "entity").lower()
                for f_name, f_obj in fields.items():
                    col = getattr(f_obj, "name", None) or f_name
                    if getattr(f_obj, "unique", False) and not getattr(f_obj, "primary_key", False):
                        stmts.append(
                            f'ALTER TABLE "{table}" DROP CONSTRAINT IF EXISTS uq_{table}_{col};'
                        )
            return stmts

        for model in models:
            for s in cls.generate_drop_ddl(
                model, include_type_constraints=include_type_constraints
            ):
                if "DROP CONSTRAINT" in s:
                    stmts.append(s)
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
