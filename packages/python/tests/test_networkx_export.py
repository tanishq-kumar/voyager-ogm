"""Unit tests for NetworkX graph export bridge (Issue #117)."""

from __future__ import annotations

import sys
from unittest.mock import MagicMock

import networkx as nx
import polars as pl
import pytest
from voyager_ogm import (
    AsyncMockBridge,
    AsyncSession,
    Field,
    MockBridge,
    Node,
    QueryResult,
    Relationship,
    Session,
    node,
    relationship,
    reset_alias_counters,
    to_networkx,
)


@node("Person")
class Person(Node):
    id: Field[int] = Field()
    name: Field[str] = Field()
    age: Field[int] = Field()


@relationship("KNOWS")
class Knows(Relationship):
    since: Field[int] = Field()
    weight: Field[float] = Field()


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


def test_to_networkx_directed_and_undirected():
    bridge = MockBridge()
    bridge.queue_result(
        [
            {"source": "1", "target": "2", "type": "KNOWS", "weight": 1.5},
            {"source": "2", "target": "3", "type": "KNOWS", "weight": 2.5},
        ]
    )
    session = Session(bridge=bridge, dialect="cypher")
    res = session.execute("MATCH (a)-[r]->(b) RETURN a, r, b")

    # Directed
    g_dir = res.to_networkx(directed=True)
    assert isinstance(g_dir, nx.DiGraph)
    assert not isinstance(g_dir, nx.MultiDiGraph)
    assert g_dir.number_of_nodes() == 3
    assert g_dir.number_of_edges() == 2
    assert g_dir.has_edge("1", "2")
    assert not g_dir.has_edge("2", "1")
    assert g_dir["1"]["2"]["weight"] == 1.5

    # Undirected
    bridge.queue_result(
        [
            {"source": "1", "target": "2", "type": "KNOWS", "weight": 1.5},
            {"source": "2", "target": "3", "type": "KNOWS", "weight": 2.5},
        ]
    )
    res_undir = session.execute("MATCH (a)-[r]->(b) RETURN a, r, b")
    g_undir = res_undir.to_networkx(directed=False)
    assert isinstance(g_undir, nx.Graph)
    assert not isinstance(g_undir, nx.DiGraph)
    assert g_undir.has_edge("1", "2")
    assert g_undir.has_edge("2", "1")


def test_to_networkx_multigraph():
    bridge = MockBridge()
    # Parallel edges between 1 and 2
    records = [
        {"source": "1", "target": "2", "type": "KNOWS", "weight": 1.0},
        {"source": "1", "target": "2", "type": "WORKS_WITH", "weight": 2.0},
    ]

    # Simple DiGraph overwrites parallel edges
    bridge.queue_result(records)
    session = Session(bridge=bridge, dialect="cypher")
    res1 = session.execute("MATCH (a)-[r]->(b) RETURN a, r, b")
    g_simple = res1.to_networkx(directed=True, multigraph=False)
    assert len(g_simple.edges()) == 1

    # MultiDiGraph preserves all parallel edges
    bridge.queue_result(records)
    res2 = session.execute("MATCH (a)-[r]->(b) RETURN a, r, b")
    g_multi = res2.to_networkx(directed=True, multigraph=True)
    assert isinstance(g_multi, nx.MultiDiGraph)
    assert len(g_multi.edges()) == 2

    # Undirected MultiGraph
    bridge.queue_result(records)
    res3 = session.execute("MATCH (a)-[r]->(b) RETURN a, r, b")
    g_multi_undir = res3.to_networkx(directed=False, multigraph=True)
    assert isinstance(g_multi_undir, nx.MultiGraph)
    assert not isinstance(g_multi_undir, nx.MultiDiGraph)
    assert len(g_multi_undir.edges()) == 2


def test_to_networkx_graph_entities_extraction():
    bridge = MockBridge()
    bridge.queue_result(
        [
            {
                "p": {"labels": ["Person"], "id": 10, "name": "Alice", "age": 30},
                "r": {"type": "KNOWS", "start": 10, "end": 20, "since": 2020},
                "f": {"labels": ["Person"], "id": 20, "name": "Bob", "age": 25},
            }
        ]
    )
    session = Session(bridge=bridge, dialect="cypher")
    res = session.execute("MATCH (p)-[r]->(f) RETURN p, r, f")
    graph = res.to_networkx(directed=True)

    assert graph.number_of_nodes() == 2
    assert graph.number_of_edges() == 1
    assert graph.nodes[10]["name"] == "Alice"
    assert graph.nodes[10]["label"] == "Person"
    assert graph.nodes[20]["name"] == "Bob"
    assert graph[10][20]["since"] == 2020
    assert graph[10][20]["type"] == "KNOWS"


def test_to_networkx_attribute_filtering():
    bridge = MockBridge()
    bridge.queue_result(
        [
            {
                "p": {
                    "labels": ["Person"],
                    "id": 1,
                    "name": "Alice",
                    "age": 30,
                    "secret": "hidden",
                },
                "r": {"type": "KNOWS", "start": 1, "end": 2, "since": 2020, "weight": 0.9},
                "f": {"labels": ["Person"], "id": 2, "name": "Bob", "age": 25, "secret": "hidden"},
            }
        ]
    )
    session = Session(bridge=bridge, dialect="cypher")
    res = session.execute("MATCH (p)-[r]->(f) RETURN p, r, f")

    graph = res.to_networkx(
        directed=True,
        node_attrs=["name"],
        edge_attrs=["weight"],
    )

    assert "name" in graph.nodes[1]
    assert "age" not in graph.nodes[1]
    assert "secret" not in graph.nodes[1]
    assert "weight" in graph[1][2]
    assert "since" not in graph[1][2]


