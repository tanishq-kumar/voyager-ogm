"""Voyager OGM: Direct Rust Native Query Builder.

Teaches:
    - Low-level Rust AST arena access via `_voyager_rs.NativeQueryBuilder`.
    - Escape hatch bypassing Python allocations for extreme compilation throughput.
    - Low-level builder primitives (`match`, `node`, `to`, `hops`, `where_gte`, `compile`).

Prerequisites:
    - Engine: None (offline / in-memory AST compilation)
    - Extras: None

Run with:
    `uv run python examples/python/29_native_rust_builder.py`
"""

from __future__ import annotations

from voyager_ogm import NativeQueryBuilder


def main() -> None:
    print("=== Voyager OGM (Python) - Approach 5: Direct Native Rust Builder ===\n")

    builder = NativeQueryBuilder()
    builder.match()
    builder.node("p", ["Person"])
    builder.to(["FOLLOWS"], "r")
    builder.hops(1, 3)
    builder.node("friend", ["Person"])
    builder.where_gte("p", "age", 21)
    builder.where_contains("friend", "name", "Smith")
    builder.return_()
    builder.field("p", "name", "start_user")
    builder.field("friend", "name", "connected_friend")
    builder.order_by("p", "name", True)
    builder.limit(25)

    res = builder.compile("cypher")
    print("Generated Native Cypher:")
    print(f"  Statement:  {res['statement']}")
    print(f"  Parameters: {res['parameters']}")


if __name__ == "__main__":
    main()
