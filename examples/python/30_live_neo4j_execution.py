"""Voyager OGM: Live Neo4j Database Execution & Path Visualizer.

Teaches:
    - Connecting to a live Neo4j database instance over Bolt protocol
    - Defining declarative domain models (`Customer`, `Account`, `Merchant`, `Owns`, `TransferredTo`, `Paid`)
    - Schema constraint application via `session.create_constraints()`
    - Handling constraints that require Neo4j Enterprise Edition on Community
    - High-throughput OGM bulk seeding (`session.bulk_create`, `session.bulk_create_relationships`)
    - Executing complex multi-hop queries with `session.execute(fraud_query)`
    - Accessing query rows via `mappings()`, `scalars()`, and `to_polars()`
    - Reconstructing entity graph topology directly from query execution results
    - Visualizing result subgraphs with `result.show()`
    - Idempotent cleanup via OGM `detach_delete` queries

Prerequisites:
    - Engine: Neo4j running on bolt://127.0.0.1:7687 via your Compose runner:
      `docker compose -f containers/compose.yaml up -d neo4j`
      (or `podman compose -f containers/compose.yaml up -d neo4j`)
    - Extras: `neo4j` extra (`pip install "voyager-ogm[neo4j]"`)

Run with:
    `uv run python examples/python/30_live_neo4j_execution.py`
"""

from __future__ import annotations

import socket
import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
    VoyagerError,
    create_bridge,
    node,
    relationship,
)


def is_port_open(host: str, port: int, timeout: float = 0.5) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


# ---------------------------------------------------------------------------
# 1. Define OGM Entity Models
# ---------------------------------------------------------------------------


@node(label="Customer")
class Customer(Node):
    cust_id: str = Field(primary_key=True)
    tier: str = Field()


@node(label="Account")
class Account(Node):
    acc_num: str = Field(primary_key=True)
    currency: str = Field()


@node(label="Merchant")
class Merchant(Node):
    merchant_id: str = Field(primary_key=True)
    name: str = Field()
    country: str = Field()


@relationship(type_name="OWNS")
class Owns(Relationship):
    since: str = Field()


@relationship(type_name="TRANSFERRED_TO")
class TransferredTo(Relationship):
    amount: float = Field()
    timestamp: str = Field()


@relationship(type_name="PAID")
class Paid(Relationship):
    amount: float = Field()
    category: str = Field()


