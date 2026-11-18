"""Voyager OGM: Multi-Dialect Query Compilation.

Teaches:
    - Compiling a single graph AST into multiple target graph query languages
    - openCypher 9 / Cypher 25 for Neo4j and Memgraph
    - SQL:2023 PGQ (`GRAPH_TABLE`) for DuckDB and PostgreSQL
    - ISO/IEC 39075:2024 GQL standard syntax
    - Parameter mapping and variable-length traversal conversion across dialects

Prerequisites:
    - Engine: None (runs offline / compile-only; no database needed)
    - Extras: None

Run with:
    `uv run python examples/python/08_multi_dialect_compilation.py`
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

    # 1. openCypher (Neo4j, Memgraph)
    cypher = query.compile("cypher")
    print("[1] openCypher 9 / Cypher 25:")
    print(f"    {cypher.statement}")
    print(f"    Params: {cypher.parameters}\n")

    # 2. SQL:2023 PGQ (DuckDB, PostgreSQL)
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
