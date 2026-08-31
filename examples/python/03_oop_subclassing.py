"""Approach 3: Pure OOP Subclassing (Without Decorators).

Run with: `uv run python examples/python/03_oop_subclassing.py`
"""

from __future__ import annotations

from voyager_ogm import Field, Node, Query, Relationship


# Pure inheritance from Node & Relationship base classes
class Person(Node):
    name: str = Field()
    age: int = Field()


class Movie(Node, label="Film"):
    title: str = Field()
    released: int = Field()


class ActedIn(Relationship, type_name="ACTED_IN"):
    role: str = Field()


def main() -> None:
    print("=== Voyager OGM (Python) - Approach 3: Pure OOP Subclassing ===\n")

    p = Person()
    m = Movie()
    acted = ActedIn()

    query = (
        Query.match(p)
        .to(acted)
        .hops(1, 2)
        .node(m)
        .where(
            p.age >= 21,
            m.released == 1999,
        )
        .return_(p.name, m.title, role=acted.role)
        .order_by(p.name)
        .limit(10)
    )

    compiled = query.compile("iso_gql")
    print(f"Generated ISO/IEC 39075:2024 GQL Statement:\n  {compiled.statement}\n")
    print(f"Parameters:\n  {compiled.parameters}\n")


if __name__ == "__main__":
    main()
