"""Voyager OGM Standard Functions & Scalar/Temporal/Subquery Expression Library.

Provides high-level functional wrappers for Cypher/ISO GQL standard functions,
temporal math, existential/count subqueries, and conditional branching.
"""

from __future__ import annotations

from typing import Any

from voyager_ogm.expressions import (
    CaseBuilder,
    Expression,
    FunctionExpr,
    SubqueryExpr,
    to_expression,
)


# String Functions
def to_lower(expr: Any) -> FunctionExpr:
    """Emits `toLower(expr)`."""
    return FunctionExpr("toLower", [to_expression(expr)])


def to_upper(expr: Any) -> FunctionExpr:
    """Emits `toUpper(expr)`."""
    return FunctionExpr("toUpper", [to_expression(expr)])


def trim(expr: Any) -> FunctionExpr:
    """Emits `trim(expr)`."""
    return FunctionExpr("trim", [to_expression(expr)])


def ltrim(expr: Any) -> FunctionExpr:
    """Emits `ltrim(expr)`."""
    return FunctionExpr("ltrim", [to_expression(expr)])


def rtrim(expr: Any) -> FunctionExpr:
    """Emits `rtrim(expr)`."""
    return FunctionExpr("rtrim", [to_expression(expr)])


def split(expr: Any, delimiter: Any) -> FunctionExpr:
    """Emits `split(expr, delimiter)`."""
    return FunctionExpr("split", [to_expression(expr), to_expression(delimiter)])


def substring(expr: Any, start: Any, length: Any | None = None) -> FunctionExpr:
    """Emits `substring(expr, start, length)`."""
    args = [to_expression(expr), to_expression(start)]
    if length is not None:
        args.append(to_expression(length))
    return FunctionExpr("substring", args)


def left(expr: Any, count_expr: Any) -> FunctionExpr:
    """Emits `left(expr, count)`."""
    return FunctionExpr("left", [to_expression(expr), to_expression(count_expr)])


def right(expr: Any, count_expr: Any) -> FunctionExpr:
    """Emits `right(expr, count)`."""
    return FunctionExpr("right", [to_expression(expr), to_expression(count_expr)])


def replace(expr: Any, old: Any, new: Any) -> FunctionExpr:
    """Emits `replace(expr, old, new)`."""
    return FunctionExpr("replace", [to_expression(expr), to_expression(old), to_expression(new)])


def reverse(expr: Any) -> FunctionExpr:
    """Emits `reverse(expr)`."""
    return FunctionExpr("reverse", [to_expression(expr)])


def length(expr: Any) -> FunctionExpr:
    """Emits `length(expr)`."""
    return FunctionExpr("length", [to_expression(expr)])


def coalesce(*args: Any) -> FunctionExpr:
    """Emits `coalesce(arg1, arg2, ...)`."""
    return FunctionExpr("coalesce", [to_expression(a) for a in args])


def size(expr: Any) -> FunctionExpr:
    """Emits `size(expr)`."""
    return FunctionExpr("size", [to_expression(expr)])


# List & Array Functions
def head(expr: Any) -> FunctionExpr:
    """Emits `head(expr)`."""
    return FunctionExpr("head", [to_expression(expr)])


def tail(expr: Any) -> FunctionExpr:
    """Emits `tail(expr)`."""
    return FunctionExpr("tail", [to_expression(expr)])


def last(expr: Any) -> FunctionExpr:
    """Emits `last(expr)`."""
    return FunctionExpr("last", [to_expression(expr)])


def range_(start: Any, end: Any, step: Any | None = None) -> FunctionExpr:
    """Emits `range(start, end[, step])`."""
    args = [to_expression(start), to_expression(end)]
    if step is not None:
        args.append(to_expression(step))
    return FunctionExpr("range", args)


def keys(expr: Any) -> FunctionExpr:
    """Emits `keys(expr)`."""
    return FunctionExpr("keys", [to_expression(expr)])


def properties(expr: Any) -> FunctionExpr:
    """Emits `properties(expr)`."""
    return FunctionExpr("properties", [to_expression(expr)])


# Mathematical / Scalar Functions
def abs_(expr: Any) -> FunctionExpr:
    """Emits `abs(expr)`."""
    return FunctionExpr("abs", [to_expression(expr)])


def ceil(expr: Any) -> FunctionExpr:
    """Emits `ceil(expr)`."""
    return FunctionExpr("ceil", [to_expression(expr)])


