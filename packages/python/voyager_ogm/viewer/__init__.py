"""Voyager Graph Studio & Universal Notebook Viewer.

Universal interactive graph network and tabular records visualizer for:
- Marimo
- JupyterLab & Jupyter Notebook
- VS Code Interactive Notebooks
- Standalone Web / HTML dashboards
"""

from __future__ import annotations

from typing import Any

from voyager_ogm.viewer.extractor import (
    extract_graph_entities_from_records,
    extract_graph_pattern_from_cypher,
    extract_path_topology_from_query,
)
from voyager_ogm.viewer.widget import GraphViewer


def show(
    query_or_records: Any,
    session: Any = None,
    **kwargs: Any,
) -> GraphViewer:
    """Convenience helper to immediately visualize a query, records, or DataFrame in GraphViewer.

    Args:
        query_or_records: Voyager Query, DataFrame, or list of dictionary records.
        session: Optional live Session to execute the query.
        **kwargs: Extra parameters passed to GraphViewer.

    Returns:
        Interactive GraphViewer widget instance.
    """
    if hasattr(query_or_records, "to_polars"):
        return GraphViewer.from_records(
            raw_records=query_or_records.all()
            if hasattr(query_or_records, "all")
            else list(query_or_records),
            **kwargs,
        )
    if hasattr(query_or_records, "compile"):
        return GraphViewer.from_query(query=query_or_records, session=session, **kwargs)
    if hasattr(query_or_records, "to_dicts"):
        return GraphViewer.from_polars(df=query_or_records, **kwargs)
    if isinstance(query_or_records, list):
        return GraphViewer.from_records(raw_records=query_or_records, **kwargs)
    return GraphViewer.from_cypher(cypher=str(query_or_records), session=session, **kwargs)


def explore(
    query_or_records: Any,
    session: Any = None,
    **kwargs: Any,
) -> GraphViewer:
    """Synonym for `show()`."""
    return show(query_or_records, session=session, **kwargs)


def visualize_query(
    query: Any,
    session: Any = None,
    **kwargs: Any,
) -> GraphViewer:
    """Explicit query visualizer constructing a GraphViewer from a query pattern."""
    return GraphViewer.from_query(query=query, session=session, **kwargs)


__all__ = [
    "GraphViewer",
    "explore",
    "extract_graph_entities_from_records",
    "extract_graph_pattern_from_cypher",
    "extract_path_topology_from_query",
    "show",
    "visualize_query",
]
