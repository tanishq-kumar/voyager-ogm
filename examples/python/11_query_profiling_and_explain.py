"""Voyager OGM: Query Profiling and Plan Explanation (EXPLAIN & PROFILE).

Teaches:
    - Fluent query profiling with `.explain()` and `.profile()`
    - Functional wrappers `explain(query)` and `profile(query)`
    - Multi-dialect keyword translation:
        * openCypher (Neo4j / Memgraph): `EXPLAIN` and `PROFILE`
        * ISO/IEC 39075:2024 GQL: `EXPLAIN` and `PROFILE`
        * SQL:2023 PGQ (DuckDB / PostgreSQL): `EXPLAIN` and `EXPLAIN ANALYZE`
        * Apache AGE: `EXPLAIN SELECT ...` and `EXPLAIN ANALYZE SELECT ...`
    - MockBridge execution and live engine profiling when Neo4j is available

Prerequisites:
    - Engine: None for offline AST profiling; optional live Neo4j on bolt://127.0.0.1:7687
    - Extras: None (or `neo4j` for optional live profiling)

Run with:
    `uv run python examples/python/11_query_profiling_and_explain.py`
"""

from __future__ import annotations

import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

from voyager_ogm import (
    Field,
    MockBridge,
    Node,
    Query,
    Relationship,
    Session,
    create_bridge,
    explain,
    node,
    profile,
    relationship,
)


@node("User")
class User(Node):
    user_id: str = Field(primary_key=True)
    name: str
    active: bool = True
    tier: str = "standard"


@relationship("FRIENDS_WITH")
class FriendsWith(Relationship):
    since: int = 2024


