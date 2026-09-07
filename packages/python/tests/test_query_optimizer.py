"""Tests for the rule-based AST Query Optimizer & Predicate Pushdown Pass."""

import pytest
from voyager_ogm import Field, Query, node, relationship


@node(label="Person")
class Person:
    name: str = Field()
    age: int = Field()
    city: str = Field(default="London")


@relationship(type_name="WORKS_AT")
class WorksAt:
    since: int = Field(default=2020)


@node(label="Company")
class Company:
    name: str = Field()
    industry: str = Field(default="Tech")


def test_optimizer_predicate_pushdown_single_node():
    """Verify single-node equality filters are hoisted into inline property maps."""
    p = Person("p")
    query = Query.match(p).where(p.city == "New York").return_(p.name).optimize()

    compiled = query.compile("cypher")
    assert "(p:Person {city: $p0})" in compiled.statement
    assert "WHERE" not in compiled.statement
    assert compiled.parameters == {"p0": "New York"}


def test_optimizer_compile_flag_pushdown():
    """Verify compile(optimize=True) parameter works equivalently."""
    p = Person("p")
    query = Query.match(p).where(p.city == "Berlin").return_(p.name)

    # Without optimize: standard WHERE clause
    unopt = query.compile("cypher")
    assert "WHERE" in unopt.statement
    assert "{city:" not in unopt.statement

    # With optimize=True: inlined property map
    opt = query.compile("cypher", optimize=True)
    assert "(p:Person {city: $p0})" in opt.statement
    assert "WHERE" not in opt.statement
    assert opt.parameters == {"p0": "Berlin"}


def test_optimizer_multi_hop_pushdown():
    """Verify pushdown across multi-hop node patterns."""
    p = Person("p")
    w = WorksAt("w")
    c = Company("c")

    query = (
        Query.match(p)
        .to(w)
        .node(c)
        .where(p.city == "Tokyo", c.name == "Acme Corp", p.age >= 21)
        .return_(p.name, company_name=c.name)
        .optimize()
    )

    compiled = query.compile("cypher")
    assert "(p:Person {city: $p0})" in compiled.statement
    assert "(c:Company {name: $p1})" in compiled.statement
    assert "WHERE p.age >= $p2" in compiled.statement
    assert compiled.parameters == {"p0": "Tokyo", "p1": "Acme Corp", "p2": 21}


def test_optimizer_iso_gql_pushdown():
    """Verify ISO GQL syntax compatibility with optimized inlined property maps."""
    p = Person("p")
    query = Query.match(p).where(p.city == "Paris").return_(p.name).optimize()

    compiled = query.compile("iso_gql")
    assert "(p:Person {city: $p0})" in compiled.statement
    assert "WHERE" not in compiled.statement


def test_optimizer_optimization_levels():
    """Verify optimization levels: None, Standard, Aggressive."""
    p = Person("p")
    query = Query.match(p).where(p.city == "London").return_(p.name)

    # None: unoptimized
    res_none = query.compile("cypher", optimize=True, optimization_level="none")
    assert "WHERE" in res_none.statement

    # Standard: hoisted
    res_std = query.compile("cypher", optimize=True, optimization_level="standard")
    assert "{city: $p0}" in res_std.statement


def test_sql_pgq_rejects_mutations():
    """Verify SQL:2023 PGQ rejects mutations with an informative error."""
    p = Person("p")
    p.name = "Alice"
    query = Query.create(p)

    with pytest.raises(ValueError, match="DML mutations"):
        query.compile("sql_pgq")


