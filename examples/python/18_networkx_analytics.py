"""Example 18: NetworkX Graph Analytics Export Bridge.

Demonstrates exporting Voyager query execution results directly into NetworkX:
1. Execute a query with node entities and relationships.
2. Export query results directly to a NetworkX DiGraph via `result.to_networkx()`.
3. Run network analysis algorithms (PageRank, shortest paths, centrality) via NetworkX.

Run with: `uv run python examples/python/18_networkx_analytics.py`
"""

from __future__ import annotations

import networkx as nx
from voyager_ogm import MockBridge, Session


def main() -> None:
    print("=" * 70)
    print("[Example 18] NetworkX Graph Analytics Export Bridge")
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
    res = session.execute("MATCH (a:User)-[r:FOLLOWS]->(b:User) RETURN a, r, b")

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
