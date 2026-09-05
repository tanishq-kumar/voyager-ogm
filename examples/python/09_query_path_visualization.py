"""Voyager OGM: Live Graph Path Execution & Visualization (SQLAlchemy + Neo4j Browser Style).

Demonstrates how Voyager OGM empowers data scientists and backend engineers to:
1. Define expressive graph domain models and author complex multi-hop path queries.
2. Execute queries against a database session with `query.execute(session)` or `session.execute(query)`.
3. Access query results with SQLAlchemy-grade patterns:
   - `result.mappings().all()` -> List of dictionary mappings.
   - `result.scalars().all()`  -> Flat scalar values.
   - `result.to_polars()`      -> Polars DataFrame.
   - `result.to_arrow()`       -> Zero-copy Apache Arrow Table.
4. Auto-reconstruct live data nodes, relationships, and multi-hop paths from result records.
5. Render an interactive Neo4j Browser / Bloom-style GraphViewer widget (`result.show()`)
   directly inside Marimo, Jupyter, and VS Code notebooks.
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
    node,
    relationship,
)

# ---------------------------------------------------------------------------
# 1. Define Domain Graph Models
# ---------------------------------------------------------------------------


@node(label="Customer")
class Customer(Node):
    cust_id: str = Field(primary_key=True)
    tier: str = Field(default="Gold")
    credit_limit: float = Field(default=50000.0)


@node(label="Account")
class Account(Node):
    acc_num: str = Field(primary_key=True)
    currency: str = Field(default="EUR")
    balance: float = Field(default=0.0)


@node(label="Merchant")
class Merchant(Node):
    merchant_id: str = Field(primary_key=True)
    name: str = Field(default="")
    country: str = Field(default="DE")


@relationship(type_name="OWNS")
class Owns(Relationship):
    since: str = Field(default="2024-01-01")


@relationship(type_name="TRANSFERRED_TO")
class TransferredTo(Relationship):
    amount: float = Field(default=0.0)
    timestamp: str = Field(default="")


@relationship(type_name="PAID")
class Paid(Relationship):
    amount: float = Field(default=0.0)


# ---------------------------------------------------------------------------
# 2. Author Complex Multi-Hop Graph Path Query
# ---------------------------------------------------------------------------

c = Customer(alias="c")
o = Owns(alias="o")
a1 = Account(alias="a1")
tx = TransferredTo(alias="tx")
a2 = Account(alias="a2")
p = Paid(alias="p")
m = Merchant(alias="m")

# Fraud Ring & Cross-Border Payment Investigation Path:
# (c:Customer)-[o:OWNS]->(a1:Account)-[tx:TRANSFERRED_TO]->(a2:Account)-[p:PAID]->(m:Merchant)
fraud_query = (
    Query.match(c)
    .to(o)
    .node(a1)
    .to(tx)
    .node(a2)
    .to(p)
    .node(m)
    .where(tx.amount > 10000.0)
    .return_(
        customer="c.cust_id",
        source_account="a1.acc_num",
        destination_account="a2.acc_num",
        merchant="m.name",
        transfer_amount="tx.amount",
    )
    .limit(50)
)

print("=" * 75)
print("1. Multi-Dialect Compiled Graph Statements:")
print("=" * 75)
print("[openCypher]:")
print(fraud_query.compile("cypher").statement)
print("\n[ISO GQL]:")
print(fraud_query.compile("iso_gql").statement)
print("\n[SQL:2023 PGQ]:")
print(fraud_query.compile("sql_pgq", graph_name="banking_graph").statement)

# ---------------------------------------------------------------------------
# 3. Setup Live Database Session & Queue Results
# ---------------------------------------------------------------------------

# Setup session with simulated graph database records (e.g. from Neo4j / DuckDB / Memgraph)
bridge = MockBridge()
bridge.queue_result(
    [
        {
            "customer": "Cust_Alice_101",
            "source_account": "ACC_DE_Berlin_01",
            "destination_account": "ACC_CH_Zurich_99",
            "merchant": "LuxuryGoods_AG",
            "transfer_amount": 45000.0,
        },
        {
            "customer": "Cust_Bob_102",
            "source_account": "ACC_DE_Munich_02",
            "destination_account": "ACC_CH_Zurich_99",
            "merchant": "LuxuryGoods_AG",
            "transfer_amount": 85000.0,
        },
        {
            "customer": "Cust_Charlie_103",
            "source_account": "ACC_FR_Paris_03",
            "destination_account": "ACC_LU_Lux_88",
            "merchant": "GlobalRealEstate_SA",
            "transfer_amount": 120000.0,
        },
    ]
)
session = Session(bridge=bridge, dialect="cypher")

# ---------------------------------------------------------------------------
# 4. Execute Query & SQLAlchemy-Style Data Extraction
# ---------------------------------------------------------------------------

print("\n" + "=" * 75)
print("2. SQLAlchemy-Grade Result Access Patterns:")
print("=" * 75)

# Both `fraud_query.execute(session)` and `session.execute(fraud_query)` return ExecutionResult!
result = fraud_query.execute(session)

print(f"Total Rows Executed: {len(result)}")

# A. Dictionary Mappings View (.mappings().all())
print("\n--- A. Dictionary Mappings (.mappings().all()) ---")
mappings = result.mappings().all()
for idx, row in enumerate(mappings, 1):
    print(f"Row {idx}: {row}")

# B. Flat Scalars View (.scalars().all())
print("\n--- B. Primary Column Scalars (.scalars().all()) ---")
scalars = result.scalars().all()
print(f"Customers: {scalars}")

# C. Columnar Polars DataFrame (.to_polars())
print("\n--- C. Polars DataFrame (.to_polars()) ---")
df = result.to_polars()
print(df)

# ---------------------------------------------------------------------------
# 5. Neo4j-Style Live Graph Entity & Path Reconstruction
# ---------------------------------------------------------------------------

print("\n" + "=" * 75)
print("3. Live Graph Entities & Multi-Hop Path Reconstruction:")
print("=" * 75)

# `result.nodes` and `result.edges` automatically reconstruct the live graph topology from rows!
print(f"Reconstructed Live Nodes ({len(result.nodes)}):")
for n in result.nodes:
    print(f"  - Node [{n['group']}]: {n['id']}")

print(f"\nReconstructed Live Relationships/Paths ({len(result.edges)}):")
for e in result.edges:
    props = f" {e['data']}" if e.get("data") else ""
    print(f"  - ({e['source']}) -[:{e['label']}{props}]-> ({e['target']})")

# ---------------------------------------------------------------------------
# 6. Interactive GraphViewer Widget (Graph View + Table View + Query View)
# ---------------------------------------------------------------------------

print("\n" + "=" * 75)
print("4. Interactive Notebook Visualization (Marimo / Jupyter / VS Code):")
print("=" * 75)

# `result.show()` launches the interactive GraphViewer with:
# - Graph Tab: Live multi-hop paths & nodes with physics and degree scaling.
# - Table Tab: Tabular dataframe with tri-state sorting, filtering, and CSV export.
# - Query Tab: Multi-dialect openCypher, ISO GQL, and SQL:2023 PGQ statements.
viewer = result.show(theme="light", height="600px")
print(f"Viewer Default View: {viewer.default_view}")
print(f"Viewer Theme:        {viewer.theme}")
print(
    f"Viewer Summary:      {len(viewer.nodes)} nodes, {len(viewer.edges)} edges, {len(viewer.records)} rows"
)
print("\nHTML Fallback Output Preview:")
print(viewer._repr_html_())
