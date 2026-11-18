"""Voyager OGM: NetworkX Graph Analytics Export Bridge.

Teaches:
    - Exporting Voyager query results directly into NetworkX graphs via `result.to_networkx()`
    - Directed graph construction (`nx.DiGraph`) preserving edge types and weights
    - Running graph algorithms: PageRank, betweenness centrality, and shortest paths
    - In-memory analytics workflows bridging database results with NetworkX

Prerequisites:
    - Engine: None (runs in-memory / offline via MockBridge)
    - Extras: `analytics` extra (`pip install "voyager-ogm[analytics]"`)

Run with:
    `uv run python examples/python/25_networkx_analytics.py`
"""

from __future__ import annotations

import networkx as nx
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


@node(label="User")
class User(Node):
    id: int = Field(primary_key=True)
    name: str = Field()


@relationship(type_name="FOLLOWS")
class Follows(Relationship):
    weight: float = Field(default=1.0)


def main() -> None:
    print("=" * 70)
    print("[25] NetworkX Graph Analytics Export Bridge")
    print("=" * 70)

    # 1. Simulate database query returning social network graph records
    bridge = MockBridge()
    bridge.queue_result(
        [
            {
                "a": {"id": 1, "labels": ["User"], "name": "Alice"},
                "r": {"type": "FOLLOWS", "start": 1, "end": 2, "weight": 1.5},
                "b": {"id": 2, "labels": ["User"], "name": "Bob"},
            },
            {
                "a": {"id": 2, "labels": ["User"], "name": "Bob"},
                "r": {"type": "FOLLOWS", "start": 2, "end": 3, "weight": 2.0},
                "b": {"id": 3, "labels": ["User"], "name": "Charlie"},
            },
            {
                "a": {"id": 3, "labels": ["User"], "name": "Charlie"},
                "r": {"type": "FOLLOWS", "start": 3, "end": 1, "weight": 0.5},
                "b": {"id": 1, "labels": ["User"], "name": "Alice"},
            },
        ]
    )

    session = Session(bridge=bridge, dialect="cypher")
    a = User(alias="a")
    b = User(alias="b")
    r = Follows(alias="r")
    q = Query.match(a).to(r).node(b).return_(a, r, b)
    res = session.execute(q)

    # 2. Export directly into a NetworkX directed graph
    graph = res.to_networkx(directed=True)
    print(f"Exported NetworkX graph: {graph}")
    print(f"Nodes ({graph.number_of_nodes()}): {graph.nodes(data=True)}")
    print(f"Edges ({graph.number_of_edges()}): {graph.edges(data=True)}")

    # 3. Run NetworkX graph algorithms
    centrality = nx.degree_centrality(graph)
    print("\nDegree Centrality Scores:")
    for node_id, score in centrality.items():
        name = graph.nodes[node_id].get("name", node_id)
        print(f"  Node {name} (id={node_id}): {score:.4f}")

    path = nx.shortest_path(graph, source=1, target=3, weight="weight")
    print(f"\nShortest weighted path from Alice (1) to Charlie (3): {path}")


if __name__ == "__main__":
    main()
