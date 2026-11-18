"""Voyager OGM: Enterprise Multi-Dialect Query Compilation & Dialect Cheat-Sheet.

Teaches:
    - Multi-label node entity definitions (`@node(label=["Person", "Engineer"])`)
    - Dialect name cheat-sheet:
        * `"cypher"`: openCypher 9 / Cypher 25 (Neo4j, Memgraph, FalkorDB)
        * `"sql_pgq"`: SQL:2023 Property Graph Queries (`GRAPH_TABLE`) (DuckDB, PostgreSQL)
        * `"iso_gql"`: ISO/IEC 39075:2024 Graph Query Language (ISO GQL standard)
        * `"age"`: Apache AGE Cypher-in-SQL extension for PostgreSQL
    - How multi-labels compile differently: `:Person:Engineer` (Cypher) vs `IS Person IS Engineer` (PGQ) vs `Person&Engineer` (GQL)
    - Specifying graph schema names for SQL:2023 PGQ (`graph_name="infra_graph"`)

Prerequisites:
    - Engine: None (runs offline / compile-only; no database needed)
    - Extras: None

Run with:
    `uv run python examples/python/09_enterprise_dialects.py`
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

    # 2. Compile to SQL:2023 PGQ (for DuckDB / PostgreSQL)
    pgq = query.compile("sql_pgq", graph_name="infra_graph")
    print("[2] SQL:2023 PGQ GRAPH_TABLE (for DuckDB / PostgreSQL):")
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
