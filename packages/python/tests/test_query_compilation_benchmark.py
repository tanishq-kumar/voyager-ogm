"""Stage 1 Query Compilation Benchmarks: Voyager OGM vs GQLAlchemy vs Neomodel.

Run benchmarks with:
    uv run pytest packages/python/tests/test_query_compilation_benchmark.py --benchmark-only
"""

from __future__ import annotations

from typing import Any

from voyager_ogm import (
    Field,
    NativeQueryBuilder,
    Node,
    Query,
    Relationship,
    node,
    relationship,
    reset_alias_counters,
)


# ============================================================
# Voyager OGM Entity Definitions
# ============================================================
@node(label="Person")
class Person(Node):
    name: str = Field()
    age: int = Field()


@relationship(type_name="KNOWS")
class Knows(Relationship):
    since: int = Field(default=2020)


@node(label="Movie")
class Movie(Node):
    title: str = Field()
    released: int = Field()


@relationship(type_name="ACTED_IN")
class ActedIn(Relationship):
    role: str = Field()


# ============================================================
# Baseline 1: Pure Python GQLAlchemy-style AST Builder
# (Models Memgraph GQLAlchemy's pure Python query builder)
# ============================================================
class GQLAlchemyStyleBuilder:
    def __init__(self) -> None:
        self.steps: list[dict[str, Any]] = []
        self.params: dict[str, Any] = {}
        self.param_idx = 0

    def match(self) -> GQLAlchemyStyleBuilder:
        self.steps.append({"type": "MATCH"})
        return self

    def node(self, variable: str, label: str) -> GQLAlchemyStyleBuilder:
        self.steps.append({"type": "NODE", "var": variable, "label": label})
        return self

    def to(self, rel_type: str, variable: str) -> GQLAlchemyStyleBuilder:
        self.steps.append({"type": "TO", "rel": rel_type, "var": variable})
        return self

    def where_gt(self, variable: str, prop: str, val: Any) -> GQLAlchemyStyleBuilder:
        p_name = f"p{self.param_idx}"
        self.param_idx += 1
        self.params[p_name] = val
        self.steps.append({"type": "WHERE_GT", "var": variable, "prop": prop, "param": p_name})
        return self

    def return_fields(self, *fields: str) -> GQLAlchemyStyleBuilder:
        self.steps.append({"type": "RETURN", "fields": list(fields)})
        return self

    def limit(self, count: int) -> GQLAlchemyStyleBuilder:
        self.steps.append({"type": "LIMIT", "count": count})
        return self

    def compile(self) -> tuple[str, dict[str, Any]]:
        parts: list[str] = []
        wheres: list[str] = []
        for s in self.steps:
            st = s["type"]
            if st == "MATCH":
                parts.append("MATCH")
            elif st == "NODE":
                parts.append(f"({s['var']}:{s['label']})")
            elif st == "TO":
                parts.append(f"-[{s['var']}:{s['rel']}]->")
            elif st == "WHERE_GT":
                wheres.append(f"{s['var']}.{s['prop']} > ${s['param']}")
            elif st == "RETURN":
                if wheres:
                    parts.append("WHERE " + " AND ".join(wheres))
                    wheres = []
                parts.append("RETURN " + ", ".join(s["fields"]))
            elif st == "LIMIT":
                parts.append(f"LIMIT {s['count']}")
        return " ".join(parts), self.params


