"""Voyager OGM High-Level Fluent Query Builder.

Provides an intuitive, fluent query interface that generates multi-dialect
graph queries (openCypher, SQL:2023 PGQ, ISO GQL) backed by a 32-bit Rust AST arena.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from voyager_ogm._voyager_rs import NativeQueryBuilder
from voyager_ogm.models import (
    AggregationExpr,
    BoundField,
    Node,
    PredicateExpr,
    Relationship,
)


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
    def match(
        cls,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Starts a standard MATCH clause.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            A new Query instance initialized with the MATCH clause.
        """
        q = cls()
        q._native.match()
        if node_or_type is not None or labels is not None or variable is not None:
            q.node(node_or_type, labels=labels, variable=variable)
        return q

    @classmethod
    def match_node(
        cls,
        variable: str | None = None,
        labels: list[str] | str | None = None,
    ) -> Query:
        """Convenience factory method that begins a MATCH clause for a single node."""
        return cls.match(variable=variable, labels=labels)

    @classmethod
    def create(
        cls,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Starts a CREATE mutation clause.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            A new Query instance initialized with the CREATE clause.
        """
        q = cls()
        q._native.create()
        if node_or_type is not None or labels is not None or variable is not None:
            q.node(node_or_type, labels=labels, variable=variable)
        return q

    @classmethod
    def merge(
        cls,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Starts a MERGE idempotent upsert clause.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            A new Query instance initialized with the MERGE clause.
        """
        q = cls()
        q._native.merge()
        if node_or_type is not None or labels is not None or variable is not None:
            q.node(node_or_type, labels=labels, variable=variable)
        return q
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

    @classmethod
    def load_csv(cls, url: str, with_headers: bool = True, alias: str = "row") -> Query:
        """Starts a LOAD CSV file ingestion statement: `LOAD CSV [WITH HEADERS] FROM url AS alias`.

        Args:
            url: File URL or path (e.g. 'file:///persons.csv').
            with_headers: Whether to parse the first line as column header keys (default: True).
            alias: Row alias variable name (default: 'row').

        Returns:
            A new Query instance initialized with the LOAD CSV clause.
        """
        q = cls()
        q._native.load_csv(url, with_headers, alias)
        return q

    def add_load_csv(self, url: str, with_headers: bool = True, alias: str = "row") -> Query:
        """Adds a LOAD CSV ingestion clause to the active query statement.

        Args:
            url: File URL or path (e.g. 'file:///persons.csv').
            with_headers: Whether to parse the first line as column header keys (default: True).
            alias: Row alias variable name (default: 'row').

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.load_csv(url, with_headers, alias)
        return self

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

    def add_match(
        self,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Adds a successive MATCH clause.

        Args:
            node_or_type: Optional Node instance or Node subclass to append.
            labels: Optional label(s) for the node pattern.
            variable: Optional variable alias.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.match()
        if node_or_type is not None or labels is not None or variable is not None:
            self.node(node_or_type, labels=labels, variable=variable)
        return self

    def add_optional_match(
        self,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Adds a successive OPTIONAL MATCH clause.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.optional_match()
        if node_or_type is not None or labels is not None or variable is not None:
            self.node(node_or_type, labels=labels, variable=variable)
        return self

    def node(
        self,
        node_or_var: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Appends a node pattern to the query path.

        Args:
            node_or_var: Node instance, Node subclass, variable alias string, or None.
            labels: Optional label or list of labels when `node_or_var` is a variable name.
            variable: Optional variable alias name.

        Returns:
            The Query instance for fluent chaining.

        Example:
            >>> query.node("p", labels=["Person", "Actor"])
        """
        if variable is not None and node_or_var is None:
            node_or_var = variable
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
        self,
        rel: Relationship | type[Relationship] | str | list[str] | None,
        var: str | None,
        edge_type: str | list[str] | None = None,
        variable: str | None = None,
    ) -> tuple[list[str], str | None]:
        actual_rel = rel if rel is not None else edge_type
        actual_var = var if var is not None else variable
        if isinstance(actual_rel, Relationship):
            return [actual_rel.edge_type], actual_rel.alias
        elif isinstance(actual_rel, type) and issubclass(actual_rel, Relationship):
            instance = actual_rel()
            return [instance.edge_type], actual_var or instance.alias
        elif isinstance(actual_rel, str):
            return [actual_rel], actual_var
        elif isinstance(actual_rel, list):
            return actual_rel, actual_var
        return [], actual_var

    def to(
        self,
        rel: Relationship | type[Relationship] | str | list[str] | None = None,
        var: str | None = None,
        *,
        edge_type: str | list[str] | None = None,
        variable: str | None = None,
    ) -> Query:
        """Appends an outgoing relationship traversal `-[r:TYPE]->`.

        Args:
            rel: Relationship model, subclass, edge type string, or list of types.
            var: Optional variable alias for the relationship edge.
            edge_type: Keyword argument alias for `rel`.
            variable: Keyword argument alias for `var`.

        Returns:
            The Query instance for fluent chaining.
        """
        types, edge_var = self._extract_rel_info(rel, var, edge_type=edge_type, variable=variable)
        self._native.to(types, edge_var)
        return self

    def from_(
        self,
        rel: Relationship | type[Relationship] | str | list[str] | None = None,
        var: str | None = None,
        *,
        edge_type: str | list[str] | None = None,
        variable: str | None = None,
    ) -> Query:
        """Appends an incoming relationship traversal `<-[r:TYPE]-`.

        Args:
            rel: Relationship model, subclass, edge type string, or list of types.
            var: Optional variable alias for the relationship edge.
            edge_type: Keyword argument alias for `rel`.
            variable: Keyword argument alias for `var`.

        Returns:
            The Query instance for fluent chaining.
        """
        types, edge_var = self._extract_rel_info(rel, var, edge_type=edge_type, variable=variable)
        self._native.from_edge(types, edge_var)
        return self

    def edge(
        self,
        rel: Relationship | type[Relationship] | str | list[str] | None = None,
        var: str | None = None,
        *,
        edge_type: str | list[str] | None = None,
        variable: str | None = None,
    ) -> Query:
        """Appends an undirected relationship traversal `-[r:TYPE]-`.

        Args:
            rel: Relationship model, subclass, edge type string, or list of types.
            var: Optional variable alias for the relationship edge.
            edge_type: Keyword argument alias for `rel`.
            variable: Keyword argument alias for `var`.

        Returns:
            The Query instance for fluent chaining.
        """
        types, edge_var = self._extract_rel_info(rel, var, edge_type=edge_type, variable=variable)
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
            elif pred.op == "ne":
                self._native.where_ne(pred.target, pred.field, pred.value)
            elif pred.op == "gt":
                self._native.where_gt(pred.target, pred.field, pred.value)
            elif pred.op == "gte":
                self._native.where_gte(pred.target, pred.field, pred.value)
            elif pred.op == "lt":
                self._native.where_lt(pred.target, pred.field, pred.value)
            elif pred.op == "lte":
                self._native.where_lte(pred.target, pred.field, pred.value)
            elif pred.op == "in":
                self._native.where_in(pred.target, pred.field, pred.value)
            elif pred.op == "not_in":
                self._native.where_not_in(pred.target, pred.field, pred.value)
            elif pred.op == "contains":
                self._native.where_contains(pred.target, pred.field, str(pred.value))
            elif pred.op == "starts_with":
                self._native.where_starts_with(pred.target, pred.field, str(pred.value))
            elif pred.op == "ends_with":
                self._native.where_ends_with(pred.target, pred.field, str(pred.value))
            else:
                msg = f"Unsupported predicate operator: {pred.op}"
                raise ValueError(msg)
        return self

    filter = where

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
        *fields: BoundField | AggregationExpr | str,
        distinct: bool = False,
        **aliased_fields: BoundField | AggregationExpr | str,
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
            elif isinstance(field, AggregationExpr):
                self._native.aggregate(field.target_alias, field.field_name, field.func, None)
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
            elif isinstance(field, AggregationExpr):
                self._native.aggregate(field.target_alias, field.field_name, field.func, alias)
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


def load_csv(url: str, with_headers: bool = True, alias: str = "row") -> Query:
    """Starts a LOAD CSV file ingestion query statement: `LOAD CSV [WITH HEADERS] FROM url AS alias`."""
    return Query.load_csv(url, with_headers=with_headers, alias=alias)
