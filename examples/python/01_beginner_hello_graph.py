"""Level 1: Beginner - "Hello Graph!"

The absolute simplest way to define a graph node and run your first query.
No complex decorators or database jargon required.

Run with: `uv run python examples/python/01_beginner_hello_graph.py`
"""

from __future__ import annotations

from voyager_ogm import Query, node


# Step 1: Define your graph node using standard Python type annotations.
# Voyager automatically generates property descriptors and constructor auto-aliasing.
@node
class Person:
    name: str
    age: int
    city: str = "New York"


def main() -> None:
    print("=" * 65)
    print("[Level 1: Beginner] Hello Graph!")
    print("=" * 65)

    # Step 2: Create a node instance.
    # It automatically gets a conflict-free query alias: `_person_0`.
    p = Person()
    print(f"1. Created Person node with automatic alias: '{p.alias}'\n")

    # Step 3: Write a natural, readable query.
    # You can use standard Python operators like >, <, ==, and .contains()!
    query = Query.match(p).where(p.age >= 21, p.city == "London").return_(p.name, p.age).limit(5)

    # Step 4: Compile to standard openCypher for Neo4j / Memgraph.
    compiled = query.compile("cypher")

    print("2. Generated Parameterized Cypher Query:")
    print(f"   Statement:  {compiled.statement}")
    print(f"   Parameters: {compiled.parameters}\n")
    print("   Graph Pattern Explored:")
    print("   (:Person {city: 'London', age >= 21}) -> returns name, age")
    print("=" * 65)


if __name__ == "__main__":
    main()
