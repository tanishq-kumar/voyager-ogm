"""Graph analytics and network export bridges for Voyager OGM.

Provides high-performance conversion from Voyager query results, Arrow tables,
and Polars DataFrames into NetworkX graphs (`networkx.Graph`, `networkx.DiGraph`,
`networkx.MultiGraph`, `networkx.MultiDiGraph`) for graph algorithms (PageRank,
centrality, shortest paths, clustering).
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    import networkx as nx


def to_networkx(
    data: Any,
    directed: bool = True,
    multigraph: bool = False,
    node_id_attr: str = "id",
    node_attrs: Sequence[str] | None = None,
    edge_attrs: Sequence[str] | None = None,
    edge_label_attr: str = "type",
    source_col: str | None = None,
    target_col: str | None = None,
    node_only: bool = False,
) -> nx.Graph | nx.DiGraph | nx.MultiGraph | nx.MultiDiGraph:
    """Exports Voyager query execution results, records, or tables into a NetworkX graph.

    Supports directed, undirected, and multi-graph variants, preserving node and
    edge property dictionaries for downstream analytics in NetworkX.

    Args:
        data: ExecutionResult, QueryResult, Polars DataFrame, list of records, or object with `to_graph()`.
        directed: Whether to create a directed graph (`DiGraph` / `MultiDiGraph`). Default True.
        multigraph: Whether to allow parallel edges between node pairs (`MultiGraph` / `MultiDiGraph`). Default False.
        node_id_attr: Name of the property representing the node's unique ID. Default "id".
        node_attrs: Optional explicit list of node attribute names to include. None preserves all properties.
        edge_attrs: Optional explicit list of edge attribute names to include. None preserves all properties.
        edge_label_attr: Attribute name under which the relationship type/label is stored on edges. Default "type".
        source_col: Explicit source node column name when converting from tabular/flat records.
            Pass empty string ("") to disable automatic edge heuristic detection.
        target_col: Explicit target node column name when converting from tabular/flat records.
            Pass empty string ("") to disable automatic edge heuristic detection.
        node_only: Whether to extract only nodes, ignoring edges. Disables automatic tabular
            edge heuristic detection when True. Default False.

    Returns:
        A NetworkX Graph (`Graph`, `DiGraph`, `MultiGraph`, or `MultiDiGraph`) populated with nodes and edges.

    Raises:
        ImportError: If NetworkX is not installed.
    """
    try:
        import networkx as nx
    except ImportError:
        raise ImportError(
            "NetworkX is required for graph export. "
            "Install it with: pip install networkx or pip install 'voyager-ogm[analytics]'"
        ) from None

    if directed:
        graph: nx.Graph | nx.DiGraph | nx.MultiGraph | nx.MultiDiGraph = (
            nx.MultiDiGraph() if multigraph else nx.DiGraph()
        )
    else:
        graph = nx.MultiGraph() if multigraph else nx.Graph()

    if data is None:
        return graph

    # Case 1: Object with to_graph() (e.g. ExecutionResult)
    nodes: list[dict[str, Any]] = []
    edges: list[dict[str, Any]] = []

    if hasattr(data, "to_graph") and callable(data.to_graph):
        nodes, edges = data.to_graph()

    if nodes or edges:
        id_map: dict[Any, Any] = {}
        for n in nodes:
            n_data = dict(n.get("data", {}))
            if node_id_attr in n_data:
                n_id = n_data[node_id_attr]
            else:
                raw_id = n.get("id")
                matched_orig = None
                for val in n_data.values():
                    if str(val) == str(raw_id):
                        matched_orig = val
                        break
                n_id = matched_orig if matched_orig is not None else raw_id

            if n.get("id") is not None:
                id_map[str(n.get("id"))] = n_id
            if n_id is not None:
                id_map[str(n_id)] = n_id
                id_map[n_id] = n_id

            props = dict(n_data)
            if "label" not in props and n.get("label"):
                props["label"] = n.get("label")

            if node_attrs is not None:
                props = {k: v for k, v in props.items() if k in node_attrs}

            graph.add_node(n_id, **props)

        if not node_only:
            for e in edges:
                src_raw = e.get("source")
                tgt_raw = e.get("target")
                src = id_map.get(src_raw, id_map.get(str(src_raw), src_raw))
                tgt = id_map.get(tgt_raw, id_map.get(str(tgt_raw), tgt_raw))

                if src is None or tgt is None:
                    continue

                props = dict(e.get("data", {}))
                if edge_label_attr and e.get("label") and edge_label_attr not in props:
                    props[edge_label_attr] = e.get("label")
                if "id" not in props and e.get("id"):
                    props["id"] = e.get("id")

                if edge_attrs is not None:
                    props = {
                        k: v for k, v in props.items() if k in edge_attrs or k == edge_label_attr
                    }

                graph.add_edge(src, tgt, **props)

        return graph

    # Case 2: Polars DataFrame, Arrow Table, or list of dictionaries
    records: list[dict[str, Any]] = []
    if hasattr(data, "to_dicts") and callable(data.to_dicts):
        records = data.to_dicts()
    elif hasattr(data, "to_pylist") and callable(data.to_pylist):
        records = data.to_pylist()
    elif isinstance(data, list):
        records = data
    elif hasattr(data, "all") and callable(data.all):
        records = data.all()

    if not records:
        return graph

    first_row = records[0]
    if not isinstance(first_row, dict):
        return graph

    col_names = list(first_row.keys())

    # Detect source and target columns
    # Explicit node_only=True or empty string for source_col/target_col disables edge detection
    detect_edges = not node_only and source_col != "" and target_col != ""
    src_c = None
    tgt_c = None
    if detect_edges:
        src_c = source_col or next(
            (
                c
                for c in ["source", "src", "from", "start", "from_id", "source_id", "u"]
                if c in col_names
            ),
            None,
        )
        tgt_c = target_col or next(
            (
                c
                for c in ["target", "dst", "to", "end", "to_id", "target_id", "v"]
                if c in col_names
            ),
            None,
        )

    if src_c and tgt_c:
        rel_c = next(
            (
                c
                for c in ["type", "rel", "relationship", "label", "edge_type", "r"]
                if c in col_names
            ),
            None,
        )
        for r in records:
            u = r.get(src_c)
            v = r.get(tgt_c)
            if u is None or v is None:
                continue

            edge_data = {k: val for k, val in r.items() if k not in (src_c, tgt_c)}
            if rel_c and edge_label_attr and rel_c in edge_data:
                edge_data[edge_label_attr] = edge_data.pop(rel_c)

            if edge_attrs is not None:
                edge_data = {
                    k: val
                    for k, val in edge_data.items()
                    if k in edge_attrs or k == edge_label_attr
                }

            graph.add_edge(u, v, **edge_data)
        return graph

    # Fallback: Node-only tabular records
    id_c = next(
        (c for c in [node_id_attr, "id", "node_id", "name", "key"] if c in col_names),
        None,
    )
    for i, r in enumerate(records):
        nid = r.get(id_c) if id_c else i
        if nid is not None:
            node_data = {k: val for k, val in r.items() if k != id_c}
            if node_attrs is not None:
                node_data = {k: val for k, val in node_data.items() if k in node_attrs}
            graph.add_node(nid, **node_data)

    return graph
