"""Approach 5: Direct Rust Native Query Builder (`_voyager_rs.NativeQueryBuilder`).

Run with: `uv run python examples/python/05_native_rust_builder.py`
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