def test_optimizer_branching_patterns_pushdown():
    """Verify pushdown across branching multi-match query patterns."""
    p = Person("p")
    w = WorksAt("w")
    c = Company("c")
    p2 = Person("p2")

    query = (
        Query.match(p)
        .to(w)
        .node(c)
        .add_match(p)
        .to(w)
        .node(p2)
        .where(
            p.city == "London",
            c.name == "Acme Corp",
            p2.city == "Paris",
            p.age >= 30,
        )
        .return_(p.name, company=c.name, colleague=p2.name)
        .optimize()
    )

    compiled = query.compile("cypher")
    # All three single-node equalities are hoisted into their respective node patterns
    assert "city: $" in compiled.statement
    assert "(c:Company {name: $" in compiled.statement
    assert "(p2:Person {city: $" in compiled.statement
    # Range inequality is safely kept in WHERE clause
    assert "WHERE p.age >= $" in compiled.statement


def test_optimizer_with_functions_and_rich_expressions():
    """Verify optimizer correctly partitions constant property equalities from function & rich expressions."""
    from voyager_ogm import case, fn

    p = Person("p")
    w = WorksAt("w")
    c = Company("c")

    query = (
        Query.match(p)
        .to(w)
        .node(c)
        .where(
            c.name == "Acme Corp",  # Pushdown eligible -> hoisted to {name: $p0}
            fn.to_upper(p.name) == "ALICE",  # Function call -> MUST stay in WHERE
            (p.age + 5) >= 30,  # Arithmetic expr -> MUST stay in WHERE
        )
        .return_(
            name=fn.to_upper(p.name),
            future_age=p.age + 5,
            tier=case().when(p.age >= 40, "Senior").else_("Junior"),
        )
        .optimize()
    )

    compiled = query.compile("cypher")
    # 1. Company name is hoisted into inline pattern map
    assert "(c:Company {name: $p0})" in compiled.statement
    # 2. Function condition is preserved in WHERE
    assert "toUpper(p.name) = $p1" in compiled.statement
    # 3. Arithmetic condition is preserved in WHERE
    assert (
        "(p.age + $p2) >= $p3" in compiled.statement or "p.age + $p2 >= $p3" in compiled.statement
    )
    # 4. Inlined pattern MUST NOT contain invalid syntax like {name: toUpper(...)}
    assert "{name: toUpper" not in compiled.statement


def test_optimizer_anti_optimizations_and_safety_guards():
    """Verify optimizer refuses to apply unsafe transformations (anti-optimizations)."""
    p1 = Person("p1")
    p2 = Person("p2")

    # Guard 1: Cross-variable equality (p1.city == p2.city) MUST NOT be hoisted into property maps
    q1 = (
        Query.match(p1)
        .add_match(p2)
        .where(p1.city == p2.city, p1.name == "Alice")
        .return_(p1.name, p2.name)
        .optimize()
    )
    compiled1 = q1.compile("cypher")
    assert "(p1:Person {name: $p0})" in compiled1.statement
    assert "WHERE p1.city = p2.city" in compiled1.statement
    assert "{city: p2.city}" not in compiled1.statement

    # Guard 2: OPTIONAL MATCH WHERE clauses referencing non-optional outer variables MUST NOT be hoisted
    # into the main MATCH, as doing so would alter left-outer-join NULL production semantics
    q2 = (
        Query.match(p1)
        .add_optional_match(p2)
        .where(p1.city == "Berlin")
        .return_(p1.name, p2.name)
        .optimize()
    )
    compiled2 = q2.compile("cypher")
    assert "WHERE p1.city = $p0" in compiled2.statement
    assert "(p1:Person {city:" not in compiled2.statement


def test_optimizer_rhs_functions_over_parameters_hoisting():
    """Verify functions evaluated on parameters/literals (RHS) ARE hoisted into pattern maps."""
    from voyager_ogm import fn

    p = Person("p")
    # p.city == fn.to_upper("london") -> (p:Person {city: toUpper($p0)})
    q = Query.match(p).where(p.city == fn.to_upper("london")).return_(p.name).optimize()

    compiled = q.compile("cypher")
    assert "(p:Person {city: toUpper($p0)})" in compiled.statement
    assert "WHERE" not in compiled.statement
    assert compiled.parameters == {"p0": "london"}