def main() -> None:
    print("=== Voyager OGM: Live Neo4j Database Execution ===\n")

    # ---------------------------------------------------------------------------
    # 2. Connect to Live Neo4j (Port 7687)
    # ---------------------------------------------------------------------------
    uri = "bolt://127.0.0.1:7687"
    if not is_port_open("127.0.0.1", 7687):
        print(
            f"Neo4j engine is not reachable on {uri} (start with `docker compose -f containers/compose.yaml up -d neo4j` or `podman compose ...`)."
        )
        sys.exit(0)

    try:
        from neo4j import GraphDatabase

        driver = GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "voyagerpass123"))
        bridge = create_bridge(driver)
        if not bridge.ping():
            raise ConnectionError(f"Failed to ping {uri}")
        print("Connected to Live Neo4j instance on port 7687!")
    except Exception as err:
        print(f"Neo4j engine is not reachable on {uri}: {err}")
        sys.exit(0)

    session = Session(bridge=bridge, dialect="cypher")

    # ---------------------------------------------------------------------------
    # 3. Clean & Apply Schema Constraints via OGM
    # ---------------------------------------------------------------------------
    q_clean = Query.match("n").detach_delete("n")
    session.execute(q_clean)

    # Uniqueness constraints are applied first. Neo4j Community rejects existence
    # (IS NOT NULL) and key constraints because they require Enterprise Edition, so
    # create_constraints() reports them in one SCHEMA_ERROR and continues. Other
    # errors still propagate.
    try:
        session.create_constraints(Customer, Account, Merchant)
        print("Applied all schema constraints.")
    except VoyagerError as err:
        if err.code != "SCHEMA_ERROR":
            raise
        skipped = getattr(err, "skipped", [])
        print(
            "Neo4j Community applied the UNIQUE constraints but skipped existence/key "
            "constraints, which require Neo4j Enterprise Edition:"
        )
        for statement in skipped:
            print(f"  - {statement}")
        print("Continuing with the constraints the server accepted.\n")

    # ---------------------------------------------------------------------------
    # 4. Seed Real Multi-Hop Graph Data via OGM Bulk Creation
    # ---------------------------------------------------------------------------
    customer_data = [
        {"cust_id": "Cust_Alice_101", "tier": "Gold"},
        {"cust_id": "Cust_Bob_102", "tier": "Platinum"},
        {"cust_id": "Cust_Charlie_103", "tier": "Silver"},
    ]
    account_data = [
        {"acc_num": "ACC_DE_Berlin_01", "currency": "EUR"},
        {"acc_num": "ACC_CH_Zurich_99", "currency": "CHF"},
        {"acc_num": "ACC_DE_Munich_02", "currency": "EUR"},
        {"acc_num": "ACC_FR_Paris_03", "currency": "EUR"},
        {"acc_num": "ACC_LU_Lux_88", "currency": "EUR"},
    ]
    merchant_data = [
        {"merchant_id": "M_01", "name": "LuxuryGoods_AG", "country": "CH"},
        {"merchant_id": "M_02", "name": "GlobalRealEstate_SA", "country": "LU"},
    ]

    session.run_bulk(session.bulk_create(Customer, customer_data))
    session.run_bulk(session.bulk_create(Account, account_data))
    session.run_bulk(session.bulk_create(Merchant, merchant_data))

    owns_data = [
        {"from_cust_id": "Cust_Alice_101", "to_acc_num": "ACC_DE_Berlin_01", "since": "2022-01-01"},
        {"from_cust_id": "Cust_Bob_102", "to_acc_num": "ACC_DE_Munich_02", "since": "2023-05-15"},
        {
            "from_cust_id": "Cust_Charlie_103",
            "to_acc_num": "ACC_FR_Paris_03",
            "since": "2021-11-20",
        },
    ]
    session.run_bulk(
        session.bulk_create_relationships(
            rel_model=Owns,
            data=owns_data,
            from_label="Customer",
            from_key="cust_id",
            to_label="Account",
            to_key="acc_num",
            properties=["since"],
        )
    )

    transfer_data = [
        {
            "from_acc_num": "ACC_DE_Berlin_01",
            "to_acc_num": "ACC_CH_Zurich_99",
            "amount": 250000.0,
            "timestamp": "2024-03-01T10:15:00Z",
        },
        {
            "from_acc_num": "ACC_DE_Munich_02",
            "to_acc_num": "ACC_CH_Zurich_99",
            "amount": 15000.0,
            "timestamp": "2024-03-01T11:00:00Z",
        },
        {
            "from_acc_num": "ACC_FR_Paris_03",
            "to_acc_num": "ACC_LU_Lux_88",
            "amount": 450000.0,
            "timestamp": "2024-03-02T14:30:00Z",
        },
    ]
    session.run_bulk(
        session.bulk_create_relationships(
            rel_model=TransferredTo,
            data=transfer_data,
            from_label="Account",
            from_key="acc_num",
            to_label="Account",
            to_key="acc_num",
            properties=["amount", "timestamp"],
        )
    )

    paid_data = [
        {
            "from_acc_num": "ACC_CH_Zurich_99",
            "to_merchant_id": "M_01",
            "amount": 240000.0,
            "category": "Jewelry",
        },
        {
            "from_acc_num": "ACC_LU_Lux_88",
            "to_merchant_id": "M_02",
            "amount": 420000.0,
            "category": "CommercialProperty",
        },
    ]
    session.run_bulk(
        session.bulk_create_relationships(
            rel_model=Paid,
            data=paid_data,
            from_label="Account",
            from_key="acc_num",
            to_label="Merchant",
            to_key="merchant_id",
            properties=["amount", "category"],
        )
    )

    print(
        "Successfully seeded financial fraud investigation topology via OGM in live Neo4j database!"
    )

    # ---------------------------------------------------------------------------
    # 5. Construct Fluent Traversal Query & Execute on Neo4j
    # ---------------------------------------------------------------------------
    c = Customer(alias="c")
    o = Owns(alias="o")
    a1 = Account(alias="a1")
    tx = TransferredTo(alias="tx")
    a2 = Account(alias="a2")
    p = Paid(alias="p")
    m = Merchant(alias="m")

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
            customer=c.cust_id,
            source_account=a1.acc_num,
            destination_account=a2.acc_num,
            merchant=m.name,
            transfer_amount=tx.amount,
            merchant_amount=p.amount,
        )
        .order_by(tx.amount)
        .limit(50)
    )

    print("\n" + "=" * 75)
    print("Executing Compiled Cypher Query on Live Neo4j:")
    print("=" * 75)
    print(fraud_query.compile("cypher").statement)
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
    # 6. Live Graph Path Topology Reconstruction
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
    # 7. Interactive GraphViewer Widget
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

    # ---------------------------------------------------------------------------
    # 8. Clean up & Close
    # ---------------------------------------------------------------------------
    session.execute(q_clean)
    session.close()


if __name__ == "__main__":
    main()
