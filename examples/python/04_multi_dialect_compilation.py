"""Approach 4: Universal Multi-Dialect Query Compilation.

Compiles a single graph AST into openCypher 9/25, SQL:2023 PGQ, and ISO GQL.
Run with: `uv run python examples/python/04_multi_dialect_compilation.py`
"""

from __future__ import annotations

from voyager_ogm import Query, node, relationship


@node("Person")
class Person:
    name: str
    age: int


@node("Movie")
class Movie:
    title: str
    released: int


@relationship("ACTED_IN")
class ActedIn:
    pass


def main() -> None:
    print("=== Voyager OGM (Python) - Approach 4: Multi-Dialect Compilation ===\n")

    p = Person("p")
    m = Movie("m")
    r = ActedIn("r")

    query = (
        Query.match(p)
        .to(r)
        .hops(1, 3)
        .node(m)
        .where(
            p.age > 21,
            m.released >= 2000,
        )
        .return_(p.name, m.title)
        .order_by(p.name)
        .limit(10)
    )

    # 1. openCypher (Neo4j, Memgraph, RedisGraph, AgensGraph)
    cypher = query.compile("cypher")
    print("[1] openCypher 9 / Cypher 25:")
    print(f"    {cypher.statement}")
    print(f"    Params: {cypher.parameters}\n")

    # 2. SQL:2023 PGQ (PostgreSQL 19, DuckPGQ, Oracle 23ai)
    pgq = query.compile("sql_pgq", graph_name="movie_network")
    print("[2] SQL:2023 PGQ (GRAPH_TABLE):")
    print(f"    {pgq.statement}")
    print(f"    Params: {pgq.parameters}\n")

    # 3. ISO/IEC 39075:2024 GQL Standard
    gql = query.compile("iso_gql")
    print("[3] ISO/IEC 39075:2024 GQL:")
    print(f"    {gql.statement}")
    print(f"    Params: {gql.parameters}\n")


if __name__ == "__main__":
    main()
