"""Voyager OGM: Compile-Time Constant Folding & AST Query Optimizer.

Demonstrates:
1. Compile-time arithmetic folding across nested sub-expressions in Rust.
2. Associative reassociation: ((u.age + 10) + 20) -> (u.age + 30).
3. Predicate pushdown hoisting enabled by constant folding: u.age == (10 + 15) -> {age: 25}.
4. Multi-dialect emission across openCypher, ISO/IEC GQL, and SQL:2023 PGQ.
"""

from __future__ import annotations

import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

from voyager_ogm import Field, Node, Query, Relationship


class User(Node):
    __label__ = "User"
    user_id: str = Field(primary_key=True)
    name: str
    age: int
    base_quota: int


class Event(Node):
    __label__ = "Event"
    event_id: str = Field(primary_key=True)
    duration_sec: int


class Performed(Relationship):
    __type__ = "PERFORMED"


def main():
    print("=== Voyager OGM: Compile-Time Constant Folding & Query Optimizer ===\n")

    u = User("u")
    e = Event("e")
    p = Performed("p")

    # -------------------------------------------------------------------------
    # 1. Complex Arithmetic Subtree Folding (Retention Window Formula)
    # -------------------------------------------------------------------------
    # Formula: 7 days * 24 hrs * 60 min * 60 sec = 604,800 seconds
    q1 = (
        Query.match(u)
        .to(p)
        .node(e)
        .where(
            u.name == "Alice",
            e.duration_sec >= u.base_quota + (7 * 24 * 60 * 60),
        )
        .return_(u.name, e.duration_sec)
    )

    unopt1 = q1.compile("cypher", optimize=False)
    opt1 = q1.compile("cypher", optimize=True)

    print("[1] 7-Day Retention Window Arithmetic Folding:")
    print(f"  Unoptimized Cypher :\n    {unopt1.statement}")
    print(f"  Optimized Cypher   :\n    {opt1.statement}")
    print(f"  Optimized Params   : {opt1.parameters}")
    print("  -> (7 * 24 * 60 * 60) was pre-computed into 604,800 at compile time.\n")

    # -------------------------------------------------------------------------
    # 2. Associative Arithmetic Reassociation
    # -------------------------------------------------------------------------
    # Reassociates ((u.age + 10) + 20) into (u.age + 30)
    q2 = Query.match(u).where(((u.age + 10) + 20) > 50).return_(u.name)
    unopt2 = q2.compile("cypher", optimize=False)
    opt2 = q2.compile("cypher", optimize=True)

    print("[2] Associative Arithmetic Reassociation:")
    print(f"  Unoptimized Cypher :\n    {unopt2.statement}")
    print(f"  Optimized Cypher   :\n    {opt2.statement}")
    print(f"  Unoptimized Params : {unopt2.parameters}")
    print(f"  Optimized Params   : {opt2.parameters}")
    print("  -> Pruned 1 binary AST node and reduced parameter count from 3 to 2.\n")

    # -------------------------------------------------------------------------
    # 3. Predicate Pushdown Enabled via Constant Folding
    # -------------------------------------------------------------------------
    # In openCypher/GQL, inline patterns {age: $p0} cannot contain binary operators.
    # Constant folding reduces (10 + 15) to 25, enabling Pass 2 to hoist the property!
    q3 = Query.match(u).where(u.age == (10 + 15)).return_(u.name)
    unopt3 = q3.compile("cypher", optimize=False)
    opt3 = q3.compile("cypher", optimize=True)

    print("[3] Index Seek Predicate Pushdown Hoisting:")
    print(f"  Unoptimized Cypher :\n    {unopt3.statement}")
    print(f"  Optimized Cypher   :\n    {opt3.statement}")
    print("  -> Hoisted {age: $p0} directly into node pattern for O(1) B-tree/hash index seek.\n")

    # -------------------------------------------------------------------------
    # 4. Multi-Dialect Optimized Emission
    # -------------------------------------------------------------------------
    opt_gql = q1.compile("iso_gql", optimize=True)
    opt_pgq = q1.compile("sql_pgq", optimize=True)

    print("[4] Cross-Dialect Compilation:")
    print(f"  ISO/IEC 39075:2024 GQL :\n    {opt_gql.statement}")
    print(f"  SQL:2023 PGQ           :\n    {opt_pgq.statement}")


if __name__ == "__main__":
    main()
