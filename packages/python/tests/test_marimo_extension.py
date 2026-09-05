"""Unit tests for Task 4.2: Marimo Reactive Notebook Extension.

Verifies the GraphViewer component, Polars DataFrame ingestion, traitlet synchronization,
and interactive selection events.
"""

from __future__ import annotations

import polars as pl
from voyager_ogm import GraphViewer


class TestMarimoGraphViewer:
    def test_graph_viewer_init(self):
        """Verifies direct instantiation of GraphViewer."""
        nodes = [
            {"id": "1", "label": "Alice", "color": "#38bdf8", "size": 12},
            {"id": "2", "label": "Bob", "color": "#38bdf8", "size": 12},
        ]
        edges = [
            {"source": "1", "target": "2", "label": "KNOWS"},
        ]

        viewer = GraphViewer(nodes=nodes, edges=edges, height="600px", theme="dark")
        assert len(viewer.nodes) == 2
        assert len(viewer.edges) == 1
        assert viewer.height == "600px"
        assert viewer.theme == "dark"
        assert viewer.selected_node == ""

    def test_from_polars_dataframe(self):
        """Verifies GraphViewer.from_polars conversion from Polars DataFrame."""
        df = pl.DataFrame(
            {
                "source": ["u1", "u2", "u1"],
                "target": ["u2", "u3", "u4"],
                "relationship": ["FOLLOWS", "FOLLOWS", "MANAGES"],
                "source_name": ["Alice", "Bob", "Alice"],
            }
        )

        viewer = GraphViewer.from_polars(
            df,
            source_col="source",
            target_col="target",
            label_col="source_name",
            edge_label_col="relationship",
        )

        assert len(viewer.nodes) == 4  # u1, u2, u3, u4
        assert len(viewer.edges) == 3

        node_ids = {n["id"] for n in viewer.nodes}
        assert node_ids == {"u1", "u2", "u3", "u4"}

        # Edge verification
        assert viewer.edges[0]["source"] == "u1"
        assert viewer.edges[0]["target"] == "u2"
        assert viewer.edges[0]["label"] == "FOLLOWS"
        assert viewer.edges[2]["label"] == "MANAGES"

    def test_empty_dataframe(self):
        """Verifies graceful handling of empty DataFrame."""
        df = pl.DataFrame()
        viewer = GraphViewer.from_polars(df)
        assert len(viewer.nodes) == 0
        assert len(viewer.edges) == 0

    def test_node_selection_and_html_repr(self):
        """Verifies reactive selection update and HTML fallback."""
        nodes = [{"id": "node_42", "label": "Answer"}]
        edges = []

        viewer = GraphViewer(nodes=nodes, edges=edges)
        # Simulate reactive click in Marimo
        viewer.selected_node = "node_42"
        assert viewer.selected_node == "node_42"

        html = viewer._repr_html_()
        assert "<iframe srcdoc=" in html
        assert "Voyager Graph Studio" in html
        assert "allow-scripts" in html
        assert "node_42" in html

    def test_from_query_and_query_show(self):
        """Verifies GraphViewer.from_query and query.show() integration."""
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

        @node(label="Person")
        class Person(Node):
            name: str = Field(primary_key=True)

        @relationship(type_name="KNOWS")
        class Knows(Relationship):
            pass

        p1 = Person(alias="p1")
        k = Knows(alias="k")
        p2 = Person(alias="p2")

        query = Query.match(p1).to(k).node(p2).return_(source="p1.name", target="p2.name")

        # 1. Unconnected query view (auto-extracts path topology without live database execution)
        viewer = query.show()
        assert viewer.theme == "light"
        assert viewer.default_view == "graph"
        assert "MATCH (p1:Person)-[k:KNOWS]->(p2:Person)" in viewer.query_statement
        assert len(viewer.nodes) == 2
        assert len(viewer.edges) == 1
        assert viewer.nodes[0]["id"] == "p1"
        assert viewer.nodes[1]["id"] == "p2"
        assert viewer.edges[0]["label"] == "KNOWS"

        # 2. Query executed against session
        mock_bridge = MockBridge()
        mock_bridge.queue_result([{"source": "Alice", "target": "Bob"}])
        session = Session(bridge=mock_bridge, dialect="cypher")

        viewer_live = query.show(session=session)
        assert len(viewer_live.nodes) == 2
        assert len(viewer_live.edges) == 1
        assert len(viewer_live.records) == 1
        assert viewer_live.records[0]["source"] == "Alice"

    def test_session_explore(self):
        """Verifies session.explore() with DataFrame and raw queries."""
        from voyager_ogm import MockBridge, Session

        mock_bridge = MockBridge()
        mock_bridge.queue_result(
            [
                {"source": "Server1", "target": "DB_Master", "rel": "CONNECTS_TO"},
                {"source": "Server2", "target": "DB_Master", "rel": "CONNECTS_TO"},
            ]
        )
        session = Session(bridge=mock_bridge, dialect="cypher")

        viewer = session.explore("MATCH (s)-[r]->(d) RETURN s, r, d")
        assert len(viewer.nodes) == 3
        assert len(viewer.edges) == 2
        assert len(viewer.records) == 2

    def test_edge_properties_and_auto_view_detection(self):
        """Verifies edge property preservation and automatic tab selection."""
        # 1. Graph with edges -> auto-selects 'graph'
        df_graph = pl.DataFrame(
            {
                "source": ["Alice", "Bob"],
                "target": ["Bob", "Charlie"],
                "rel": ["MANAGES", "COLLABORATES"],
                "bandwidth_gbps": [10, 40],
                "since_year": [2020, 2023],
            }
        )
        viewer_g = GraphViewer.from_polars(df_graph)
        assert viewer_g.default_view == "graph"
        assert len(viewer_g.edges) == 2
        # Verify edge property extraction
        assert viewer_g.edges[0]["data"]["bandwidth_gbps"] == 10
        assert viewer_g.edges[0]["data"]["since_year"] == 2020

        # 2. Tabular scalar records without graph edges -> auto-selects 'table'
        df_table = pl.DataFrame(
            {
                "department": ["Engineering", "HR", "Finance"],
                "headcount": [120, 25, 45],
                "avg_salary": [140000, 95000, 115000],
            }
        )
        viewer_t = GraphViewer.from_polars(df_table)
        assert viewer_t.default_view == "table"
        assert len(viewer_t.nodes) == 0
        assert len(viewer_t.edges) == 0
        assert len(viewer_t.records) == 3

    def test_to_polars_and_column_types(self):
        """Verifies to_polars DataFrame export and column type capture."""
        df = pl.DataFrame(
            {
                "node_id": ["n1", "n2"],
                "score": [98.5, 42.0],
                "active": [True, False],
            }
        )
        viewer = GraphViewer.from_polars(df)
        assert viewer.column_types["node_id"] == "String"
        assert "Float" in viewer.column_types["score"]
        assert viewer.column_types["active"] == "Boolean"

        exported_df = viewer.to_polars()
        assert isinstance(exported_df, pl.DataFrame)
        assert len(exported_df) == 2
        assert exported_df.columns == ["node_id", "score", "active"]
        assert isinstance(viewer.dataframe, pl.DataFrame)

    def test_execution_result_sqlalchemy_access_and_live_path_graph(self):
        """Verifies ExecutionResult mappings, scalars, to_polars, live path reconstruction, and show()."""
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

        @node(label="Customer")
        class Customer(Node):
            cust_id: str = Field(primary_key=True)

        @node(label="Account")
        class Account(Node):
            acc_num: str = Field(primary_key=True)

        @node(label="Merchant")
        class Merchant(Node):
            merchant_id: str = Field(primary_key=True)

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
        )

        bridge = MockBridge()
        bridge.queue_result(
            [
                {
                    "customer": "Alice",
                    "source_account": "ACC_1",
                    "destination_account": "ACC_2",
                    "merchant": "Store_X",
                    "transfer_amount": 50000.0,
                },
                {
                    "customer": "Bob",
                    "source_account": "ACC_3",
                    "destination_account": "ACC_2",
                    "merchant": "Store_X",
                    "transfer_amount": 75000.0,
                },
            ]
        )
        session = Session(bridge=bridge, dialect="cypher")

        # 1. Execute query
        result = fraud_query.execute(session)

        # 2. SQLAlchemy-style data access
        assert len(result) == 2
        assert result.mappings().all() == [
            {
                "customer": "Alice",
                "source_account": "ACC_1",
                "destination_account": "ACC_2",
                "merchant": "Store_X",
                "transfer_amount": 50000.0,
            },
            {
                "customer": "Bob",
                "source_account": "ACC_3",
                "destination_account": "ACC_2",
                "merchant": "Store_X",
                "transfer_amount": 75000.0,
            },
        ]
        assert result.scalars().all() == ["Alice", "Bob"]
        assert result.first()["customer"] == "Alice"
        assert isinstance(result.to_polars(), pl.DataFrame)
        assert len(result.to_polars()) == 2

        # 3. Neo4j-style live graph path entity reconstruction
        nodes, edges = result.to_graph()
        assert len(nodes) == 6  # Alice, ACC_1, ACC_2, Store_X, Bob, ACC_3
        assert len(edges) == 6  # 3 edges per row x 2 rows
        assert len(result.nodes) == 6
        assert len(result.edges) == 6

        # 4. Interactive visualizer with live data
        viewer = result.show()
        assert viewer.default_view == "graph"
        assert len(viewer.nodes) == 6
        assert len(viewer.edges) == 6
        assert len(viewer.records) == 2
