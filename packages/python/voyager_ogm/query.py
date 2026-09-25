"""Voyager OGM Fluent Query Builder.

Provides an intuitive, fluent query interface that generates multi-dialect
graph queries (openCypher, SQL:2023 PGQ, ISO GQL) backed by a native Rust AST engine.
"""

from __future__ import annotations

import functools
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


class hybridmethod:  # noqa: N801
    """Descriptor enabling a method to be called either on a class or on an instance.

    When called on a class (e.g. ``Query.match(...)``), it passes the class as the first argument.
    When called on an instance (e.g. ``q.match(...)``), it passes the instance as the first argument.
    """

    def __init__(self, func: Any) -> None:
        self.func = func
        self.__doc__ = func.__doc__
        self.__name__ = getattr(func, "__name__", "hybridmethod")

    def __get__(self, instance: Any, owner: Any) -> Any:
        if instance is None:
            return functools.partial(self.func, owner)
        return functools.partial(self.func, instance)


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
        self._clause_mode: str = "match"
        self._match_clauses: list[dict[str, Any]] = []
        self._mutations: list[tuple[Any, ...] | list[Any]] = []
        self._current_paths: list[list[Any]] = [[]]
        self._where_specs: list[Any] = []
        self._with_clauses: list[dict[str, Any]] = []
        self._projections: list[Any] = []
        self._order_bys: list[Any] = []
        self._skip: int | None = None
        self._limit: int | None = None
        self._distinct: bool = False
        self._is_optional: bool = False
        self._unwinds: list[tuple[str, str]] = []
        self._load_csv: tuple[str, bool, str] | None = None

    def has_mutations(self) -> bool:
        """Returns True if this query contains any mutating clauses (CREATE, MERGE, SET, DELETE, REMOVE)."""
        has_mut = getattr(self._native, "has_mutations", None)
        if callable(has_mut):
            return bool(has_mut())
        return bool(getattr(self, "_mutations", False))

    @staticmethod
    def exists(subquery_or_pattern: Any) -> Any:
        """Creates an existential subquery expression `EXISTS { MATCH ... }`."""
        if hasattr(subquery_or_pattern, "has_mutations") and subquery_or_pattern.has_mutations():
            raise ValueError("Subqueries do not support mutating clauses (CREATE/MERGE/SET/DELETE)")
        elif (
            hasattr(subquery_or_pattern, "_native")
            and hasattr(subquery_or_pattern._native, "has_mutations")
            and subquery_or_pattern._native.has_mutations()
        ):
            raise ValueError("Subqueries do not support mutating clauses (CREATE/MERGE/SET/DELETE)")
        elif hasattr(subquery_or_pattern, "_mutations") and subquery_or_pattern._mutations:
            raise ValueError("Subqueries do not support mutating clauses (CREATE/MERGE/SET/DELETE)")
        from voyager_ogm.fn import exists as fn_exists

        return fn_exists(subquery_or_pattern)

    @staticmethod
    def count(subquery_or_expr: Any) -> Any:
        """Creates a scalar subquery `COUNT { MATCH ... }` or function `count(expr)`."""
        if hasattr(subquery_or_expr, "has_mutations") and subquery_or_expr.has_mutations():
            raise ValueError("Subqueries do not support mutating clauses (CREATE/MERGE/SET/DELETE)")
        elif (
            hasattr(subquery_or_expr, "_native")
            and hasattr(subquery_or_expr._native, "has_mutations")
            and subquery_or_expr._native.has_mutations()
        ):
            raise ValueError("Subqueries do not support mutating clauses (CREATE/MERGE/SET/DELETE)")
        elif hasattr(subquery_or_expr, "_mutations") and subquery_or_expr._mutations:
            raise ValueError("Subqueries do not support mutating clauses (CREATE/MERGE/SET/DELETE)")
        from voyager_ogm.fn import count as fn_count

        return fn_count(subquery_or_expr)

    def _start_match_clause(self, optional: bool = False) -> None:
        self._clause_mode = "match"
        if optional:
            self._is_optional = True
            self._native.optional_match()
        else:
            self._native.match()

        if (
            len(self._match_clauses) == 1
            and not any(self._match_clauses[0]["paths"])
            and not self._match_clauses[0]["where"]
        ):
            self._match_clauses[0]["optional"] = optional
            return

        new_path: list[Any] = []
        clause: dict[str, Any] = {
            "optional": optional,
            "paths": [new_path],
            "where": [],
        }
        self._match_clauses.append(clause)
        if len(self._current_paths) == 1 and not self._current_paths[0]:
            self._current_paths[0] = new_path
        else:
            self._current_paths.append(new_path)

    def _start_create_clause(self) -> None:
        self._clause_mode = "create"
        self._native.create()
        new_path: list[Any] = []
        self._mutations.append(["create", [new_path]])
        if len(self._current_paths) == 1 and not self._current_paths[0]:
            self._current_paths[0] = new_path
        else:
            self._current_paths.append(new_path)

    def _start_merge_clause(self) -> None:
        self._clause_mode = "merge"
        self._native.merge()
        merge_path: list[Any] = []
        on_creates: list[tuple[str, str, Any]] = []
        on_matches: list[tuple[str, str, Any]] = []
        self._mutations.append(["merge", merge_path, on_creates, on_matches])
        if len(self._current_paths) == 1 and not self._current_paths[0]:
            self._current_paths[0] = merge_path
        else:
            self._current_paths.append(merge_path)

    def _record_merge_set(self, kind: str, var: str, prop: str, val_spec: Any) -> None:
        for mut in reversed(self._mutations):
            if mut[0] == "merge":
                if kind == "on_create":
                    mut[2].append((var, prop, val_spec))
                else:
                    mut[3].append((var, prop, val_spec))
                break

    @hybridmethod
    def match(
        self: Any,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Starts or appends a standard MATCH clause.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            The Query instance initialized or chained with the MATCH clause.
        """
        if isinstance(self, type):
            q = self()
        else:
            q = self
        q._start_match_clause(optional=False)
        if node_or_type is not None or labels is not None or variable is not None:
            q.node(node_or_type, labels=labels, variable=variable)
        return q

    @hybridmethod
    def match_node(
        self: Any,
        variable: str | None = None,
        labels: list[str] | str | None = None,
    ) -> Query:
        """Convenience method that begins or appends a MATCH clause for a single node."""
        return self.match(variable=variable, labels=labels)

    @hybridmethod
    def match_patterns(self: Any, *patterns: Any) -> Query:
        """Combines multiple independent path patterns into a single MATCH clause.

        Args:
            *patterns: Query or Path instances representing independent path chains.

        Returns:
            The Query instance with the combined MATCH patterns.
        """
        if isinstance(self, type):
            q = self()
        else:
            q = self

        q._clause_mode = "match"
        q._native.match()
        seen_vars: set[str] = set()
        first_pattern = True

        for pat in patterns:
            if isinstance(pat, Query):
                if pat.has_mutations():
                    raise ValueError("Cannot import mutating query into MATCH clause")
                first_pattern = False
                seen_vars = set(q._native.import_match_patterns(pat._native, list(seen_vars)))
                for w in pat._where_specs:
                    q._where_specs.append(w)
            elif isinstance(pat, Node):
                if not first_pattern:
                    q.pattern()
                first_pattern = False
                if pat.alias and pat.alias in seen_vars:
                    q.node(pat.alias)
                else:
                    if pat.alias:
                        seen_vars.add(pat.alias)
                    q.node(pat)
        return q

    @hybridmethod
    def create(
        self: Any,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Starts or appends a CREATE mutation clause.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            The Query instance initialized or chained with the CREATE clause.
        """
        if isinstance(self, type):
            q = self()
        else:
            q = self
        q._start_create_clause()
        if node_or_type is not None or labels is not None or variable is not None:
            q.node(node_or_type, labels=labels, variable=variable)
        return q

    @hybridmethod
    def merge(
        self: Any,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Starts or appends a MERGE idempotent upsert clause.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            The Query instance initialized or chained with the MERGE clause.
        """
        if isinstance(self, type):
            q = self()
        else:
            q = self
        q._start_merge_clause()
        if node_or_type is not None or labels is not None or variable is not None:
            q.node(node_or_type, labels=labels, variable=variable)
        return q

    @hybridmethod
    def optional_match(
        self: Any,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Starts or appends an OPTIONAL MATCH clause.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            The Query instance initialized or chained with the OPTIONAL MATCH clause.
        """
        if isinstance(self, type):
            q = self()
        else:
            q = self
        q._start_match_clause(optional=True)
        if node_or_type is not None or labels is not None or variable is not None:
            q.node(node_or_type, labels=labels, variable=variable)
        return q

    @hybridmethod
    def call(self: Any, procedure_name: str, *args: Any, **kwargs: Any) -> Query:
        """Starts or appends a vendor procedure call (e.g. APOC or GDS).

        Args:
            procedure_name: Qualified procedure name (e.g. 'apoc.path.expandConfig').
            *args: Positional literal arguments.
            **kwargs: Named parameter key-value pairs.

        Returns:
            The Query instance initialized or chained with the procedure call.
        """
        if isinstance(self, type):
            q = self()
        else:
            q = self
        arg_literals = list(args)
        q._native.call_procedure(procedure_name, arg_literals, kwargs)
        return q

    @hybridmethod
    def unwind(self: Any, batch_param: str, alias: str = "row") -> Query:
        """Starts or appends an UNWIND batch expansion clause: `UNWIND $batch_param AS alias`.

        Args:
            batch_param: Name of the parameter list (e.g. 'batch').
            alias: Row alias name (default: 'row').

        Returns:
            The Query instance initialized or chained with the UNWIND clause.
        """
        if isinstance(self, type):
            q = self()
        else:
            q = self
        param_clean = batch_param.lstrip("$")
        q._unwinds.append((param_clean, alias))
        q._native.unwind(param_clean, alias)
        return q

    @hybridmethod
    def load_csv(
        self: Any,
        url: str,
        with_headers: bool = True,
        alias: str = "row",
    ) -> Query:
        """Starts or appends a LOAD CSV file ingestion clause: `LOAD CSV [WITH HEADERS] FROM url AS alias`.

        Args:
            url: File URL or path (e.g. 'file:///persons.csv').
            with_headers: Whether to parse the first line as column header keys (default: True).
            alias: Row alias variable name (default: 'row').

        Returns:
            The Query instance initialized or chained with the LOAD CSV clause.
        """
        if isinstance(self, type):
            q = self()
        else:
            q = self
        q._load_csv = (url, with_headers, alias)
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
        self._load_csv = (url, with_headers, alias)
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
        param_clean = batch_param.lstrip("$")
        self._unwinds.append((param_clean, alias))
        self._native.unwind(param_clean, alias)
        return self

    def add_create(
        self,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Adds a CREATE mutation clause to the active query statement.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            The Query instance for fluent chaining.
        """
        return self.create(node_or_type=node_or_type, labels=labels, variable=variable)

    def add_merge(
        self,
        node_or_type: Node | type[Node] | str | None = None,
        labels: list[str] | str | None = None,
        variable: str | None = None,
    ) -> Query:
        """Adds a MERGE idempotent upsert clause to the active query statement.

        Args:
            node_or_type: Optional Node instance, subclass, or variable alias.
            labels: Optional label or list of labels.
            variable: Optional variable alias name.

        Returns:
            The Query instance for fluent chaining.
        """
        return self.merge(node_or_type=node_or_type, labels=labels, variable=variable)

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
        return self.match(node_or_type=node_or_type, labels=labels, variable=variable)

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
        return self.optional_match(node_or_type=node_or_type, labels=labels, variable=variable)

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
        if not self._match_clauses and not self._mutations:
            self._start_match_clause(optional=False)

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
        new_path: list[Any] = []
        if self._clause_mode == "create" and self._mutations and self._mutations[-1][0] == "create":
            self._mutations[-1][1].append(new_path)
        elif self._match_clauses:
            self._match_clauses[-1]["paths"].append(new_path)
        self._current_paths.append(new_path)
        return self

    def where(self, *predicates: Any) -> Query:
        """Applies filter predicates to the current query path.

        Args:
            *predicates: Predicate expressions created via operator overloads, boolean combinations,
                or rich function expressions (e.g. `p.age >= 21`, `(p.age > 18) & (p.status == 'ACTIVE')`,
                `Query.exists(subquery)`).

        Returns:
            The Query instance for fluent chaining.

        Raises:
            ValueError: If called with no predicates.
        """
        if not predicates:
            raise ValueError("where() requires at least one predicate condition")
        for pred in predicates:
            if isinstance(pred, Expression) or hasattr(pred, "to_spec"):
                spec = pred.to_spec()
            elif isinstance(pred, PredicateExpr):
                spec = pred.to_spec()
            else:
                spec = to_expression(pred).to_spec()

            if self._clause_mode == "with" and self._with_clauses:
                self._with_clauses[-1]["where"].append(spec)
            elif self._match_clauses:
                self._match_clauses[-1]["where"].append(spec)
                self._where_specs.append(spec)
            else:
                self._where_specs.append(spec)
            self._native.where_expr(spec)
        return self

    def to_spec(self) -> Any:
        """Converts the query AST into a dictionary or path list spec for native subquery embedding."""
        paths = [
            [tuple(x) if isinstance(x, list) else x for x in p] for p in self._current_paths if p
        ]
        if (
            not self._mutations
            and not self._where_specs
            and not self._projections
            and not self._with_clauses
            and not self._unwinds
            and not self._load_csv
            and not self._order_bys
            and self._skip is None
            and self._limit is None
            and not self._distinct
            and not self._is_optional
            and len(self._match_clauses) <= 1
            and len(paths) == 1
        ):
            return paths[0]

        matches_spec: list[dict[str, Any]] = []
        for clause in self._match_clauses:
            clause_paths = [
                [tuple(x) if isinstance(x, list) else x for x in p]
                for p in clause.get("paths", [])
                if p
            ]
            if clause_paths or clause.get("where"):
                matches_spec.append(
                    {
                        "optional": clause.get("optional", False),
                        "paths": clause_paths,
                        "where": clause.get("where", []),
                    }
                )

        if not matches_spec and paths and not self._mutations:
            matches_spec.append(
                {
                    "optional": self._is_optional,
                    "paths": paths,
                    "where": self._where_specs,
                }
            )

        spec: dict[str, Any] = {
            "matches": matches_spec,
            "with_clauses": self._with_clauses,
            "projections": self._projections,
            "order_by": self._order_bys,
            "skip": self._skip,
            "limit": self._limit,
            "distinct": self._distinct,
        }

        if self._mutations:
            mut_specs: list[tuple[Any, ...]] = []
            for m in self._mutations:
                tag = m[0]
                if tag == "create":
                    raw_paths = m[1]
                    norm_paths = [
                        [tuple(x) if isinstance(x, list) else x for x in p] for p in raw_paths if p
                    ]
                    mut_specs.append(("create", norm_paths))
                elif tag == "merge":
                    raw_path = m[1]
                    norm_path = [tuple(x) if isinstance(x, list) else x for x in raw_path]
                    mut_specs.append(("merge", norm_path, list(m[2]), list(m[3])))
                else:
                    mut_specs.append(tuple(m))
            spec["mutations"] = mut_specs

        if self._unwinds:
            spec["unwinds"] = self._unwinds
        if self._load_csv:
            spec["load_csv"] = self._load_csv
        return spec

    filter = where

    def where_not(self, *predicates: Any) -> Query:
        """Applies negated filter predicates `NOT (pred)` to the current query path.

        Args:
            *predicates: Predicate expressions to negate.

        Returns:
            The Query instance for fluent chaining.

        Raises:
            ValueError: If called with no predicates.
        """
        if not predicates:
            raise ValueError("where_not() requires at least one predicate condition")
        for pred in predicates:
            expr = to_expression(pred)
            spec = (~expr).to_spec()
            if self._clause_mode == "with" and self._with_clauses:
                self._with_clauses[-1]["where"].append(spec)
            elif self._match_clauses:
                self._match_clauses[-1]["where"].append(spec)
                self._where_specs.append(spec)
            else:
                self._where_specs.append(spec)
            self._native.where_expr(spec)
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
            val_spec = (
                assign.value.to_spec()
                if hasattr(assign.value, "to_spec")
                else to_expression(assign.value).to_spec()
            )
            self._record_merge_set("on_create", assign.target, assign.field, val_spec)
        for key, val in kwargs.items():
            if "." in key:
                var, prop = key.split(".", 1)
                self._native.on_create_set(var, prop, val)
                val_spec = (
                    val.to_spec() if hasattr(val, "to_spec") else to_expression(val).to_spec()
                )
                self._record_merge_set("on_create", var, prop, val_spec)
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
            val_spec = (
                assign.value.to_spec()
                if hasattr(assign.value, "to_spec")
                else to_expression(assign.value).to_spec()
            )
            self._record_merge_set("on_match", assign.target, assign.field, val_spec)
        for key, val in kwargs.items():
            if "." in key:
                var, prop = key.split(".", 1)
                self._native.on_match_set(var, prop, val)
                val_spec = (
                    val.to_spec() if hasattr(val, "to_spec") else to_expression(val).to_spec()
                )
                self._record_merge_set("on_match", var, prop, val_spec)
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
                val_spec = (
                    assign.value.to_spec()
                    if hasattr(assign.value, "to_spec")
                    else to_expression(assign.value).to_spec()
                )
                self._mutations.append(("set", assign.target, assign.field, val_spec))
            elif isinstance(assign, Node):
                for field_name, val in assign.dirty_fields.items():
                    self._native.set_property(assign.alias, field_name, val)
                    val_spec = (
                        val.to_spec() if hasattr(val, "to_spec") else to_expression(val).to_spec()
                    )
                    self._mutations.append(("set", assign.alias, field_name, val_spec))
        for key, val in kwargs.items():
            if "." in key:
                var, prop = key.split(".", 1)
                self._native.set_property(var, prop, val)
                val_spec = (
                    val.to_spec() if hasattr(val, "to_spec") else to_expression(val).to_spec()
                )
                self._mutations.append(("set", var, prop, val_spec))
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
        self._mutations.append(("delete", False, names))
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
        self._mutations.append(("delete", True, names))
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
                self._mutations.append(("remove", p.target_alias, p.field_name))
            elif isinstance(p, str) and "." in p:
                var, prop = p.split(".", 1)
                self._native.remove_property(var, prop)
                self._mutations.append(("remove", var, prop))
        return self

    def _project_field(self, field: Any, alias: str | None = None) -> tuple[Any, ...]:
        if isinstance(field, AliasedExpr):
            final_alias = field.alias if alias is None else alias
            expr_spec = field.expr.to_spec()
            self._native.select_expr(expr_spec, final_alias)
            return ("expr", expr_spec, final_alias)
        elif isinstance(field, AggregationExpr):
            self._native.aggregate(field.target_alias, field.field_name, field.func, alias)
            return ("agg", field.target_alias, field.field_name, field.func, alias)
        elif isinstance(field, BoundField):
            self._native.field(field.target_alias, field.field_name, alias)
            return ("field", field.target_alias, field.field_name, alias)
        elif isinstance(field, Expression):
            expr_spec = field.to_spec()
            self._native.select_expr(expr_spec, alias)
            return ("expr", expr_spec, alias)
        elif isinstance(field, Field) or (hasattr(field, "name") and not hasattr(field, "alias")):
            var_name = getattr(field, "target_alias", "") or ""
            name = getattr(field, "name", "") or ""
            self._native.field(var_name, name, alias)
            return ("field", var_name, name, alias)
        elif isinstance(field, str):
            parts = field.split()
            if len(parts) == 3 and parts[1].upper() == "AS":
                final_alias = alias or parts[2]
                if "." in parts[0]:
                    var_prop = parts[0].split(".", 1)
                    self._native.field(var_prop[0], var_prop[1], final_alias)
                    return ("field", var_prop[0], var_prop[1], final_alias)
                else:
                    self._native.field(parts[0], "", final_alias)
                    return ("field", parts[0], "", final_alias)
            elif "." in parts[0]:
                var_prop = parts[0].split(".", 1)
                self._native.field(var_prop[0], var_prop[1], alias)
                return ("field", var_prop[0], var_prop[1], alias)
            else:
                self._native.field(parts[0], "", alias)
                return ("field", parts[0], "", alias)
        else:
            expr_spec = to_expression(field).to_spec()
            self._native.select_expr(expr_spec, alias)
            return ("expr", expr_spec, alias)

    def with_(
        self,
        *fields: Any,
        distinct: bool = False,
        **aliased_fields: Any,
    ) -> Query:
        """Starts or appends an intermediate `WITH` projection pipeline.

        Args:
            *fields: Positional fields, expressions, or raw strings to project.
            distinct: If True, emits `WITH DISTINCT`.
            **aliased_fields: Keyword arguments mapping custom alias names to fields.

        Returns:
            The Query instance for fluent chaining.

        Raises:
            ValueError: If called with no projection fields.

        Example:
            >>> query = (
            ...     Query.match(p)
            ...     .with_(p.name, p.age)
            ...     .where(p.age > 21)
            ...     .return_(p.name)
            ... )
        """
        if not fields and not aliased_fields:
            raise ValueError("with_() requires at least one projection field")

        self._clause_mode = "with"
        self._native.with_()
        with_entry: dict[str, Any] = {
            "distinct": distinct,
            "projections": [],
            "where": [],
            "order_by": [],
            "skip": None,
            "limit": None,
        }
        if distinct:
            self._native.distinct()

        for field in fields:
            proj = self._project_field(field, None)
            with_entry["projections"].append(proj)

        for alias, field in aliased_fields.items():
            proj = self._project_field(field, alias)
            with_entry["projections"].append(proj)

        self._with_clauses.append(with_entry)
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
        self._clause_mode = "return"
        self._native.return_()
        if distinct:
            self._distinct = True
            self._native.distinct()

        for field in fields:
            proj = self._project_field(field, None)
            self._projections.append(proj)

        for alias, field in aliased_fields.items():
            proj = self._project_field(field, alias)
            self._projections.append(proj)

        return self

    def distinct(self) -> Query:
        """Enables DISTINCT on the active RETURN or WITH clause.

        Returns:
            The Query instance for fluent chaining.
        """
        if self._clause_mode == "with" and self._with_clauses:
            self._with_clauses[-1]["distinct"] = True
        else:
            self._distinct = True
        self._native.distinct()
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
            order_spec: Any = (field.target_alias, field.field_name, ascending)
            self._native.order_by(field.target_alias, field.field_name, ascending)
        elif isinstance(field, Expression) or hasattr(field, "to_spec"):
            order_spec = (field.to_spec(), ascending)
            self._native.order_by_expr(field.to_spec(), ascending)
        elif isinstance(field, str) and "." in field:
            var_prop = field.split(".", 1)
            order_spec = (var_prop[0], var_prop[1], ascending)
            self._native.order_by(var_prop[0], var_prop[1], ascending)
        elif isinstance(field, str):
            order_spec = (field, "", ascending)
            self._native.order_by(field, "", ascending)
        else:
            expr_spec = to_expression(field).to_spec()
            order_spec = (expr_spec, ascending)
            self._native.order_by_expr(expr_spec, ascending)

        if self._clause_mode == "with" and self._with_clauses:
            self._with_clauses[-1]["order_by"].append(order_spec)
        else:
            self._order_bys.append(order_spec)
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
            count: Maximum number of rows to return. Must be non-negative.

        Returns:
            The Query instance for fluent chaining.

        Raises:
            ValueError: If count is negative.
        """
        if count < 0:
            raise ValueError(f"limit count must be non-negative, got {count}")
        if self._clause_mode == "with" and self._with_clauses:
            self._with_clauses[-1]["limit"] = count
        else:
            self._limit = count
        self._native.limit(count)
        return self

    def skip(self, count: int) -> Query:
        """Skips the first N rows for pagination.

        Args:
            count: Number of rows to skip. Must be non-negative.

        Returns:
            The Query instance for fluent chaining.

        Raises:
            ValueError: If count is negative.
        """
        if count < 0:
            raise ValueError(f"skip count must be non-negative, got {count}")
        if self._clause_mode == "with" and self._with_clauses:
            self._with_clauses[-1]["skip"] = count
        else:
            self._skip = count
        self._native.skip(count)
        return self

    def offset(self, count: int) -> Query:
        """Skips the first N rows for pagination (SQL/GQL alias for skip).

        Args:
            count: Number of rows to skip. Must be non-negative.

        Returns:
            The Query instance for fluent chaining.

        Raises:
            ValueError: If count is negative.
        """
        if count < 0:
            raise ValueError(f"offset count must be non-negative, got {count}")
        return self.skip(count)

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


class Path(Query):
    """Path pattern specification builder for graph topologies.

    Used to define independent path chains for multi-pattern MATCH clauses
    via ``Query.match_patterns(p1, p2)``.

    Example:
        >>> p1 = Path.match(u).to("KNOWS").node(f)
        >>> p2 = Path.match(u).to("WORKS_AT").node(c)
        >>> query = Query.match_patterns(p1, p2).where(c.country == "UK").return_(u.name, f.name)
    """

    pass


def unwind(batch_param: str, alias: str = "row") -> Query:
    """Starts an UNWIND batch expansion query statement: `UNWIND $batch_param AS alias`."""
    return Query.unwind(batch_param, alias=alias)


def load_csv(url: str, with_headers: bool = True, alias: str = "row") -> Query:
    """Starts a LOAD CSV file ingestion query statement: `LOAD CSV [WITH HEADERS] FROM url AS alias`."""
    return Query.load_csv(url, with_headers=with_headers, alias=alias)


__all__ = [
    "CompiledQuery",
    "Path",
    "Query",
    "hybridmethod",
    "load_csv",
    "unwind",
]