def floor(expr: Any) -> FunctionExpr:
    """Emits `floor(expr)`."""
    return FunctionExpr("floor", [to_expression(expr)])


def round_(expr: Any, precision: Any | None = None) -> FunctionExpr:
    """Emits `round(expr[, precision])`."""
    args = [to_expression(expr)]
    if precision is not None:
        args.append(to_expression(precision))
    return FunctionExpr("round", args)


def sign(expr: Any) -> FunctionExpr:
    """Emits `sign(expr)`."""
    return FunctionExpr("sign", [to_expression(expr)])


def sqrt(expr: Any) -> FunctionExpr:
    """Emits `sqrt(expr)`."""
    return FunctionExpr("sqrt", [to_expression(expr)])


def power(base: Any, exponent: Any) -> FunctionExpr:
    """Emits `power(base, exponent)` or `^` exponent."""
    return FunctionExpr("power", [to_expression(base), to_expression(exponent)])


def exp(expr: Any) -> FunctionExpr:
    """Emits `exp(expr)`."""
    return FunctionExpr("exp", [to_expression(expr)])


def log(expr: Any) -> FunctionExpr:
    """Emits `log(expr)` (natural logarithm)."""
    return FunctionExpr("log", [to_expression(expr)])


def log10(expr: Any) -> FunctionExpr:
    """Emits `log10(expr)`."""
    return FunctionExpr("log10", [to_expression(expr)])


def sin(expr: Any) -> FunctionExpr:
    """Emits `sin(expr)`."""
    return FunctionExpr("sin", [to_expression(expr)])


def cos(expr: Any) -> FunctionExpr:
    """Emits `cos(expr)`."""
    return FunctionExpr("cos", [to_expression(expr)])


def tan(expr: Any) -> FunctionExpr:
    """Emits `tan(expr)`."""
    return FunctionExpr("tan", [to_expression(expr)])


def asin(expr: Any) -> FunctionExpr:
    """Emits `asin(expr)`."""
    return FunctionExpr("asin", [to_expression(expr)])


def acos(expr: Any) -> FunctionExpr:
    """Emits `acos(expr)`."""
    return FunctionExpr("acos", [to_expression(expr)])


def atan(expr: Any) -> FunctionExpr:
    """Emits `atan(expr)`."""
    return FunctionExpr("atan", [to_expression(expr)])


def atan2(y: Any, x: Any) -> FunctionExpr:
    """Emits `atan2(y, x)`."""
    return FunctionExpr("atan2", [to_expression(y), to_expression(x)])


def degrees(expr: Any) -> FunctionExpr:
    """Emits `degrees(expr)`."""
    return FunctionExpr("degrees", [to_expression(expr)])


def radians(expr: Any) -> FunctionExpr:
    """Emits `radians(expr)`."""
    return FunctionExpr("radians", [to_expression(expr)])


def pi() -> FunctionExpr:
    """Emits `pi()`."""
    return FunctionExpr("pi", [])


def rand() -> FunctionExpr:
    """Emits `rand()`."""
    return FunctionExpr("rand", [])


# Temporal Functions
def datetime(expr: Any | None = None) -> FunctionExpr:
    """Emits `datetime()` or `datetime(expr)`."""
    args = [to_expression(expr)] if expr is not None else []
    return FunctionExpr("datetime", args)


def date(expr: Any | None = None) -> FunctionExpr:
    """Emits `date()` or `date(expr)`."""
    args = [to_expression(expr)] if expr is not None else []
    return FunctionExpr("date", args)


def time(expr: Any | None = None) -> FunctionExpr:
    """Emits `time()` or `time(expr)`."""
    args = [to_expression(expr)] if expr is not None else []
    return FunctionExpr("time", args)


def date_trunc(unit: str, expr: Any) -> FunctionExpr:
    """Emits `date.truncate(unit, expr)`."""
    return FunctionExpr("date.truncate", [to_expression(unit), to_expression(expr)])


def duration(spec: Any) -> FunctionExpr:
    """Emits `duration(spec)`."""
    return FunctionExpr("duration", [to_expression(spec)])


def duration_between(d1: Any, d2: Any) -> FunctionExpr:
    """Emits `duration.between(d1, d2)`."""
    return FunctionExpr("duration.between", [to_expression(d1), to_expression(d2)])


