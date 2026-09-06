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
