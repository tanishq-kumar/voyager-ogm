import marimo

__generated_with = "0.24.0"
app = marimo.App(width="full")


@app.cell
def _():
    import sys

    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    import marimo as mo
    import polars as pl

    # 1. Dataset A: AI & GraphRAG Pipeline (Dictionary Records)
    graphrag_records = [
        {
            "source": "Transformer",
            "target": "Attention_Is_All_You_Need",
            "rel": "INTRODUCED_IN",
            "category": "Architecture",
        },
        {
            "source": "BERT",
            "target": "Attention_Is_All_You_Need",
            "rel": "DERIVED_FROM",
            "category": "Model",
        },
        {"source": "GPT-4", "target": "Transformer", "rel": "BASED_ON", "category": "LLM"},
        {"source": "Claude_3", "target": "Transformer", "rel": "BASED_ON", "category": "LLM"},
        {
            "source": "Voyager_OGM",
            "target": "GraphRAG",
            "rel": "POWERS",
            "category": "Database_OGM",
        },
        {"source": "GraphRAG", "target": "GPT-4", "rel": "USES", "category": "Pipeline"},
        {"source": "GraphRAG", "target": "Claude_3", "rel": "USES", "category": "Pipeline"},
        {
            "source": "Vector_DB",
            "target": "GraphRAG",
            "rel": "INDEXED_BY",
            "category": "Storage",
        },
        {
            "source": "Voyager_OGM",
            "target": "Vector_DB",
            "rel": "BRIDGES_WITH",
            "category": "Integration",
        },
    ]

    # 2. Dataset B: Microservices & Cloud Infrastructure (CSV -> Polars DataFrame)
    infra_csv = """source,target,rel,bandwidth_gbps,environment
    WebServer_01,Auth_Gateway,CALLS,10,Production
    WebServer_02,Auth_Gateway,CALLS,10,Production
    Auth_Gateway,User_DB_Primary,QUERIES,40,Production
    User_DB_Primary,User_DB_Replica,REPLICATES_TO,100,Production
    Payment_Service,User_DB_Primary,QUERIES,40,Production
    Payment_Service,Fraud_Engine,EVALUATES,25,Production
    Fraud_Engine,Redis_Cache,READS,80,Production
    Analytics_Worker,Redis_Cache,CONSUMES,15,Staging
    """
    infra_df = pl.read_csv(infra_csv.encode("utf-8"))

    # 3. Dataset C: Enterprise Org Hierarchy (Dictionary Records)
    org_records = [
        {
            "source": "Alice (VP)",
            "target": "Bob (Tech Lead)",
            "rel": "MANAGES",
            "dept": "Engineering",
        },
        {
            "source": "Alice (VP)",
            "target": "Charlie (Staff Eng)",
            "rel": "MANAGES",
            "dept": "Engineering",
        },
        {
            "source": "Bob (Tech Lead)",
            "target": "Project_Voyager",
            "rel": "WORKS_ON",
            "dept": "Core",
        },
        {
            "source": "Charlie (Staff Eng)",
            "target": "Project_Voyager",
            "rel": "WORKS_ON",
            "dept": "Core",
        },
        {
            "source": "Charlie (Staff Eng)",
            "target": "Project_Titan",
            "rel": "WORKS_ON",
            "dept": "Infra",
        },
        {
            "source": "Diana (Director)",
            "target": "Alice (VP)",
            "rel": "MANAGES",
            "dept": "Executive",
        },
    ]

    # Reactive Query Dataset Selector Dropdown
    dataset_dropdown = mo.ui.dropdown(
        options={
            "AI & GraphRAG Pipeline": pl.DataFrame(graphrag_records),
            "Microservices & Cloud Infrastructure": infra_df,
            "Enterprise Org Hierarchy": pl.DataFrame(org_records),
        },
        value="AI & GraphRAG Pipeline",
        label="⚡ Select Active Graph Dataset:",
    )
    return dataset_dropdown, mo, pl


@app.cell
def _(dataset_dropdown, mo):
    # UI Selector Control Header
    mo.vstack(
        [
            mo.md("# Voyager OGM: Reactive Graph & Records Explorer"),
            mo.md(
                "Switch between multiple datasets (CSV, DataFrames, Dictionaries) and inspect graph structures interactively."
            ),
            dataset_dropdown,
        ]
    )
    return


@app.cell
def _(dataset_dropdown):
    from voyager_ogm import GraphViewer

    # Active Dataset -> Interactive GraphViewer
    active_df = dataset_dropdown.value
    viewer = GraphViewer.from_polars(
        active_df,
        source_col="source",
        target_col="target",
        edge_label_col="rel",
        height="520px",
    )
    viewer
    return active_df, viewer


@app.cell
def _(active_df, mo, pl, viewer):
    # Downstream Reactive Cell: Reacts to node selection in the WebGL visualizer
    selected = viewer.selected_node

    if selected:
        subgraph = active_df.filter((pl.col("source") == selected) | (pl.col("target") == selected))
        result = mo.vstack(
            [
                mo.md(f"### 🔍 Neighborhood Details for Selected Node: **`{selected}`**"),
                mo.ui.table(subgraph),
            ]
        )
    else:
        result = mo.md(
            "*Click any node on the graph canvas above to inspect its 1-hop neighborhood and properties in real time.*"
        )
    result
    return


@app.cell
def _():
    from voyager_ogm import (
        Field,
        Node,
        Query,
        Relationship,
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
    # 2. Author Complex Multi-Hop Graph Path Queries
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
    return (fraud_query,)


@app.cell
def _(fraud_query):
    fraud_query.show(height="520px")
    return


@app.cell
def _():
    return


if __name__ == "__main__":
    app.run()
