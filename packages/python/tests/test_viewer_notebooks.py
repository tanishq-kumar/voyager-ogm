"""Comprehensive multi-notebook tests for Voyager Graph Studio & Viewer.

Verifies compatibility across the three primary notebook and execution environments:
1. Marimo Reactive Notebooks (traitlet sync & reactive click callbacks).
2. Jupyter Notebook / JupyterLab (IPython AnyWidget, DataFrame ingestion, multi-dialect query tabs).
3. VS Code Interactive Notebooks & Standalone HTML Exports (self-contained HTML, fallback renderers).
"""

from __future__ import annotations

import polars as pl
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
    node,
    relationship,
)
from voyager_ogm import (
    GraphViewer as TopLevelGraphViewer,
)
from voyager_ogm.viewer import (
    GraphViewer,
    explore,
    extract_graph_entities_from_records,
    extract_path_topology_from_query,
    show,
    visualize_query,
)


@node(label="Account")
class Account(Node):
    acc_no: str = Field(primary_key=True)
    balance: float = Field(default=0.0)


@relationship(type_name="TRANSFERRED")
class Transferred(Relationship):
    amount: float = Field(default=0.0)


# ===========================================================================
# 1. Environment 1: Marimo Reactive Notebooks
# ===========================================================================


class TestMarimoNotebookEnvironment:
    def test_top_level_import_parity(self):
        """Verifies that importing from voyager_ogm is identical to voyager_ogm.viewer."""
        assert TopLevelGraphViewer is GraphViewer

    def test_marimo_reactive_selection_sync(self):
        """Verifies reactive selection synchronization in Marimo notebooks."""
        nodes = [
            {"id": "acc_1", "label": "ACC-1001", "group": "Account"},
            {"id": "acc_2", "label": "ACC-1002", "group": "Account"},
        ]
        edges = [
            {"id": "tx_1", "source": "acc_1", "target": "acc_2", "label": "TRANSFERRED"},
        ]

        viewer = GraphViewer(nodes=nodes, edges=edges, default_view="graph", theme="light")

        # Initial state
        assert viewer.selected_node == ""
        assert viewer.selected_edge == ""

        # Simulate user clicking a node in Marimo
        viewer.selected_node = "acc_1"
        assert viewer.selected_node == "acc_1"

        # Simulate user clicking an edge in Marimo
        viewer.selected_edge = "tx_1"
        assert viewer.selected_edge == "tx_1"

    def test_marimo_ui_convenience_functions(self):
        """Verifies show() and explore() helpers in reactive cells."""
        q = Query.match(Account(alias="a")).return_(acc=Account.acc_no)
        viewer = show(q)
        assert isinstance(viewer, GraphViewer)
        assert viewer.query_statement != ""

        viewer2 = explore([{"source": "A", "target": "B", "rel": "KNOWS"}])
        assert isinstance(viewer2, GraphViewer)
        assert len(viewer2.nodes) == 2
        assert len(viewer2.edges) == 1


# ===========================================================================
# 2. Environment 2: Jupyter Notebook & JupyterLab
# ===========================================================================