def main():
    print("=== Voyager OGM: Query Profiling & Plan Explanation (Issue #54) ===\n")

    u = User("u")
    f = User("f")
    r = FriendsWith("r")

    # -------------------------------------------------------------------------
    # 1. Base Query Construction
    # -------------------------------------------------------------------------
    base_query = (
        Query.match(u)
        .to(r)
        .node(f)
        .where(u.active == True, u.tier == "enterprise")  # noqa: E712
        .return_(u.name, f.name)
    )

    print(f"Base Query Execution Mode: {base_query.execution_mode}")
    assert base_query.execution_mode == "normal"

    # -------------------------------------------------------------------------
    # 2. Fluent Method Chaining (.explain() and .profile())
    # -------------------------------------------------------------------------
    print("\n--- 1. Fluent Query Modifiers ---")

    # .explain() marks query for plan explanation
    q_fluent_explain = base_query.clone().explain()
    print(f"Fluent .explain() Mode: {q_fluent_explain.execution_mode}")
    assert q_fluent_explain.execution_mode == "explain"

    # .profile() marks query for execution profiling
    q_fluent_profile = base_query.clone().profile()
    print(f"Fluent .profile() Mode: {q_fluent_profile.execution_mode}")
    assert q_fluent_profile.execution_mode == "profile"

    # Composable: .explain().profile() -> explain_and_profile
    q_fluent_both = base_query.clone().explain().profile()
    print(f"Fluent .explain().profile() Mode: {q_fluent_both.execution_mode}")
    assert q_fluent_both.execution_mode == "explain_and_profile"

    # -------------------------------------------------------------------------
    # 3. Functional Style with Immutability (explain(q) and profile(q))
    # -------------------------------------------------------------------------
    print("\n--- 2. Functional Style (Pure & Immutable) ---")

    # Functional style returns a modified clone, keeping original untouched
    pure_explain = explain(base_query)
    pure_profile = profile(base_query)
    pure_both = explain(profile(base_query))

    print(f"Original base_query mode:  {base_query.execution_mode} (remains unmutated)")
    print(f"explain(base_query) mode:  {pure_explain.execution_mode}")
    print(f"profile(base_query) mode:  {pure_profile.execution_mode}")
    print(f"explain(profile(q)) mode:  {pure_both.execution_mode}")

    assert base_query.execution_mode == "normal"
    assert pure_explain.execution_mode == "explain"
    assert pure_profile.execution_mode == "profile"
    assert pure_both.execution_mode == "explain_and_profile"

    # -------------------------------------------------------------------------
    # 4. Multi-Dialect Keyword Translation
    # -------------------------------------------------------------------------
    print("\n--- 3. Multi-Dialect Keyword Translation ---")

    dialects = [
        ("cypher", "openCypher (Neo4j / Memgraph)"),
        ("iso_gql", "ISO/IEC GQL"),
        ("sql_pgq", "SQL:2023 PGQ (PostgreSQL / DuckDB)"),
        ("age", "Apache AGE (PostgreSQL Extension)"),
    ]

    print("\n[EXPLAIN Statements across Dialects]")
    for dialect, label in dialects:
        compiled = q_fluent_explain.compile(dialect=dialect)
        print(f"-> {label}:\n   {compiled.statement}\n")

    print("[PROFILE Statements across Dialects]")
    for dialect, label in dialects:
        compiled = q_fluent_profile.compile(dialect=dialect)
        print(f"-> {label}:\n   {compiled.statement}\n")

    # -------------------------------------------------------------------------
    # 5. Session Execution with Driver Bridge (MockBridge)
    # -------------------------------------------------------------------------
    print("--- 4. Session Execution with Database Bridge ---")

    mock_driver = MockBridge()
    session = Session(bridge=mock_driver, dialect="cypher")

    # Executing explain query through session
    print("Executing explain(base_query) via Session...")
    explain(base_query).execute(session)
    last_stmt, _ = mock_driver.executed_queries[-1]
    print(f"Driver received: {last_stmt}")
    assert last_stmt.startswith("EXPLAIN ")

    # Executing profile query through session
    print("Executing profile(base_query) via Session...")
    profile(base_query).execute(session)
    last_stmt, _ = mock_driver.executed_queries[-1]
    print(f"Driver received: {last_stmt}")
    assert last_stmt.startswith("PROFILE ")

    # -------------------------------------------------------------------------
    # 6. Live Engine Execution (Neo4j on localhost:7687)
    # -------------------------------------------------------------------------
    print("\n--- 5. Live Engine Execution (Neo4j) ---")
    uri = "bolt://127.0.0.1:7687"
    try:
        bridge = create_bridge(uri, fallback="mock")
        if not bridge.ping():
            print(
                f"[SKIP] Neo4j engine is not reachable on {uri} (start with `docker compose -f containers/compose.yaml up -d neo4j` or `podman compose ...`)."
            )
        else:
            live_session = Session(bridge=bridge, dialect="cypher")
            print("Connected to live Neo4j database on localhost:7687!")

            # 1. Clean up test nodes using OGM QueryBuilder
            u_clean = User("u_clean")
            q_cleanup = Query.match(u_clean).detach_delete(u_clean)
            live_session.execute(q_cleanup)

            # 2. Seed test graph data using OGM models & QueryBuilder fluent mutations
            u1 = User(alias="u1", user_id="u1", name="Alice", active=True, tier="enterprise")
            u2 = User(alias="u2", user_id="u2", name="Bob", active=True, tier="enterprise")
            r1 = FriendsWith(alias="r1", since=2024)
            q_seed = Query.create(u1).to(r1).node(u2)
            live_session.execute(q_seed)

            # 3. Execute live EXPLAIN
            print("Executing live EXPLAIN query...")
            exp_res = explain(base_query).execute(live_session)
            print(f"-> Live EXPLAIN executed cleanly! (Data rows returned: {len(exp_res.all())})")
            assert len(exp_res.all()) == 0

            # 4. Execute live PROFILE
            print("Executing live PROFILE query...")
            prof_res = profile(base_query).execute(live_session)
            prof_rows = prof_res.all()
            print(f"-> Live PROFILE executed cleanly! (Data rows returned: {len(prof_rows)})")
            assert len(prof_rows) >= 1
            print(f"-> Profiled Result Row: {prof_rows[0]}")

            # 5. Clean up using QueryBuilder
            live_session.execute(q_cleanup)
            live_session.close()
            print("Live Neo4j execution validated successfully!")
    except Exception as e:
        print(f"[SKIP] Live Neo4j engine check skipped: {e}")

    print("\nAll explain & profile examples executed successfully!")


if __name__ == "__main__":
    main()
