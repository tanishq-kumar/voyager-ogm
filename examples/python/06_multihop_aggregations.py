"""Voyager OGM: Variable-Length Paths, Aggregations & Pagination.

Teaches:
    - Variable-length path bounds (`.hops(1, 3)` / `*1..3`)
    - Deduplicating entity returns with `.distinct()`
    - Sorting with `.order_by_desc()`
    - Pagination with `.skip()` and `.limit()`
    - Query parameter extraction and statement inspection

Expected Output:
    MATCH (u_start:User)-[rel:FOLLOWS*1..3]->(u_influencer:User)
    WHERE ((u_start.username = $p0) AND (u_influencer.reputation >= $p1)) AND (u_influencer.country CONTAINS $p2)
    RETURN DISTINCT u_influencer.username, u_influencer.reputation, u_influencer.country
    ORDER BY u_influencer.reputation DESC SKIP 10 LIMIT 20

Prerequisites:
    - Engine: None (runs offline / compile-only; no database needed)
    - Extras: None

Run with:
    `uv run python examples/python/06_multihop_aggregations.py`
"""

from __future__ import annotations

from voyager_ogm import Field, Query, node, relationship


@node(label="User")
class User:
    username: str
    reputation: int
    country: str = Field(index=True)


@relationship(type_name="FOLLOWS")
class Follows:
    since: int = 2024


def main() -> None:
    print("=" * 65)
    print("[Level 3: Advanced] Variable Paths & Aggregations")
    print("=" * 65)

    # Scenario: Find all reachable influential friends between 1 and 3 hops away
    # Shape: (start_user:User)-[:FOLLOWS*1..3]->(influencer:User)
    start_user = User("u_start")
    influencer = User("u_influencer")
    follows = Follows("rel")

    query = (
        Query.match(start_user)
        .to(follows)
        .hops(1, 3)  # Variable-length path: -[:FOLLOWS*1..3]->
        .node(influencer)
        .where(
            start_user.username == "alice_dev",
            influencer.reputation >= 1000,
            influencer.country.contains("Germany"),
        )
        .return_(
            influencer.username,
            influencer.reputation,
            influencer.country,
            distinct=True,  # Deduplicate reachable nodes
        )
        .order_by_desc(influencer.reputation)
        .skip(10)  # Pagination offset
        .limit(20)  # Pagination page size
    )

    compiled = query.compile("cypher")

    print("\n1. Compiled Deep Variable-Length Graph Query:")
    print(f"   {compiled.statement}\n")
    print(f"2. Extracted Parameters: {compiled.parameters}\n")
    print("   Capabilities Demonstrated:")
    print("   - Variable-length expansion: *1..3 hops")
    print("   - Automatic DISTINCT deduplication")
    print("   - DESC sorting + SKIP / LIMIT pagination")
    print("=" * 65)


if __name__ == "__main__":
    main()
