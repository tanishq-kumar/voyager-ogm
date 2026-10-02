"""Example 14: Native Query Topology Extraction & Inspection.

Demonstrates extracting graph topology directly from the AST arena:
1. Build a multi-hop traversal query using Voyager's fluent DSL.
2. Extract graph topology with `query.extract_topology()`.
3. Inspect versioned JSON structure, node properties, edge directions, and hops.
4. Bridge extracted topology into visualization tooling or downstream AST consumers.

Run with: `uv run python examples/python/14_topology_extraction.py`
"""

from __future__ import annotations

import json

from voyager_ogm import Field, Node, Query, Relationship, node, relationship


@node(labels=["Person"])
class Person(Node):
    name: str = Field(primary_key=True)
    city: str = Field(default="London")


@node(labels=["Movie"])
class Movie(Node):
    title: str = Field(primary_key=True)
    released: int = Field(default=2000)


@relationship(type_name="ACTED_IN")
class ActedIn(Relationship):
    role: str = Field(default="Lead")


def main() -> None:
    print("=" * 70)
    print("[Example 14] Native Query Topology Extraction")
    print("=" * 70)

    # 1. Build a fluent query traversing Person -> ACTED_IN -> Movie
    p = Person(alias="p")
    m = Movie(alias="m")
    act = ActedIn(alias="a")

    q = (
        Query.match(p)
        .where(p.city == "London")
        .to(act)
        .node(m)
        .where(m.released >= 1999)
        .return_(actor=p.name, movie=m.title)
    )

    # 2. Extract structured topology from native AST arena (zero regex)
    topo = q.extract_topology()

    # 3. Print versioned JSON payload
    print(f"Topology JSON Version: {topo.get('version', 1)}")
    print(f"Extracted Nodes: {len(topo['nodes'])}, Edges: {len(topo['edges'])}\n")
    print("Structured Topology Payload:")
    print(json.dumps(topo, indent=2))

    # 4. Point out key topological invariants
    print("\n" + "-" * 70)
    print("Topological Invariants:")
    for n in topo["nodes"]:
        print(f"  • Node '{n['id']}': labels={n['labels']}, props={n['properties']}")
    for e in topo["edges"]:
        print(
            f"  • Edge '{e['id']}': {e['source']} -[{e['label']}]-> {e['target']} (dir={e['direction']})"
        )

    # 5. Bridge to interactive viewer (GraphViewer widget)
    print("\nVisualizing graph pattern with GraphViewer:")
    viewer = q.show()
    print(f"Viewer Component Initialized: {viewer.__class__.__name__}")
    print("=" * 70)


if __name__ == "__main__":
    main()
