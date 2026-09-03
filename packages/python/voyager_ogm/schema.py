"""Voyager OGM Schema & Graph Types DDL Engine.

Provides automated constraint generation, schema reflection, index management,
and Graph Types validation for openCypher (Neo4j / Memgraph), ISO GQL, and SQL:2023 PGQ.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from voyager_ogm.models import Node, Relationship
    from voyager_ogm.session import Session

_PYTHON_TO_NEO4J_TYPES: dict[Any, str] = {
    str: "STRING",
    int: "INTEGER",
    float: "FLOAT",
    bool: "BOOLEAN",
    list: "LIST",
    dict: "MAP",
}

_PYTHON_TO_GQL_TYPES: dict[Any, str] = {
    str: "STRING",
    int: "INTEGER",
    float: "FLOAT",
    bool: "BOOLEAN",
    list: "LIST",
}


class SchemaManager:
    """Manages schema constraints, indexes, graph types, and DDL migrations."""

    @staticmethod
    def generate_cypher_ddl(
        model: type[Node] | type[Relationship],
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates openCypher / Neo4j constraint and index creation DDL statements.

        Supports:
        - Unique constraints (REQUIRE n.prop IS UNIQUE)
        - Not Null / Existence constraints (REQUIRE n.prop IS NOT NULL)
        - Property Type Constraints (Neo4j 5.x Graph Types: REQUIRE n.prop :: STRING)
        - B-Tree Indexes (FOR (n:Label) ON (n.prop))

        Args:
            model: Target Node or Relationship model class.
            include_type_constraints: Whether to emit Neo4j 5.x Enterprise Property Type
                constraints (:: STRING, :: INTEGER). Defaults to False.

        Returns:
            List of executable DDL Cypher statement strings.
        """
        statements: list[str] = []
        labels = getattr(model, "__labels__", None)
        edge_type = getattr(model, "__type__", None)
        fields = dict(getattr(model, "_schema_fields", {}))
        if not fields:
            for k, v in model.__dict__.items():
                if hasattr(v, "unique") or hasattr(v, "index"):
                    fields[k] = v

        if labels is not None or (
            hasattr(model, "__mro__") and any(b.__name__ == "Node" for b in model.__mro__)
        ):
            actual_labels = labels if labels else [model.__name__]
            primary_label = actual_labels[0]
            for prop_name, field_def in fields.items():
                db_name = getattr(field_def, "name", None) or prop_name

                # Primary Key / Unique Constraint
                if getattr(field_def, "unique", False) or getattr(field_def, "primary_key", False):
                    c_name = f"constraint_{primary_label.lower()}_{db_name}_unique"
                    statements.append(
                        f"CREATE CONSTRAINT {c_name} IF NOT EXISTS FOR (n:{primary_label}) REQUIRE n.{db_name} IS UNIQUE"
                    )

                # Index
                if getattr(field_def, "index", False) and not (
                    getattr(field_def, "unique", False) or getattr(field_def, "primary_key", False)
                ):
                    i_name = f"index_{primary_label.lower()}_{db_name}"
                    statements.append(
                        f"CREATE INDEX {i_name} IF NOT EXISTS FOR (n:{primary_label}) ON (n.{db_name})"
                    )

                # Property Type Constraint (Graph Types in Neo4j 5+ Enterprise)
                if include_type_constraints:
                    type_ann = getattr(field_def, "type_annotation", None)
                    if type_ann and type_ann in _PYTHON_TO_NEO4J_TYPES:
                        neo4j_type = _PYTHON_TO_NEO4J_TYPES[type_ann]
                        t_name = f"constraint_{primary_label.lower()}_{db_name}_type"
                        statements.append(
                            f"CREATE CONSTRAINT {t_name} IF NOT EXISTS FOR (n:{primary_label}) REQUIRE n.{db_name} :: {neo4j_type}"
                        )

        elif edge_type or (
            hasattr(model, "__mro__") and any(b.__name__ == "Relationship" for b in model.__mro__)
        ):
            actual_type = edge_type or model.__name__.upper()
            for prop_name, field_def in fields.items():
                db_name = getattr(field_def, "name", None) or prop_name
                if getattr(field_def, "primary_key", False) or getattr(field_def, "unique", False):
                    c_name = f"constraint_rel_{actual_type.lower()}_{db_name}_not_null"
                    statements.append(
                        f"CREATE CONSTRAINT {c_name} IF NOT EXISTS FOR ()-[r:{actual_type}]-() REQUIRE r.{db_name} IS NOT NULL"
                    )

        return statements

    @staticmethod
    def generate_drop_ddl(
        model: type[Node] | type[Relationship],
        include_type_constraints: bool = False,
    ) -> list[str]:
        """Generates drop statements for constraints and indexes.

        Args:
            model: Target Node or Relationship model class.
            include_type_constraints: Whether to emit drop statements for Property Type constraints.

        Returns:
            List of executable DROP statement strings.
        """
        statements: list[str] = []
        labels = getattr(model, "__labels__", None)
        fields = dict(getattr(model, "_schema_fields", {}))
        if not fields:
            for k, v in model.__dict__.items():
                if hasattr(v, "unique") or hasattr(v, "index"):
                    fields[k] = v

        if labels is not None or (
            hasattr(model, "__mro__") and any(b.__name__ == "Node" for b in model.__mro__)
        ):
            actual_labels = labels if labels else [model.__name__]
            primary_label = actual_labels[0]
            for prop_name, field_def in fields.items():
                db_name = getattr(field_def, "name", None) or prop_name
                if getattr(field_def, "unique", False) or getattr(field_def, "primary_key", False):
                    c_name = f"constraint_{primary_label.lower()}_{db_name}_unique"
                    statements.append(f"DROP CONSTRAINT {c_name} IF EXISTS")
                if getattr(field_def, "index", False):
                    i_name = f"index_{primary_label.lower()}_{db_name}"
                    statements.append(f"DROP INDEX {i_name} IF EXISTS")
                if (
                    include_type_constraints
                    and getattr(field_def, "type_annotation", None)
                    and field_def.type_annotation in _PYTHON_TO_NEO4J_TYPES
                ):
                    t_name = f"constraint_{primary_label.lower()}_{db_name}_type"
                    statements.append(f"DROP CONSTRAINT {t_name} IF EXISTS")

        return statements

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
        """
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
        """
        dropped: list[str] = []
        for model in models:
            for stmt in cls.generate_drop_ddl(
                model, include_type_constraints=include_type_constraints
            ):
                session.execute(stmt)
                dropped.append(stmt)
        return dropped

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
        labels = getattr(model, "__labels__", None)
        edge_type = getattr(model, "__type__", None)
        fields = dict(getattr(model, "_schema_fields", {}))
        if not fields:
            for k, v in model.__dict__.items():
                if hasattr(v, "unique") or hasattr(v, "index"):
                    fields[k] = v

        if labels is not None or (
            hasattr(model, "__mro__") and any(b.__name__ == "Node" for b in model.__mro__)
        ):
            actual_labels = labels if labels else [model.__name__]
            primary_label = actual_labels[0]
            prop_defs: list[str] = []
            for prop_name, field_def in fields.items():
                db_name = getattr(field_def, "name", None) or prop_name
                type_ann = getattr(field_def, "type_annotation", None)
                gql_type = _PYTHON_TO_GQL_TYPES.get(type_ann, "ANY")
                optional_marker = (
                    ""
                    if getattr(field_def, "primary_key", False)
                    or getattr(field_def, "unique", False)
                    else "?"
                )
                prop_defs.append(f"{db_name} :: {gql_type}{optional_marker}")

            props_str = f" {{{', '.join(prop_defs)}}}" if prop_defs else ""
            return f"ALTER CURRENT GRAPH TYPE ADD NODE TYPE (:{primary_label}{props_str})"

        elif edge_type or (
            hasattr(model, "__mro__") and any(b.__name__ == "Relationship" for b in model.__mro__)
        ):
            actual_type = edge_type or model.__name__.upper()
            prop_defs = []
            for prop_name, field_def in fields.items():
                db_name = getattr(field_def, "name", None) or prop_name
                type_ann = getattr(field_def, "type_annotation", None)
                gql_type = _PYTHON_TO_GQL_TYPES.get(type_ann, "ANY")
                prop_defs.append(f"{db_name} :: {gql_type}")

            props_str = f" {{{', '.join(prop_defs)}}}" if prop_defs else ""
            src_str = getattr(source_node, "__name__", str(source_node)) if source_node else ""
            tgt_str = getattr(target_node, "__name__", str(target_node)) if target_node else ""
            src_pattern = f"(:{src_str})" if src_str else "()"
            tgt_pattern = f"(:{tgt_str})" if tgt_str else "()"
            return f"ALTER CURRENT GRAPH TYPE ADD RELATIONSHIP TYPE {src_pattern}-[:{actual_type}{props_str}]->{tgt_pattern}"

        return ""

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
        elements: list[str] = []
        for model in models:
            labels = getattr(model, "__labels__", None)
            edge_type = getattr(model, "__type__", None)
            fields = dict(getattr(model, "_schema_fields", {}))
            if not fields:
                for k, v in model.__dict__.items():
                    if hasattr(v, "unique") or hasattr(v, "index"):
                        fields[k] = v

            if labels is not None or (
                hasattr(model, "__mro__") and any(b.__name__ == "Node" for b in model.__mro__)
            ):
                actual_labels = labels if labels else [model.__name__]
                primary_label = actual_labels[0]
                prop_defs = []
                for prop_name, field_def in fields.items():
                    db_name = getattr(field_def, "name", None) or prop_name
                    type_ann = getattr(field_def, "type_annotation", None)
                    gql_type = _PYTHON_TO_GQL_TYPES.get(type_ann, "ANY")
                    prop_defs.append(f"{db_name} {gql_type}")
                props_str = f" ({', '.join(prop_defs)})" if prop_defs else ""
                elements.append(f"    NODE {primary_label}{props_str}")

            elif edge_type or (
                hasattr(model, "__mro__")
                and any(b.__name__ == "Relationship" for b in model.__mro__)
            ):
                actual_type = edge_type or model.__name__.upper()
                prop_defs = []
                for prop_name, field_def in fields.items():
                    db_name = getattr(field_def, "name", None) or prop_name
                    type_ann = getattr(field_def, "type_annotation", None)
                    gql_type = _PYTHON_TO_GQL_TYPES.get(type_ann, "ANY")
                    prop_defs.append(f"{db_name} {gql_type}")
                props_str = f" ({', '.join(prop_defs)})" if prop_defs else ""
                elements.append(f"    EDGE {actual_type}{props_str}")

        body = ",\n".join(elements)
        return f"CREATE GRAPH TYPE {graph_type_name} AS {{\n{body}\n}}"