def test_to_networkx_tabular_custom_columns():
    bridge = MockBridge()
    bridge.queue_result(
        [
            {"src_usr": 100, "dst_usr": 200, "cost": 42},
            {"src_usr": 200, "dst_usr": 300, "cost": 50},
        ]
    )
    session = Session(bridge=bridge, dialect="cypher")
    res = session.execute("MATCH ...")
    graph = res.to_networkx(source_col="src_usr", target_col="dst_usr")

    assert graph.number_of_nodes() == 3
    assert graph.number_of_edges() == 2
    assert graph[100][200]["cost"] == 42


def test_to_networkx_node_only_records():
    bridge = MockBridge()
    bridge.queue_result(
        [
            {"id": "A", "name": "Alpha", "active": True},
            {"id": "B", "name": "Beta", "active": False},
        ]
    )
    session = Session(bridge=bridge, dialect="cypher")
    res = session.execute("MATCH (n) RETURN n.id AS id, n.name AS name, n.active AS active")
    graph = res.to_networkx(node_id_attr="id")

    assert graph.number_of_nodes() == 2
    assert graph.number_of_edges() == 0
    assert graph.nodes["A"]["name"] == "Alpha"
    assert graph.nodes["A"]["active"] is True


def test_to_networkx_empty_result():
    bridge = MockBridge()
    bridge.queue_result([])
    session = Session(bridge=bridge, dialect="cypher")
    res = session.execute("MATCH (n) WHERE false RETURN n")
    graph = res.to_networkx()

    assert isinstance(graph, nx.DiGraph)
    assert graph.number_of_nodes() == 0
    assert graph.number_of_edges() == 0


def test_to_networkx_polars_dataframe_direct():
    df = pl.DataFrame(
        {
            "source": [1, 2, 3],
            "target": [2, 3, 4],
            "weight": [0.1, 0.2, 0.3],
        }
    )
    graph = to_networkx(df)
    assert graph.number_of_nodes() == 4
    assert graph.number_of_edges() == 3
    assert graph[1][2]["weight"] == 0.1


def test_session_query_fluent_to_networkx():
    bridge = MockBridge()
    bridge.queue_result(
        [
            {"id": 1, "name": "Alice", "age": 30},
            {"id": 2, "name": "Bob", "age": 25},
        ]
    )
    session = Session(bridge=bridge, dialect="cypher")
    p = Person("p")
    graph = session.query(p).where(p.age > 21).to_networkx()

    assert graph.number_of_nodes() == 2
    assert 1 in graph.nodes or "1" in graph.nodes
    assert len(bridge.executed_queries) == 1
    assert "WHERE p.age > $p0" in bridge.executed_queries[0][0]


@pytest.mark.asyncio
async def test_async_session_query_fluent_to_networkx():
    bridge = AsyncMockBridge()
    bridge.sync_mock.queue_result(
        [
            {"id": 1, "name": "Alice", "age": 30},
        ]
    )
    session = AsyncSession(bridge=bridge, dialect="cypher")
    p = Person("p")
    graph = await session.query(p).where(p.age > 21).to_networkx()

    assert graph.number_of_nodes() == 1
    assert len(bridge.sync_mock.executed_queries) == 1


def test_networkx_graph_algorithms_interop():
    bridge = MockBridge()
    bridge.queue_result(
        [
            {"source": "A", "target": "B", "weight": 1.0},
            {"source": "B", "target": "C", "weight": 2.0},
            {"source": "A", "target": "C", "weight": 5.0},
        ]
    )
    session = Session(bridge=bridge, dialect="cypher")
    res = session.execute("MATCH (a)-[r]->(b) RETURN a, r, b")
    graph = res.to_networkx(directed=True)

    # Shortest path
    path = nx.shortest_path(graph, "A", "C", weight="weight")
    assert path == ["A", "B", "C"]

    # Degree centrality
    centrality = nx.degree_centrality(graph)
    assert centrality["B"] == 1.0

    # Weakly connected components
    components = list(nx.weakly_connected_components(graph))
    assert len(components) == 1
    assert components[0] == {"A", "B", "C"}


def test_to_networkx_missing_dependency_raises_import_error(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setitem(sys.modules, "networkx", None)

    with pytest.raises(ImportError, match="NetworkX is required for graph export"):
        to_networkx([])


def test_streaming_query_result_to_networkx():
    mock_stream = MagicMock()
    mock_stream.num_rows = 2
    mock_stream.num_columns = 2
    mock_stream.to_dicts.return_value = [
        {"source": "X", "target": "Y", "label": "LINKS_TO"},
    ]
    mock_stream.is_consumed = False

    qr = QueryResult(mock_stream)
    graph = qr.to_networkx()

    assert isinstance(graph, nx.DiGraph)
    assert graph.number_of_nodes() == 2
    assert graph.has_edge("X", "Y")
    assert graph["X"]["Y"]["type"] == "LINKS_TO"
