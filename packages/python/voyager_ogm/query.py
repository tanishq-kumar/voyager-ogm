"""Voyager OGM Fluent Query Builder.

Provides an intuitive, fluent query interface that generates multi-dialect
graph queries (openCypher, SQL:2023 PGQ, ISO GQL) backed by a native Rust AST engine.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from voyager_ogm._voyager_rs import NativeQueryBuilder
from voyager_ogm.expressions import AliasedExpr, Expression, to_expression
from voyager_ogm.models import (
    AggregationExpr,
    BoundField,
    Field,
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
        self._optimize: bool | None = None
        self._optimization_level: str | None = None
        self._current_paths: list[list[Any]] = [[]]
        self._where_specs: list[Any] = []
        self._projections: list[Any] = []
        self._order_bys: list[Any] = []
        self._skip: int | None = None
        self._limit: int | None = None
        self._distinct: bool = False
        self._is_optional: bool = False
        self._unwinds: list[tuple[str, str]] = []
        self._load_csv: tuple[str, bool, str] | None = None

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
        var_name: str | None = None
        lbls: list[str] = []
        if isinstance(node_or_var, Node):
            var_name = node_or_var.alias
            lbls = node_or_var.labels
            self._native.node(node_or_var.alias, node_or_var.labels)
        elif isinstance(node_or_var, type) and issubclass(node_or_var, Node):
            instance = node_or_var()
            var_name = instance.alias
            lbls = instance.labels
            self._native.node(instance.alias, instance.labels)
        elif isinstance(node_or_var, str):
            var_name = node_or_var
            lbls = [labels] if isinstance(labels, str) else (labels or [])
            self._native.node(node_or_var, lbls)
        elif labels is not None:
            lbls = [labels] if isinstance(labels, str) else labels
            self._native.node(None, lbls)
        else:
            self._native.node(None, [])
        self._current_paths[-1].append(("node", var_name, lbls))
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
        self._current_paths[-1].append(["edge", "out", types, edge_var, 1, 1])
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
        self._current_paths[-1].append(["edge", "in", types, edge_var, 1, 1])
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
        self._current_paths[-1].append(["edge", "undirected", types, edge_var, 1, 1])
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
        if (
            self._current_paths[-1]
            and isinstance(self._current_paths[-1][-1], list)
            and self._current_paths[-1][-1][0] == "edge"
        ):
            self._current_paths[-1][-1][4] = min_hops
            self._current_paths[-1][-1][5] = max_hops
        return self

    def pattern(self) -> Query:
        """Separates multiple graph patterns within the same MATCH or CREATE clause: `MATCH p1, p2, p3`.

        Returns:
            The Query instance for fluent chaining.
        """
        self._native.pattern()
        self._current_paths.append([])
        return self

    def where(self, *predicates: Any) -> Query:
        """Applies filter predicates to the current query path.

        Args:
            *predicates: Predicate expressions created via operator overloads, boolean combinations,
                or rich function expressions (e.g. `p.age >= 21`, `(p.age > 18) & (p.status == 'ACTIVE')`,
                `fn.exists(subquery)`).

        Returns:
            The Query instance for fluent chaining.
        """
        for pred in predicates:
            if isinstance(pred, Expression) or hasattr(pred, "to_spec"):
                spec = pred.to_spec()
                self._where_specs.append(spec)
                self._native.where_expr(spec)
            elif isinstance(pred, PredicateExpr):
                spec = pred.to_spec()
                self._where_specs.append(spec)
                self._native.where_expr(spec)
            else:
                spec = to_expression(pred).to_spec()
                self._where_specs.append(spec)
                self._native.where_expr(spec)
        return self

    def to_spec(self) -> Any:
        """Converts the query AST into a dictionary or path list spec for native subquery embedding."""
        paths = [
            [tuple(x) if isinstance(x, list) else x for x in p] for p in self._current_paths if p
        ]
        if not self._where_specs and not self._projections and len(paths) == 1:
            return paths[0]
        return {
            "matches": [
                {
                    "optional": self._is_optional,
                    "paths": paths,
                    "where": self._where_specs,
                }
            ],
            "projections": self._projections,
            "order_by": self._order_bys,
            "skip": self._skip,
            "limit": self._limit,
            "distinct": self._distinct,
        }

    filter = where

    def where_not(self, *predicates: Any) -> Query:
        """Applies negated filter predicates `NOT (pred)` to the current query path."""
        for pred in predicates:
            expr = to_expression(pred)
            self._native.where_expr((~expr).to_spec())
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
        *fields: Any,
        distinct: bool = False,
        **aliased_fields: Any,
    ) -> Query:
        """Initializes column projections for the RETURN clause.

        Args:
            *fields: Positional fields, expressions, or raw strings to project.
            distinct: If True, emits `RETURN DISTINCT`.
            **aliased_fields: Keyword arguments mapping custom alias names to fields.

        Returns:
            The Query instance for fluent chaining.

        Example:
            >>> query.return_(p.name, p.age, distinct=True, user_city=p.city)
            >>> query.return_(fn.to_lower(p.name).as_("lower_name"), p.age + 5)
        """
        self._native.return_()
        if distinct:
            self._native.distinct()

        for field in fields:
            if isinstance(field, AliasedExpr):
                self._native.select_expr(field.expr.to_spec(), field.alias)
            elif isinstance(field, AggregationExpr):
                self._native.aggregate(field.target_alias, field.field_name, field.func, None)
            elif isinstance(field, BoundField):
                self._native.field(field.target_alias, field.field_name, None)
            elif isinstance(field, Expression):
                self._native.select_expr(field.to_spec(), None)
            elif isinstance(field, Field) or (
                hasattr(field, "name") and not hasattr(field, "alias")
            ):
                self._native.field("", field.name or "", None)
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
            else:
                self._native.select_expr(to_expression(field).to_spec(), None)

        for alias, field in aliased_fields.items():
            if isinstance(field, AliasedExpr):
                self._native.select_expr(field.expr.to_spec(), alias)
            elif isinstance(field, AggregationExpr):
                self._native.aggregate(field.target_alias, field.field_name, field.func, alias)
            elif isinstance(field, BoundField):
                self._native.field(field.target_alias, field.field_name, alias)
            elif isinstance(field, Expression):
                self._native.select_expr(field.to_spec(), alias)
            elif isinstance(field, Field) or (
                hasattr(field, "name") and not hasattr(field, "alias")
            ):
                self._native.field("", field.name or "", alias)
            elif isinstance(field, str) and "." in field:
                var_prop = field.split(".")
                self._native.field(var_prop[0], var_prop[1], alias)
            elif isinstance(field, str):
                self._native.field(field, "", alias)
            else:
                self._native.select_expr(to_expression(field).to_spec(), alias)
        return self

    def order_by(self, field: Any, ascending: bool = True) -> Query:
        """Sorts the query results by a field or expression.

        Args:
            field: Field, expression, or property string to sort by.
            ascending: If True sorts ascending; if False sorts descending.

        Returns:
            The Query instance for fluent chaining.
        """
        if isinstance(field, BoundField):
            self._native.order_by(field.target_alias, field.field_name, ascending)
        elif isinstance(field, Expression) or hasattr(field, "to_spec"):
            self._native.order_by_expr(field.to_spec(), ascending)
        elif isinstance(field, str) and "." in field:
            var_prop = field.split(".")
            self._native.order_by(var_prop[0], var_prop[1], ascending)
        return self

    def order_by_desc(self, field: Any) -> Query:
        """Sorts the query results descending.

        Args:
            field: Field, expression, or property string to sort by descending.

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

    def optimize(self, level: str = "standard") -> Query:
        """Enables rule-based AST query optimization.

        Hoists single-node equality filters into inline pattern property maps
        (`(p:Person {city: $p0})`), simplifies boolean expressions, and prunes
        unreferenced intermediate variables.

        Args:
            level: Optimization level ('none', 'standard', 'aggressive'). Defaults to 'standard'.

        Returns:
            The Query instance for fluent chaining.

        Example:
            >>> query = Query.match(p).where(p.city == "NY").optimize()
            >>> compiled = query.compile("cypher")
        """
        self._optimize = True
        self._optimization_level = level
        return self

    def compile(
        self,
        dialect: str = "cypher",
        graph_name: str | None = None,
        optimize: bool | None = None,
        optimization_level: str | None = None,
    ) -> CompiledQuery:
        """Compiles the AST query into a parameterized dialect query statement.

        Args:
            dialect: Target dialect name ('cypher', 'sql_pgq', 'iso_gql').
            graph_name: Optional graph table name for SQL:2023 PGQ queries.
            optimize: Optional override to enable/disable AST optimization.
            optimization_level: Optional override for optimization level ('none', 'standard', 'aggressive').

        Returns:
            CompiledQuery containing the parameterized statement and parameter map.

        Raises:
            ValueError: If the requested dialect is unsupported.

        Example:
            >>> compiled = query.compile("cypher", optimize=True)
        """
        from voyager_ogm.config import get_config

        cfg = get_config()
        if optimize is not None:
            opt = optimize
        elif self._optimize is not None:
            opt = self._optimize
        else:
            opt = cfg.optimize

        if optimization_level is not None:
            opt_level = optimization_level
        elif self._optimization_level is not None:
            opt_level = self._optimization_level
        else:
            opt_level = cfg.optimization_level

        res = self._native.compile(
            dialect,
            graph_name,
            optimize=opt,
            optimization_level=opt_level,
        )
        return CompiledQuery(statement=res["statement"], parameters=res["parameters"])

    def execute(self, session: Any, parameters: dict[str, Any] | None = None) -> Any:
        """Executes this query against a Voyager Session or database bridge.

        Args:
            session: Voyager Session or database bridge instance.
            parameters: Optional additional runtime parameters.

        Returns:
            ExecutionResult containing rows, SQLAlchemy mappings/scalars access, and graph entity extraction.

        Example:
            >>> result = query.execute(session)
            >>> rows = result.mappings().all()
            >>> viewer = result.show()
        """
        return session.execute(self, parameters=parameters)

    def show(self, session: Any | None = None, **kwargs: Any) -> Any:
        """Visualizes this query in an interactive notebook GraphViewer widget.

        Args:
            session: Optional Voyager Session or DatabaseBridge to execute against.
            **kwargs: Additional styling or layout options passed to GraphViewer.

        Returns:
            Interactive GraphViewer component.
        """
        from voyager_ogm.viewer import GraphViewer

        return GraphViewer.from_query(self, session=session, **kwargs)


def unwind(batch_param: str, alias: str = "row") -> Query:
    """Starts an UNWIND batch expansion query statement: `UNWIND $batch_param AS alias`."""
    return Query.unwind(batch_param, alias=alias)


def load_csv(url: str, with_headers: bool = True, alias: str = "row") -> Query:
    """Starts a LOAD CSV file ingestion query statement: `LOAD CSV [WITH HEADERS] FROM url AS alias`."""
    return Query.load_csv(url, with_headers=with_headers, alias=alias)
