"""Voyager OGM High-Level Fluent Query Builder.

Provides an intuitive, fluent query interface that generates multi-dialect
graph queries (openCypher, SQL:2023 PGQ, ISO GQL) backed by a 32-bit Rust AST arena.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from voyager_ogm._voyager_rs import NativeQueryBuilder
from voyager_ogm.models import BoundField, Node, PredicateExpr, Relationship


@dataclass(frozen=True)
class CompiledQuery:
    """Compiled parameterized graph query statement and parameters.

    Attributes:
        statement: The parameterized query string formatted for the target dialect.
        parameters: Deterministic dictionary mapping parameter names (e.g. 'p0') to values.
    """

    statement: str
    parameters: dict[str, Any]


class Query:
    """Fluent Graph Query Builder with multi-dialect compilation.

    Constructs graph query ASTs with chainable pattern matching, relationship
    traversals, filter predicates, aggregations, and pagination.

    Example:
        >>> p = Person("p")
        >>> query = (
        ...     Query.match(p)
        ...     .where(p.age >= 21)
        ...     .return_(p.name, p.age)
        ...     .limit(10)
        ... )
        >>> compiled = query.compile("cypher")
    """

    def __init__(self) -> None:
        """Initializes a new query builder with an underlying Rust AST arena."""
        self._native = NativeQueryBuilder()

    @classmethod
    def match(cls, node_or_type: Node | type[Node] | None = None) -> Query:
        """Starts a standard MATCH clause.

        Args:
            node_or_type: Optional Node instance or Node subclass to initialize the path.

        Returns:
            A new Query instance initialized with the MATCH clause.

        Example:
            >>> query = Query.match(Person)
        """
        q = cls()
        q._native.match()
        if node_or_type is not None:
            q.node(node_or_type)
        return q

    @classmethod
    def create(cls, node_or_type: Node | type[Node] | None = None) -> Query:
        """Starts a CREATE mutation clause.

        Args:
            node_or_type: Optional Node instance or Node subclass to initialize the path.

        Returns:
            A new Query instance initialized with the CREATE clause.

        Example:
            >>> query = Query.create(Person(name="Alice"))
        """
        q = cls()
        q._native.create()
        if node_or_type is not None:
            q.node(node_or_type)
        return q

    @classmethod
    def merge(cls, node_or_type: Node | type[Node] | None = None) -> Query:
        """Starts a MERGE idempotent upsert clause.

        Args:
            node_or_type: Optional Node instance or Node subclass to initialize the path.

        Returns:
            A new Query instance initialized with the MERGE clause.

        Example:
            >>> query = Query.merge(Person(id=42)).on_create_set(p.name == "Bob")
        """
        q = cls()
        q._native.merge()
        if node_or_type is not None:
            q.node(node_or_type)
        return q

    @classmethod
    def optional_match(cls, node_or_type: Node | type[Node] | None = None) -> Query:
        """Starts an OPTIONAL MATCH clause.

        Args:
            node_or_type: Optional Node instance or Node subclass to initialize the path.

        Returns:
            A new Query instance initialized with the OPTIONAL MATCH clause.
        """
        q = cls()
        q._native.optional_match()
        if node_or_type is not None:
            q.node(node_or_type)
        return q

    @classmethod
    def call(cls, procedure_name: str, *args: Any, **kwargs: Any) -> Query:
        """Starts a vendor procedure call (e.g. APOC or GDS).

        Args:
            procedure_name: Qualified procedure name (e.g. 'apoc.path.expandConfig').
            *args: Positional literal arguments.
            **kwargs: Named parameter key-value pairs.

        Returns:
            A new Query instance initialized with the procedure call.
        """
        q = cls()
        arg_literals = list(args)
        q._native.call_procedure(procedure_name, arg_literals, kwargs)
        return q

    @classmethod
    def unwind(cls, batch_param: str, alias: str = "row") -> Query:
        """Starts an UNWIND batch unrolling statement: `UNWIND $batch_param AS alias`.

        Args:
            batch_param: Name of the parameter list (e.g. 'batch').
            alias: Row alias name (default: 'row').

        Returns:
            A new Query instance initialized with the UNWIND clause.
        """
        q = cls()
        q._native.unwind(batch_param.lstrip("$"), alias)
        return q

    def yield_(self, *yield_items: str) -> Query:
        """Yields columns from a procedure call.

        Args:
            *yield_items: Names of the procedure result columns to yield.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.yield_items(list(yield_items))
        return self

    def add_unwind(self, batch_param: str, alias: str = "row") -> Query:
        """Adds an UNWIND batch expansion clause: `UNWIND $batch_param AS alias`.

        Args:
            batch_param: Name of the parameter list (e.g. 'batch').
            alias: Row alias name (default: 'row').

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.unwind(batch_param.lstrip("$"), alias)
        return self

    def add_create(self, node_or_type: Node | type[Node] | None = None) -> Query:
        """Adds a CREATE mutation clause to the active query statement.

        Args:
            node_or_type: Optional Node instance or Node subclass to initialize the path.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.create()
        if node_or_type is not None:
            self.node(node_or_type)
        return self

    def add_merge(self, node_or_type: Node | type[Node] | None = None) -> Query:
        """Adds a MERGE idempotent upsert clause to the active query statement.

        Args:
            node_or_type: Optional Node instance or Node subclass to initialize the path.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.merge()
        if node_or_type is not None:
            self.node(node_or_type)
        return self

    def add_match(self, node_or_type: Node | type[Node] | None = None) -> Query:
        """Adds a successive MATCH clause.

        Args:
            node_or_type: Optional Node instance or Node subclass to append.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.match()
        if node_or_type is not None:
            self.node(node_or_type)
        return self

    def add_optional_match(self, node_or_type: Node | type[Node] | None = None) -> Query:
        """Adds a successive OPTIONAL MATCH clause.

        Args:
            node_or_type: Optional Node instance or Node subclass to append.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.optional_match()
        if node_or_type is not None:
            self.node(node_or_type)
        return self

    def node(
        self,
        node_or_var: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
    ) -> Query:
        """Appends a node pattern to the query path.

        Args:
            node_or_var: Node instance, Node subclass, variable alias string, or None.
            labels: Optional label or list of labels when `node_or_var` is a variable name.

        Returns:
            The Query instance for fluent chaining.

        Example:
            >>> query.node("p", labels=["Person", "Actor"])
        """
        if isinstance(node_or_var, Node):
            self._native.node(node_or_var.alias, node_or_var.labels)
        elif isinstance(node_or_var, type) and issubclass(node_or_var, Node):
            instance = node_or_var()
            self._native.node(instance.alias, instance.labels)
        elif isinstance(node_or_var, str):
            lbls = [labels] if isinstance(labels, str) else (labels or [])
            self._native.node(node_or_var, lbls)
        elif labels is not None:
            lbls = [labels] if isinstance(labels, str) else labels
            self._native.node(None, lbls)
        else:
            self._native.node(None, [])
        return self

    def _extract_rel_info(
        self, rel: Relationship | type[Relationship] | str | list[str] | None, var: str | None
    ) -> tuple[list[str], str | None]:
        if isinstance(rel, Relationship):
            return [rel.edge_type], rel.alias
        elif isinstance(rel, type) and issubclass(rel, Relationship):
            instance = rel()
            return [instance.edge_type], var or instance.alias
        elif isinstance(rel, str):
            return [rel], var
        elif isinstance(rel, list):
            return rel, var
        return [], var

    def to(
        self,
        rel: Relationship | type[Relationship] | str | list[str] | None = None,
        var: str | None = None,
    ) -> Query:
        """Appends an outgoing relationship traversal `-[r:TYPE]->`.

        Args:
            rel: Relationship model, subclass, edge type string, or list of types.
            var: Optional variable alias for the relationship edge.

        Returns:
            The Query instance for fluent chaining.
        """
        types, edge_var = self._extract_rel_info(rel, var)
        self._native.to(types, edge_var)
        return self

    def from_(
        self,
        rel: Relationship | type[Relationship] | str | list[str] | None = None,
        var: str | None = None,
    ) -> Query:
        """Appends an incoming relationship traversal `<-[r:TYPE]-`.

        Args:
            rel: Relationship model, subclass, edge type string, or list of types.
            var: Optional variable alias for the relationship edge.

        Returns:
            The Query instance for fluent chaining.
        """
        types, edge_var = self._extract_rel_info(rel, var)
        self._native.from_edge(types, edge_var)
        return self

    def edge(
        self,
        rel: Relationship | type[Relationship] | str | list[str] | None = None,
        var: str | None = None,
    ) -> Query:
        """Appends an undirected relationship traversal `-[r:TYPE]-`.

        Args:
            rel: Relationship model, subclass, edge type string, or list of types.
            var: Optional variable alias for the relationship edge.

        Returns:
            The Query instance for fluent chaining.
        """
        types, edge_var = self._extract_rel_info(rel, var)
        self._native.edge(types, edge_var)
        return self

    def hops(self, min_hops: int, max_hops: int) -> Query:
        """Sets variable-length path repetition (e.g. `*1..3`).

        Args:
            min_hops: Minimum number of relationship hops (e.g. 1).
            max_hops: Maximum number of relationship hops (e.g. 3).

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.hops(min_hops, max_hops)
        return self

    def where(self, *predicates: PredicateExpr) -> Query:
        """Applies filter predicates to the current query path.

        Args:
            *predicates: Predicate expressions created via operator overloads
                (e.g. `p.age >= 21`, `p.name == 'Alice'`).

        Returns:
            The Query instance for fluent chaining.

        Raises:
            ValueError: If an unsupported predicate operator is encountered.
        """
        for pred in predicates:
            if pred.op == "eq":
                self._native.where_eq(pred.target, pred.field, pred.value)
            elif pred.op == "gt":
                self._native.where_gt(pred.target, pred.field, pred.value)
            elif pred.op == "gte":
                self._native.where_gte(pred.target, pred.field, pred.value)
            elif pred.op == "lt":
                self._native.where_lt(pred.target, pred.field, pred.value)
            elif pred.op == "lte":
                self._native.where_lte(pred.target, pred.field, pred.value)
            elif pred.op == "contains":
                self._native.where_contains(pred.target, pred.field, str(pred.value))
            else:
                msg = f"Unsupported predicate operator: {pred.op}"
                raise ValueError(msg)
        return self

    def on_create_set(self, *assignments: PredicateExpr, **kwargs: Any) -> Query:
        """Adds ON CREATE SET property assignments to the active MERGE block.

        Args:
            *assignments: PredicateExpr assignments (e.g. `p.created_at == 2026`).
            **kwargs: Property assignments for the active node.

        Returns:
            The Query instance for fluent chaining.
        """
        for assign in assignments:
            self._native.on_create_set(assign.target, assign.field, assign.value)
        for key, val in kwargs.items():
            if "." in key:
                var, prop = key.split(".", 1)
                self._native.on_create_set(var, prop, val)
        return self

    def on_match_set(self, *assignments: PredicateExpr, **kwargs: Any) -> Query:
        """Adds ON MATCH SET property assignments to the active MERGE block.

        Args:
            *assignments: PredicateExpr assignments (e.g. `p.updated_at == 2026`).
            **kwargs: Property assignments for the active node.

        Returns:
            The Query instance for fluent chaining.
        """
        for assign in assignments:
            self._native.on_match_set(assign.target, assign.field, assign.value)
        for key, val in kwargs.items():
            if "." in key:
                var, prop = key.split(".", 1)
                self._native.on_match_set(var, prop, val)
        return self

    def set(self, *assignments: PredicateExpr | Node, **kwargs: Any) -> Query:
        """Adds SET property assignments to the active statement.

        Args:
            *assignments: PredicateExpr assignments (e.g. `p.status == 'ACTIVE'`) or
                Node instances with dirty tracked fields.
            **kwargs: Property assignments (e.g. `{"p.status": "ACTIVE"}`).

        Returns:
            The Query instance for fluent chaining.
        """
        for assign in assignments:
            if isinstance(assign, PredicateExpr):
                self._native.set_property(assign.target, assign.field, assign.value)
            elif isinstance(assign, Node):
                for field_name, val in assign.dirty_fields.items():
                    self._native.set_property(assign.alias, field_name, val)
        for key, val in kwargs.items():
            if "." in key:
                var, prop = key.split(".", 1)
                self._native.set_property(var, prop, val)
        return self

    def delete(self, *targets: Node | Relationship | str) -> Query:
        """Adds a DELETE clause for one or more entity variables.

        Args:
            *targets: Node instances, Relationship instances, or variable strings.

        Returns:
            The Query instance for fluent chaining.
        """
        names: list[str] = []
        for t in targets:
            if isinstance(t, (Node, Relationship)):
                names.append(t.alias)
            else:
                names.append(str(t))
        self._native.delete(names)
        return self

    def detach_delete(self, *targets: Node | Relationship | str) -> Query:
        """Adds a DETACH DELETE clause for one or more entity variables.

        Args:
            *targets: Node instances, Relationship instances, or variable strings.

        Returns:
            The Query instance for fluent chaining.
        """
        names: list[str] = []
        for t in targets:
            if isinstance(t, (Node, Relationship)):
                names.append(t.alias)
            else:
                names.append(str(t))
        self._native.detach_delete(names)
        return self

    def remove(self, *properties: BoundField | str) -> Query:
        """Adds a REMOVE clause for one or more property fields.

        Args:
            *properties: BoundField instances or property strings ('var.prop').

        Returns:
            The Query instance for fluent chaining.
        """
        for p in properties:
            if isinstance(p, BoundField):
                self._native.remove_property(p.target_alias, p.field_name)
            elif isinstance(p, str) and "." in p:
                var, prop = p.split(".", 1)
                self._native.remove_property(var, prop)
        return self

    def return_(
        self,
        *fields: BoundField | str,
        distinct: bool = False,
        **aliased_fields: BoundField | str,
    ) -> Query:
        """Initializes column projections for the RETURN clause.

        Args:
            *fields: Positional fields or raw strings to project.
            distinct: If True, emits `RETURN DISTINCT`.
            **aliased_fields: Keyword arguments mapping custom alias names to fields.

        Returns:
            The Query instance for fluent chaining.

        Example:
            >>> query.return_(p.name, p.age, distinct=True, user_city=p.city)
        """
        self._native.return_()
        if distinct:
            self._native.distinct()

        for field in fields:
            if isinstance(field, BoundField):
                self._native.field(field.target_alias, field.field_name, None)
            elif isinstance(field, str):
                parts = field.split()
                if len(parts) == 3 and parts[1].upper() == "AS":
                    var_prop = parts[0].split(".")
                    self._native.field(var_prop[0], var_prop[1], parts[2])
                elif "." in parts[0]:
                    var_prop = parts[0].split(".")
                    self._native.field(var_prop[0], var_prop[1], None)
                else:
                    self._native.field(parts[0], "", None)

        for alias, field in aliased_fields.items():
            if isinstance(field, BoundField):
                self._native.field(field.target_alias, field.field_name, alias)
            elif isinstance(field, str) and "." in field:
                var_prop = field.split(".")
                self._native.field(var_prop[0], var_prop[1], alias)
            elif isinstance(field, str):
                self._native.field(field, "", alias)
        return self

    def order_by(self, field: BoundField | str, ascending: bool = True) -> Query:
        """Sorts the query results by a field.

        Args:
            field: Field or property string to sort by.
            ascending: If True sorts ascending; if False sorts descending.

        Returns:
            The Query instance for fluent chaining.
        """
        if isinstance(field, BoundField):
            self._native.order_by(field.target_alias, field.field_name, ascending)
        elif isinstance(field, str) and "." in field:
            var_prop = field.split(".")
            self._native.order_by(var_prop[0], var_prop[1], ascending)
        return self

    def order_by_desc(self, field: BoundField | str) -> Query:
        """Sorts the query results descending.

        Args:
            field: Field or property string to sort by descending.

        Returns:
            The Query instance for fluent chaining.
        """
        return self.order_by(field, ascending=False)

    def limit(self, count: int) -> Query:
        """Limits the maximum number of returned rows.

        Args:
            count: Maximum number of rows to return.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.limit(count)
        return self

    def skip(self, count: int) -> Query:
        """Skips the first N rows for pagination.

        Args:
            count: Number of rows to skip.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.skip(count)
        return self

    def compile(self, dialect: str = "cypher", graph_name: str | None = None) -> CompiledQuery:
        """Compiles the AST query into a parameterized dialect query statement.

        Args:
            dialect: Target dialect name ('cypher', 'sql_pgq', 'iso_gql').
            graph_name: Optional graph table name for SQL:2023 PGQ queries.

        Returns:
            CompiledQuery containing the parameterized statement and parameter map.

        Raises:
            ValueError: If the requested dialect is unsupported.

        Example:
            >>> compiled = query.compile("sql_pgq", graph_name="social_graph")
        """
        res = self._native.compile(dialect, graph_name)
        return CompiledQuery(statement=res["statement"], parameters=res["parameters"])


def unwind(batch_param: str, alias: str = "row") -> Query:
    """Starts an UNWIND batch expansion query statement: `UNWIND $batch_param AS alias`."""
    return Query.unwind(batch_param, alias=alias)