# Type Conversion & Reflection
def to_integer(expr: Any) -> FunctionExpr:
    """Emits `toInteger(expr)`."""
    return FunctionExpr("toInteger", [to_expression(expr)])


def to_float(expr: Any) -> FunctionExpr:
    """Emits `toFloat(expr)`."""
    return FunctionExpr("toFloat", [to_expression(expr)])


def to_string(expr: Any) -> FunctionExpr:
    """Emits `toString(expr)`."""
    return FunctionExpr("toString", [to_expression(expr)])


def to_boolean(expr: Any) -> FunctionExpr:
    """Emits `toBoolean(expr)`."""
    return FunctionExpr("toBoolean", [to_expression(expr)])


def type_(expr: Any) -> FunctionExpr:
    """Emits `type(r)` for relationship types."""
    return FunctionExpr("type", [to_expression(expr)])


def labels(expr: Any) -> FunctionExpr:
    """Emits `labels(n)` for node labels."""
    return FunctionExpr("labels", [to_expression(expr)])


def element_id(expr: Any) -> FunctionExpr:
    """Emits `elementId(n)` for entity element IDs."""
    return FunctionExpr("elementId", [to_expression(expr)])


# Graph & Path Functions
def nodes(path: Any) -> FunctionExpr:
    """Emits `nodes(path)` returning all nodes along a path."""
    return FunctionExpr("nodes", [to_expression(path)])


def relationships(path: Any) -> FunctionExpr:
    """Emits `relationships(path)` returning all relationships along a path."""
    return FunctionExpr("relationships", [to_expression(path)])


def start_node(rel: Any) -> FunctionExpr:
    """Emits `startNode(r)`."""
    return FunctionExpr("startNode", [to_expression(rel)])


def end_node(rel: Any) -> FunctionExpr:
    """Emits `endNode(r)`."""
    return FunctionExpr("endNode", [to_expression(rel)])


def shortest_path(path_pattern: Any) -> FunctionExpr:
    """Emits `shortestPath(path_pattern)`."""
    return FunctionExpr("shortestPath", [to_expression(path_pattern)])


def all_shortest_paths(path_pattern: Any) -> FunctionExpr:
    """Emits `allShortestPaths(path_pattern)`."""
    return FunctionExpr("allShortestPaths", [to_expression(path_pattern)])


# Subqueries & Conditionals
def exists(subquery_or_pattern: Any) -> SubqueryExpr:
    """Emits an existential subquery `EXISTS { MATCH ... }`."""
    return SubqueryExpr("exists", subquery_or_pattern)


def count(subquery_or_expr: Any) -> Expression:
    """Emits scalar subquery `COUNT { MATCH ... }` if given a Query/subquery, or `count(expr)` function."""
    from voyager_ogm.query import Query

    if isinstance(subquery_or_expr, Query):
        return SubqueryExpr("count", subquery_or_expr)
    return FunctionExpr("count", [to_expression(subquery_or_expr)])


def case(operand: Any | None = None) -> CaseBuilder:
    """Creates a fluent `CASE WHEN` expression builder."""
    return CaseBuilder(operand)


# Universal Dynamic & Custom Function Dispatch
def call(name: str, *args: Any) -> FunctionExpr:
    """Invokes any arbitrary user-defined function (UDF), vendor function, or database extension.

    Args:
        name: The exact database function name (e.g. 'apoc.text.clean', 'custom_math_op').
        *args: Expression arguments passed to the function.

    Returns:
        FunctionExpr representing the function call AST node.

    Example:
        >>> fn.call("apoc.text.clean", p.name)
        >>> fn.call("vector.similarity.cosine", p.embedding, target_vec)
    """
    return FunctionExpr(name, [to_expression(a) for a in args])


def __getattr__(name: str) -> Any:
    """Dynamically resolves any function call on `fn` (similar to SQLAlchemy's `func.*`).

    Allows calling arbitrary database functions without requiring them to be explicitly
    defined in Voyager OGM.

    Args:
        name: Name of the function accessed on `fn`.

    Returns:
        Callable that constructs a `FunctionExpr` with the given function name.

    Example:
        >>> fn.my_custom_udf(p.age, 42)
        >>> fn.custom_geo_distance(p.lat, p.lon)
    """

    def _dynamic_fn(*args: Any) -> FunctionExpr:
        return FunctionExpr(name, [to_expression(a) for a in args])

    return _dynamic_fn
