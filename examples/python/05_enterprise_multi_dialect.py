"""Level 5: Enterprise - Universal Multi-Dialect Query Compilation.

Learn how to write ONE graph query in Python and compile it into:
  1. openCypher 9 / Cypher 25 (Neo4j, Memgraph, RedisGraph)
  2. SQL:2023 PGQ (DuckPGQ, PostgreSQL 19, Oracle 23ai)
  3. ISO/IEC 39075:2024 GQL (Official ISO standard)

Run with: `uv run python examples/python/05_enterprise_multi_dialect.py`
"""

from __future__ import annotations

from voyager_ogm import Query, node, relationship


@node(label=["Person", "Engineer"])
class Engineer:
    name: str
    experience_years: int


@node(label="Service")
class Microservice:
    name: str
    language: str


@relationship(type_name="MAINTAINS")
class Maintains:
    on_call: bool = True


def main() -> None:
    print("=" * 65)
    print("[Level 5: Enterprise] Universal Multi-Dialect Compilation")
    print("=" * 65)

    eng = Engineer("eng")
    svc = Microservice("svc")
    maint = Maintains("rel")

    # Build graph query once in Python
    query = (
        Query.match(eng)
        .to(maint)
        .hops(1, 2)
        .node(svc)
        .where(
            eng.experience_years >= 5,
            svc.language == "Rust",
        )
        .return_(eng.name, svc.name, on_call=maint.on_call)
        .order_by(eng.name)
        .limit(20)
    )

    # 1. Compile to openCypher 9 / Cypher 25
    cypher = query.compile("cypher")
    print("\n[1] openCypher 9 / Cypher 25 (for Neo4j / Memgraph):")
    print(f"    {cypher.statement}")
    print(f"    Parameters: {cypher.parameters}\n")

    # 2. Compile to SQL:2023 PGQ (for DuckPGQ / PostgreSQL 19)
    pgq = query.compile("sql_pgq", graph_name="infra_graph")
    print("[2] SQL:2023 PGQ GRAPH_TABLE (for DuckDB / PostgreSQL 19):")
    print(f"    {pgq.statement}")
    print(f"    Parameters: {pgq.parameters}\n")

    # 3. Compile to ISO/IEC 39075:2024 GQL
    gql = query.compile("iso_gql")
    print("[3] ISO/IEC 39075:2024 GQL (Official ISO Standard):")
    print(f"    {gql.statement}")
    print(f"    Parameters: {gql.parameters}\n")

    print("=" * 65)


if __name__ == "__main__":
    main()
