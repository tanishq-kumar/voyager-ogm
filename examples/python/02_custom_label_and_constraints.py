"""Approach 2: Custom Graph Labels & Field Constraints (Indexes / Uniqueness).

Run with: `uv run python examples/python/02_custom_label_and_constraints.py`
"""

from __future__ import annotations

from voyager_ogm import Field, Query, node, relationship


@node(label="Customer")
class User:
    username: str = Field(unique=True, index=True)
    email: str = Field(unique=True)
    age: int = Field(default=18)
    city: str = Field(default="London", index=True)


@node(label="Item")
class Product:
    sku: str = Field(unique=True)
    price: float
    category: str


@relationship(type_name="PURCHASED", direction="outgoing")
class Purchased:
    order_id: str
    quantity: int = 1


def main() -> None:
    print("=== Voyager OGM (Python) - Approach 2: Custom Labels & Constraints ===\n")

    user = User("u")
    prod = Product("p")
    purchased = Purchased("rel")

    query = (
        Query.match(user)
        .to(purchased)
        .node(prod)
        .where(
            user.city == "London",
            prod.price > 50.0,
        )
        .return_(user.username, prod.sku, quantity=purchased.quantity)
        .limit(20)
    )

    # Compile to SQL:2023 PGQ
    compiled = query.compile("sql_pgq", graph_name="ecommerce_graph")
    print(f"Generated SQL:2023 PGQ (GRAPH_TABLE):\n  {compiled.statement}\n")
    print(f"Parameters:\n  {compiled.parameters}\n")


if __name__ == "__main__":
    main()