# ============================================================
# Baseline 2: Pure Python Neomodel-style NodeSet Builder
# (Models Neo4j Neomodel's NodeSet Cypher query generator)
# ============================================================
class NeomodelStyleNodeSet:
    def __init__(self, node_label: str) -> None:
        self.label = node_label
        self.filters: list[tuple[str, str, Any]] = []
        self.projections: list[str] = []
        self.limit_count: int | None = None
        self.traversals: list[tuple[str, str]] = []

    def filter(self, **kwargs: Any) -> NeomodelStyleNodeSet:
        for k, v in kwargs.items():
            self.filters.append((k, ">", v))
        return self

    def traverse(self, rel_type: str, target_label: str) -> NeomodelStyleNodeSet:
        self.traversals.append((rel_type, target_label))
        return self

    def values(self, *fields: str) -> NeomodelStyleNodeSet:
        self.projections.extend(fields)
        return self

    def limit(self, count: int) -> NeomodelStyleNodeSet:
        self.limit_count = count
        return self

    def compile(self) -> tuple[str, dict[str, Any]]:
        params: dict[str, Any] = {}
        query = f"MATCH (n:{self.label})"
        for i, (rel, target) in enumerate(self.traversals):
            query += f"-[r{i}:{rel}]->(m{i}:{target})"

        if self.filters:
            conds = []
            for j, (prop, op, val) in enumerate(self.filters):
                p_key = f"p{j}"
                params[p_key] = val
                conds.append(f"n.{prop} {op} ${p_key}")
            query += " WHERE " + " AND ".join(conds)

        if self.projections:
            query += " RETURN " + ", ".join(f"n.{f}" for f in self.projections)

        if self.limit_count is not None:
            query += f" LIMIT {self.limit_count}"

        return query, params


# ============================================================
# Benchmark 1: Simple 1-Hop Query Construction & Compilation
# ============================================================
def test_bench_voyager_1hop_cypher_compilation(benchmark: Any) -> None:
    def run():
        p = Person("p")
        m = Movie("m")
        query = (
            Query.match(p)
            .to(ActedIn, var="r")
            .node(m)
            .where(p.age > 21)
            .return_(p.name, m.title)
            .limit(10)
        )
        return query.compile("cypher")

    result = benchmark(run)
    assert "MATCH (p:Person)-[r:ACTED_IN]->(m:Movie)" in result.statement


def test_bench_gqlalchemy_1hop_compilation(benchmark: Any) -> None:
    def run():
        builder = GQLAlchemyStyleBuilder()
        builder.match().node("p", "Person").to("ACTED_IN", "r").node("m", "Movie").where_gt(
            "p", "age", 21
        ).return_fields("p.name", "m.title").limit(10)
        return builder.compile()

    stmt, _ = benchmark(run)
    assert "MATCH (p:Person)" in stmt


def test_bench_neomodel_1hop_compilation(benchmark: Any) -> None:
    def run():
        nodeset = NeomodelStyleNodeSet("Person")
        nodeset.traverse("ACTED_IN", "Movie").filter(age=21).values("name").limit(10)
        return nodeset.compile()

    stmt, _ = benchmark(run)
    assert "MATCH (n:Person)" in stmt


# ============================================================
# Benchmark 2: 10-Hop Deep Graph Traversal Compilation
# ============================================================
def test_bench_voyager_10hop_cypher_compilation(benchmark: Any) -> None:
    def run():
        reset_alias_counters()
        q = Query.match()
        start = Person()
        q.node(start).where(start.age > 21)

        for _ in range(10):
            nxt = Person()
            q.to(Knows).node(nxt).where(nxt.age >= 18)

        q.return_(start.name).limit(50)
        return q.compile("cypher")

    result = benchmark(run)
    assert "MATCH (_person_0:Person)" in result.statement


def test_bench_gqlalchemy_10hop_compilation(benchmark: Any) -> None:
    def run():
        builder = GQLAlchemyStyleBuilder()
        builder.match().node("u0", "Person").where_gt("u0", "age", 21)
        for i in range(1, 11):
            builder.to("KNOWS", f"r{i}").node(f"u{i}", "Person").where_gt(f"u{i}", "age", 18)
        builder.return_fields("u0.name").limit(50)
        return builder.compile()

    stmt, _ = benchmark(run)
    assert "MATCH (u0:Person)" in stmt


# ============================================================
# Benchmark 3: Native Rust Builder Direct Bridge (PyO3)
# ============================================================
def test_bench_voyager_native_rust_direct(benchmark: Any) -> None:
    def run():
        builder = NativeQueryBuilder()
        builder.match()
        builder.node("p", ["Person"])
        builder.to(["ACTED_IN"], "r")
        builder.node("m", ["Movie"])
        builder.where_gt("p", "age", 21)
        builder.return_()
        builder.field("p", "name", None)
        builder.field("m", "title", None)
        builder.limit(10)
        return builder.compile("cypher")

    res = benchmark(run)
    assert "MATCH (p:Person)" in res["statement"]
