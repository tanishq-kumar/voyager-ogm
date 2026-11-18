"""Voyager OGM: Custom Graph Labels & Field Constraints.

Teaches:
    - Explicit database labels with `@node(label="Customer")`
    - Defining index and uniqueness constraints via `Field(unique=True, index=True)`
    - How constraints feed into DDL generation (see also `13_schema_ddl_and_indexes.py`)
    - Custom relationship type names with `@relationship(type_name="PURCHASED")`
    - Compiling queries to SQL:2023 PGQ (`GRAPH_TABLE`)

Prerequisites:
    - Engine: None (runs offline / compile-only)
    - Extras: None

Run with:
    `uv run python examples/python/03_custom_labels_and_constraints.py`
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
