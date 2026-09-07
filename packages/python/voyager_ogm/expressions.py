"""Voyager OGM Rich Expression Engine.

Provides an AST expression tree builder in Python with comprehensive operator overloads,
arithmetic calculations, functions, conditional CASE WHEN expressions, and comprehensions.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any


class Expression:
    """Base class for all Voyager OGM AST expressions."""

    def to_spec(self) -> Any:
        """Converts this expression into an AST descriptor tuple recognized by the native Rust FFI engine."""
        raise NotImplementedError

    def as_(self, alias: str) -> AliasedExpr:
        """Assigns a projection alias name to this expression in RETURN clauses."""
        return AliasedExpr(self, alias)

    # Arithmetic Operator Overloads
    def __add__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "+", to_expression(other))

    def __radd__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(to_expression(other), "+", self)

    def __sub__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "-", to_expression(other))

    def __rsub__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(to_expression(other), "-", self)

    def __mul__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "*", to_expression(other))

    def __rmul__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(to_expression(other), "*", self)

    def __truediv__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "/", to_expression(other))

    def __rtruediv__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(to_expression(other), "/", self)

    def __mod__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "%", to_expression(other))

    def __rmod__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(to_expression(other), "%", self)

    def __neg__(self) -> UnaryExpr:
        return UnaryExpr("-", self)

    # Boolean & Bitwise Operator Overloads
    def __and__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "AND", to_expression(other))

    def __rand__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(to_expression(other), "AND", self)

    def __or__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "OR", to_expression(other))

    def __ror__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(to_expression(other), "OR", self)

    def __xor__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "XOR", to_expression(other))

    def __rxor__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(to_expression(other), "XOR", self)

    def __invert__(self) -> UnaryExpr:
        return UnaryExpr("NOT", self)

    # Comparison Operator Overloads
    def __eq__(self, other: Any) -> BinaryExpr:  # ty: ignore[invalid-method-override] # type: ignore[override]
        return BinaryExpr(self, "=", to_expression(other))

    def __ne__(self, other: Any) -> BinaryExpr:  # ty: ignore[invalid-method-override] # type: ignore[override]
        return BinaryExpr(self, "!=", to_expression(other))

    def __lt__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "<", to_expression(other))

    def __le__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, "<=", to_expression(other))

    def __gt__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, ">", to_expression(other))

    def __ge__(self, other: Any) -> BinaryExpr:
        return BinaryExpr(self, ">=", to_expression(other))

    # Extended Predicates
    def in_(self, other: Any) -> BinaryExpr:
        """Constructs an `IN` containment predicate expression."""
        if isinstance(other, (list, tuple, set)):
            return BinaryExpr(self, "IN", ListLiteralExpr([to_expression(x) for x in other]))
        return BinaryExpr(self, "IN", to_expression(other))

    def not_in(self, other: Any) -> BinaryExpr:
        """Constructs a `NOT IN` non-containment predicate expression."""
        if isinstance(other, (list, tuple, set)):
            return BinaryExpr(self, "NOT IN", ListLiteralExpr([to_expression(x) for x in other]))
        return BinaryExpr(self, "NOT IN", to_expression(other))

    def contains(self, substring: Any) -> BinaryExpr:
        """Constructs a string `CONTAINS` predicate expression."""
        return BinaryExpr(self, "CONTAINS", to_expression(substring))

    def startswith(self, prefix: Any) -> BinaryExpr:
        """Constructs a string `STARTS WITH` predicate expression."""
        return BinaryExpr(self, "STARTS WITH", to_expression(prefix))

    def endswith(self, suffix: Any) -> BinaryExpr:
        """Constructs a string `ENDS WITH` predicate expression."""
        return BinaryExpr(self, "ENDS WITH", to_expression(suffix))

    def regex(self, pattern: Any) -> BinaryExpr:
        """Constructs a regular expression `=~` matching predicate expression."""
        return BinaryExpr(self, "=~", to_expression(pattern))

    def is_null(self) -> UnaryExpr:
        """Constructs an `IS NULL` unary predicate expression."""
        return UnaryExpr("IS NULL", self)

    def is_not_null(self) -> UnaryExpr:
        """Constructs an `IS NOT NULL` unary predicate expression."""
        return UnaryExpr("IS NOT NULL", self)


class PropExpr(Expression):
    """Property expression accessing a property on an aliased node or edge variable: `var.prop`."""

    def __init__(self, target_alias: str, field_name: str) -> None:
        self.target_alias = target_alias
        self.field_name = field_name
        self.target = target_alias
        self.field = field_name

    def to_spec(self) -> tuple[str, str, str]:
        """Converts this property access into an AST spec descriptor tuple."""
        return ("prop", self.target_alias, self.field_name)

    def __repr__(self) -> str:
        return f"{self.target_alias}.{self.field_name}"


class IdentExpr(Expression):
    """Identifier expression referencing a variable directly: `var`."""

    def __init__(self, name: str) -> None:
        self.name = name

    def to_spec(self) -> tuple[str, str]:
        """Converts this identifier into an AST spec descriptor tuple."""
        return ("ident", self.name)

    def __repr__(self) -> str:
        return self.name


class ParamExpr(Expression):
    """Parameter reference expression: `$param`."""

    def __init__(self, name: str) -> None:
        self.name = name

    def to_spec(self) -> tuple[str, str]:
        """Converts this parameter reference into an AST spec descriptor tuple."""
        return ("param", self.name)

    def __repr__(self) -> str:
        return f"${self.name}"


class LiteralExpr(Expression):
    """Literal scalar or list value: `42`, `'Alice'`, `True`, `null`."""

    def __init__(self, value: Any) -> None:
        self.value = value

    def to_spec(self) -> tuple[str, Any]:
        """Converts this literal value into an AST spec descriptor tuple."""
        return ("lit", self.value)

    def __repr__(self) -> str:
        return repr(self.value)


class ListLiteralExpr(Expression):
    """List literal of expression items: `[expr1, expr2, ...]`."""

    def __init__(self, items: Sequence[Expression]) -> None:
        self.items = list(items)

    def to_spec(self) -> tuple[str, list[Any]]:
        """Converts this list literal into an AST spec descriptor tuple."""
        return ("list", [item.to_spec() for item in self.items])

    def __repr__(self) -> str:
        return f"[{', '.join(repr(i) for i in self.items)}]"


class BinaryExpr(Expression):
    """Binary expression connecting two expressions with an operator: `left OP right`."""

    def __init__(self, left: Expression, op: str, right: Expression) -> None:
        self.left = left
        self.op = op
        self.right = right

        # Backward compatibility attributes for PredicateExpr
        if isinstance(left, PropExpr):
            self.target = left.target_alias
            self.field = left.field_name
            self.value = getattr(right, "value", right)

    def to_spec(self) -> tuple[str, str, Any, Any]:
        """Converts this binary expression into an AST spec descriptor tuple."""
        return ("bin", self.op, self.left.to_spec(), self.right.to_spec())

    def __repr__(self) -> str:
        return f"({self.left!r} {self.op} {self.right!r})"


class UnaryExpr(Expression):
    """Unary prefix or postfix expression: `NOT expr`, `-expr`, `expr IS NULL`."""

    def __init__(self, op: str, operand: Expression) -> None:
        self.op = op
        self.operand = operand

    def to_spec(self) -> tuple[str, str, Any]:
        """Converts this unary expression into an AST spec descriptor tuple."""
        return ("unary", self.op, self.operand.to_spec())

    def __repr__(self) -> str:
        if self.op.upper() in ("IS NULL", "IS NOT NULL"):
            return f"({self.operand!r} {self.op})"
        return f"({self.op} {self.operand!r})"


class FunctionExpr(Expression):
    """Function invocation expression: `func(arg1, arg2, ...)`."""

    def __init__(self, name: str, args: Sequence[Expression]) -> None:
        self.name = name
        self.args = list(args)

    def to_spec(self) -> tuple[str, str, list[Any]]:
        """Converts this function invocation into an AST spec descriptor tuple."""
        return ("fn", self.name, [a.to_spec() for a in self.args])

    def __repr__(self) -> str:
        args_str = ", ".join(repr(a) for a in self.args)
        return f"{self.name}({args_str})"


class CaseExpr(Expression):
    """CASE WHEN conditional branching expression."""

    def __init__(
        self,
        operand: Expression | None,
        branches: Sequence[tuple[Expression, Expression]],
        else_expr: Expression | None = None,
    ) -> None:
        self.operand = operand
        self.branches = list(branches)
        self.else_expr = else_expr

    def to_spec(self) -> tuple[str, Any, list[tuple[Any, Any]], Any]:
        """Converts this CASE WHEN expression into an AST spec descriptor tuple."""
        operand_spec = self.operand.to_spec() if self.operand is not None else None
        branches_spec = [(w.to_spec(), t.to_spec()) for w, t in self.branches]
        else_spec = self.else_expr.to_spec() if self.else_expr is not None else None
        return ("case", operand_spec, branches_spec, else_spec)

    def __repr__(self) -> str:
        op_str = f" {self.operand!r}" if self.operand is not None else ""
        branches_str = " ".join(f"WHEN {w!r} THEN {t!r}" for w, t in self.branches)
        else_str = f" ELSE {self.else_expr!r}" if self.else_expr is not None else ""
        return f"CASE{op_str} {branches_str}{else_str} END"


class CaseBuilder:
    """Fluent builder for constructing `CASE WHEN` conditional expressions."""

    def __init__(self, operand: Any | None = None) -> None:
        self._operand = to_expression(operand) if operand is not None else None
        self._branches: list[tuple[Expression, Expression]] = []
        self._else: Expression | None = None

    def when(self, condition: Any, result: Any) -> CaseBuilder:
        """Adds a `WHEN condition THEN result` branch."""
        self._branches.append((to_expression(condition), to_expression(result)))
        return self

    def else_(self, result: Any) -> CaseExpr:
        """Adds the fallback `ELSE result` branch and finishes the CASE expression."""
        self._else = to_expression(result)
        return self.end()

    def end(self) -> CaseExpr:
        """Finishes the CASE expression without an ELSE branch."""
        return CaseExpr(self._operand, self._branches, self._else)


class ListCompExpr(Expression):
    """List comprehension expression: `[x IN list WHERE cond | map_expr]`."""

    def __init__(
        self,
        var: str,
        list_expr: Expression,
        where_filter: Expression | None = None,
        map_expr: Expression | None = None,
    ) -> None:
        self.var = var
        self.list_expr = list_expr
        self.where_filter = where_filter
        self.map_expr = map_expr

    def to_spec(self) -> tuple[str, str, Any, Any, Any]:
        """Converts this list comprehension into an AST spec descriptor tuple."""
        where_spec = self.where_filter.to_spec() if self.where_filter is not None else None
        map_spec = self.map_expr.to_spec() if self.map_expr is not None else None
        return ("list_comp", self.var, self.list_expr.to_spec(), where_spec, map_spec)

    def __repr__(self) -> str:
        wh_str = f" WHERE {self.where_filter!r}" if self.where_filter is not None else ""
        map_str = f" | {self.map_expr!r}" if self.map_expr is not None else ""
        return f"[{self.var} IN {self.list_expr!r}{wh_str}{map_str}]"


class PatternCompExpr(Expression):
    """Pattern comprehension expression: `[(pattern) WHERE cond | proj]`."""

    def __init__(
        self,
        path: Any,
        proj: Expression,
        where_filter: Expression | None = None,
    ) -> None:
        self.path = path
        self.proj = proj
        self.where_filter = where_filter

    def to_spec(self) -> tuple[str, Any, Any, Any]:
        """Converts this pattern comprehension into an AST spec descriptor tuple."""
        path_spec = self.path.to_spec() if hasattr(self.path, "to_spec") else self.path
        where_spec = self.where_filter.to_spec() if self.where_filter is not None else None
        return ("pattern_comp", path_spec, where_spec, self.proj.to_spec())

    def __repr__(self) -> str:
        wh_str = f" WHERE {self.where_filter!r}" if self.where_filter is not None else ""
        return f"[({self.path!r}){wh_str} | {self.proj!r}]"


class SubqueryExpr(Expression):
    """Existential or Scalar count subquery expression: `EXISTS { MATCH ... }` or `COUNT { MATCH ... }`."""

    def __init__(self, kind: str, subquery: Any) -> None:
        self.kind = kind
        self.subquery = subquery

    def to_spec(self) -> tuple[str, Any]:
        """Converts this subquery into an AST spec descriptor tuple."""
        sub_spec = self.subquery.to_spec() if hasattr(self.subquery, "to_spec") else self.subquery
        return (self.kind, sub_spec)

    def __repr__(self) -> str:
        return f"{self.kind.upper()} {{ {self.subquery!r} }}"


class AliasedExpr(Expression):
    """Expression with a projection alias: `expr AS alias`."""

    def __init__(self, expr: Expression, alias: str) -> None:
        self.expr = expr
        self.alias = alias

    def to_spec(self) -> Any:
        """Converts the inner aliased expression into an AST spec descriptor tuple."""
        return self.expr.to_spec()

    def __repr__(self) -> str:
        return f"{self.expr!r} AS {self.alias}"


def to_expression(val: Any) -> Expression:
    """Coerces any Python object into a Voyager OGM AST `Expression`."""
    if isinstance(val, Expression):
        return val
    if hasattr(val, "_alias"):
        return IdentExpr(val._alias)
    if hasattr(val, "target_alias") and hasattr(val, "field_name"):
        return PropExpr(str(val.target_alias), str(val.field_name))
    if hasattr(val, "alias") and isinstance(val.alias, str):
        return IdentExpr(val.alias)
    if hasattr(val, "name") and not hasattr(val, "alias"):
        return PropExpr("", str(val.name or ""))
    return LiteralExpr(val)


def ident(name: str) -> IdentExpr:
    """Creates an identifier expression."""
    return IdentExpr(name)


def col(name: str) -> IdentExpr:
    """Creates a column identifier expression."""
    return IdentExpr(name)


def prop(var: str, prop_name: str) -> PropExpr:
    """Creates a property access expression `var.prop`."""
    return PropExpr(var, prop_name)


def param(name: str) -> ParamExpr:
    """Creates a parameter reference expression `$name`."""
    return ParamExpr(name.lstrip("$"))


def lit(val: Any) -> LiteralExpr:
    """Creates a literal value expression."""
    return LiteralExpr(val)


def list_comprehension(
    var: str | IdentExpr,
    list_expr: Any,
    where_filter: Any = None,
    map_expr: Any = None,
) -> ListCompExpr:
    """Creates a list comprehension expression `[x IN list WHERE cond | map_expr]`."""
    var_name = var.name if isinstance(var, IdentExpr) else str(var)
    wh = to_expression(where_filter) if where_filter is not None else None
    mp = to_expression(map_expr) if map_expr is not None else None
    return ListCompExpr(var_name, to_expression(list_expr), wh, mp)


def pattern_comprehension(
    path: Any,
    proj: Any,
    where_filter: Any = None,
) -> PatternCompExpr:
    """Creates a pattern comprehension expression `[(pattern) WHERE cond | proj]`."""
    wh = to_expression(where_filter) if where_filter is not None else None
    return PatternCompExpr(path, to_expression(proj), wh)


def case(operand: Any | None = None) -> CaseBuilder:
    """Creates a fluent `CASE WHEN` expression builder."""
    return CaseBuilder(operand)