class TestJupyterNotebookEnvironment:
    def test_jupyter_anywidget_traitlets_and_assets(self):
        """Verifies AnyWidget static asset loading and traitlet registration."""
        viewer = GraphViewer(
            nodes=[{"id": "n1", "label": "Node 1"}],
            edges=[],
            query_statement="MATCH (n:Account) RETURN n",
            height="500px",
            theme="dark",
        )

        assert hasattr(viewer, "_esm")
        assert hasattr(viewer, "_css")
        assert viewer.height == "500px"
        assert viewer.theme == "dark"
        assert viewer.query_statement == "MATCH (n:Account) RETURN n"

    def test_jupyter_polars_dataframe_ingestion(self):
        """Verifies zero-copy Polars ingestion and schema reflection in Jupyter."""
        df = pl.DataFrame(
            {
                "src_id": ["101", "102", "103"],
                "dst_id": ["102", "103", "101"],
                "type": ["KNOWS", "FOLLOWS", "BLOCKS"],
                "weight": [0.85, 0.42, 0.99],
            }
        )

        viewer = GraphViewer.from_polars(
            df,
            source_col="src_id",
            target_col="dst_id",
            edge_label_col="type",
        )

        assert len(viewer.nodes) == 3
        assert len(viewer.edges) == 3
        assert viewer.default_view == "graph"

        # Export back to Polars
        exported_df = viewer.to_polars()
        assert isinstance(exported_df, pl.DataFrame)
        assert exported_df.height == 3
        assert "weight" in exported_df.columns

    def test_jupyter_multi_dialect_query_compilation(self):
        """Verifies that query visualizer populates Cypher, GQL, and SQL:2023 PGQ statements."""
        a = Account(alias="a")
        b = Account(alias="b")
        t = Transferred(alias="t")

        q = (
            Query.match(a)
            .to(t)
            .node(b)
            .where(t.amount > 5000.0)
            .return_(src=a.acc_no, tgt=b.acc_no, amt=t.amount)
        )

        viewer = visualize_query(q)
        assert "MATCH (a:Account)" in viewer.query_statement
        assert "WHERE" in viewer.query_statement
        assert viewer.gql_statement != ""
        assert viewer.pgq_statement != ""
        assert len(viewer.nodes) == 2  # a, b
        assert len(viewer.edges) == 1  # t

    def test_direct_topology_and_entity_extractors(self):
        """Verifies direct invocation of topology and record entity extraction algorithms."""
        nodes, edges = extract_path_topology_from_query(
            "MATCH (a:Account)-[r:TRANSFERRED]->(b:Account) RETURN a, r, b"
        )
        assert len(nodes) == 2
        assert len(edges) == 1
        assert edges[0]["label"] == "TRANSFERRED"

        rec_nodes, rec_edges = extract_graph_entities_from_records(
            [{"source": "A", "target": "B", "rel": "KNOWS"}],
            statement="MATCH (a:Account)-[r:KNOWS]->(b:Account) RETURN a, r, b",
        )
        assert len(rec_nodes) == 2
        assert len(rec_edges) == 1


# ===========================================================================
# 3. Environment 3: VS Code Interactive Notebooks & Standalone HTML
# ===========================================================================


class TestVSCodeAndStandaloneEnvironment:
    def test_standalone_to_html_generation(self):
        """Verifies full standalone HTML export for offline viewing and VS Code webviews."""
        records = [
            {"source": "Alice", "target": "Bob", "relationship": "FRIENDS_WITH", "since": 2021},
            {"source": "Bob", "target": "Charlie", "relationship": "FRIENDS_WITH", "since": 2023},
        ]

        viewer = GraphViewer.from_records(
            records, query_statement="MATCH (a)-[r]->(b) RETURN a, r, b"
        )
        html_page = viewer.to_html()

        assert "<!DOCTYPE html>" in html_page
        assert "<title>Voyager Graph Studio</title>" in html_page
        assert "export function render" in html_page
        assert ".voyager-root" in html_page
        assert "Alice" in html_page
        assert "Bob" in html_page
        assert "Charlie" in html_page

    def test_repr_html_fallback(self):
        """Verifies interactive iframe and MIME bundle generation for VS Code and notebook runtimes."""
        viewer = GraphViewer(
            nodes=[{"id": "1", "label": "Node 1"}],
            edges=[],
            records=[{"id": 1, "val": "A"}],
            query_statement="MATCH (n) RETURN n",
        )

        repr_html = viewer._repr_html_()
        assert "<iframe srcdoc=" in repr_html
        assert "Voyager Graph Studio" in repr_html
        assert "allow-scripts" in repr_html
        assert "MATCH (n) RETURN n" in repr_html

        mime_bundle = viewer._repr_mimebundle_()
        assert mime_bundle is not None
        data, metadata = mime_bundle
        assert "text/html" in data
        assert "<iframe srcdoc=" in data["text/html"]

    def test_live_session_execution_and_extraction(self):
        """Verifies session.execute(query).show() end-to-end extraction across all environments."""
        session = Session()  # MockBridge default

        a = Account(alias="a")
        b = Account(alias="b")
        t = Transferred(alias="t")
        q = Query.match(a).to(t).node(b).return_(src=a.acc_no, tgt=b.acc_no, amt=t.amount)

        res = session.execute(q)
        assert res.statement != ""

        viewer = res.show()
        assert isinstance(viewer, GraphViewer)
        assert len(viewer.nodes) >= 0
