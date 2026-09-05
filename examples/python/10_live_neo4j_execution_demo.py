"""Voyager OGM: Live Neo4j Database Execution & Path Visualizer (Podman).

Connects to the live Neo4j database running in Podman, seeds real multi-hop graph data,
executes the query using `session.execute(fraud_query)`, accesses rows via
SQLAlchemy-style `mappings()` / `scalars()` / `to_polars()`, and visualizes the
reconstructed live data path graph.
"""

from __future__ import annotations

import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

from neo4j import GraphDatabase
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
    node,
    relationship,
)

# ---------------------------------------------------------------------------
# 1. Connect to Live Neo4j via Podman (Port 7687)
# ---------------------------------------------------------------------------

driver = GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "voyagerpass123"))
driver.verify_connectivity()
print(" Connected to Live Neo4j in Podman container `voyager-neo4j`!")

session = Session(bridge=driver, dialect="cypher")

# ---------------------------------------------------------------------------
# 2. Seed Real Multi-Hop Graph Data in Live Database
# ---------------------------------------------------------------------------

session.execute("MATCH (n) DETACH DELETE n")

session.execute("""
CREATE (c1:Customer {cust_id: 'Cust_Alice_101', tier: 'Gold'})
CREATE (c2:Customer {cust_id: 'Cust_Bob_102', tier: 'Platinum'})
CREATE (c3:Customer {cust_id: 'Cust_Charlie_103', tier: 'Silver'})

CREATE (a1:Account {acc_num: 'ACC_DE_Berlin_01', currency: 'EUR'})
CREATE (a2:Account {acc_num: 'ACC_CH_Zurich_99', currency: 'CHF'})
CREATE (a3:Account {acc_num: 'ACC_DE_Munich_02', currency: 'EUR'})
CREATE (a4:Account {acc_num: 'ACC_FR_Paris_03', currency: 'EUR'})
CREATE (a5:Account {acc_num: 'ACC_LU_Lux_88', currency: 'EUR'})

CREATE (m1:Merchant {merchant_id: 'M_01', name: 'LuxuryGoods_AG', country: 'CH'})
CREATE (m2:Merchant {merchant_id: 'M_02', name: 'GlobalRealEstate_SA', country: 'LU'})

CREATE (c1)-[:OWNS {since: '2022-01-01'}]->(a1)
CREATE (c2)-[:OWNS {since: '2023-05-15'}]->(a3)
CREATE (c3)-[:OWNS {since: '2021-11-20'}]->(a4)

CREATE (a1)-[:TRANSFERRED_TO {amount: 45000.0, timestamp: '2024-03-01'}]->(a2)
CREATE (a3)-[:TRANSFERRED_TO {amount: 85000.0, timestamp: '2024-03-02'}]->(a2)
CREATE (a4)-[:TRANSFERRED_TO {amount: 120000.0, timestamp: '2024-03-03'}]->(a5)

CREATE (a2)-[:PAID {amount: 130000.0}]->(m1)
CREATE (a5)-[:PAID {amount: 120000.0}]->(m2)
""")

print(" Seeded multi-hop financial transactions into live Neo4j database.")

# ---------------------------------------------------------------------------
# 3. Define Voyager OGM Domain Models
# ---------------------------------------------------------------------------


@node(label="Customer")
class Customer(Node):
    cust_id: str = Field(primary_key=True)


@node(label="Account")
class Account(Node):
    acc_num: str = Field(primary_key=True)


@node(label="Merchant")
class Merchant(Node):
    merchant_id: str = Field(primary_key=True)
    name: str = Field(default="")


@relationship(type_name="OWNS")
class Owns(Relationship):
    pass


@relationship(type_name="TRANSFERRED_TO")
class TransferredTo(Relationship):
    amount: float = Field(default=0.0)


@relationship(type_name="PAID")
class Paid(Relationship):
    pass


c = Customer(alias="c")
o = Owns(alias="o")
a1 = Account(alias="a1")
tx = TransferredTo(alias="tx")
a2 = Account(alias="a2")
p = Paid(alias="p")
m = Merchant(alias="m")

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

# ---------------------------------------------------------------------------
# 4. Canonical Session Execution: `session.execute(fraud_query)`
# ---------------------------------------------------------------------------

print("\n" + "=" * 75)
print("Executing Query on Live Database via `session.execute(fraud_query)`:")
print("=" * 75)

result = session.execute(fraud_query)

print(f"Total Live Rows Returned: {len(result)}")

# A. Dictionary Mappings View (.mappings().all())
print("\n--- A. Result Mappings (.mappings().all()) ---")
for idx, row in enumerate(result.mappings().all(), 1):
    print(f"Row {idx}: {row}")

# B. Flat Primary Column Scalars (.scalars().all())
print("\n--- B. Primary Column Scalars (.scalars().all()) ---")
print(f"Customers: {result.scalars().all()}")

# C. Columnar Polars DataFrame (.to_polars())
print("\n--- C. Polars DataFrame (.to_polars()) ---")
print(result.to_polars())

# ---------------------------------------------------------------------------
# 5. Live Graph Path Topology Reconstruction
# ---------------------------------------------------------------------------

print("\n" + "=" * 75)
print("Reconstructed Live Data Graph & Paths from Database Records:")
print("=" * 75)

print(f"Total Unique Graph Nodes ({len(result.nodes)}):")
for n in result.nodes:
    print(f"  - Node [{n['group']}]: {n['id']}")

print(f"\nTotal Reconstructed Path Edges ({len(result.edges)}):")
for e in result.edges:
    print(f"  - ({e['source']}) -[:{e['label']}]-> ({e['target']})")

# ---------------------------------------------------------------------------
# 6. Interactive GraphViewer Widget
# ---------------------------------------------------------------------------

viewer = result.show(theme="light")
print("\n" + "=" * 75)
print("Interactive Notebook Widget Status:")
print("=" * 75)
print(f"Viewer Mode:         {viewer.default_view}")
print(f"Viewer Theme:        {viewer.theme}")
print(
    f"Viewer Summary:      {len(viewer.nodes)} nodes, {len(viewer.edges)} edges, {len(viewer.records)} records"
)

driver.close()
