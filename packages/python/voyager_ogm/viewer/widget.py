"""Universal interactive GraphViewer widget for Jupyter, VS Code, Marimo, and web runtimes."""

from __future__ import annotations

import html
import pathlib
from collections.abc import Sequence
from typing import TYPE_CHECKING, Any

import polars as pl

from voyager_ogm.viewer.extractor import (
    extract_graph_entities_from_records,
    extract_graph_pattern_from_cypher,
)

if TYPE_CHECKING:
    from voyager_ogm.query import CompiledQuery, Query
    from voyager_ogm.session import Session

try:
    import anywidget
    import traitlets

    HAS_ANYWIDGET = True
except ImportError:
    HAS_ANYWIDGET = False


if TYPE_CHECKING:
    import anywidget

    class _BaseWidget(anywidget.AnyWidget):
        pass

elif HAS_ANYWIDGET:

    class _BaseWidget(anywidget.AnyWidget):
        pass

else:

    class _BaseWidget:  # type: ignore[no-redef]
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            pass


_STATIC_DIR = pathlib.Path(__file__).parent / "static"
_ESM_FILE = _STATIC_DIR / "viewer.js"
_CSS_FILE = _STATIC_DIR / "viewer.css"


def _read_esm_source() -> str:
    """Reads ESM Javascript source code from static assets."""
    if _ESM_FILE.exists():
        return _ESM_FILE.read_text(encoding="utf-8")
    return ""


def _read_css_source() -> str:
    """Reads CSS source code from static assets."""
    if _CSS_FILE.exists():
        return _CSS_FILE.read_text(encoding="utf-8")
    return ""


class GraphViewer(_BaseWidget):
    """Universal interactive notebook graph studio and records visualizer.

    Compatible across:
    - **Marimo** (`marimo.ui.anywidget(viewer)`)
    - **JupyterLab** & **Jupyter Notebook 7+**
    - **VS Code Interactive Notebooks** (`.ipynb`)
    - **Google Colab**, **Deepnote**, and standalone HTML exports

    Key Features:
    - Pristine Light Theme default with obsidian dark mode toggle.
    - Live Graph Entity & Path Extraction from database session records.
    - Query Path Topology Auto-Extraction without database execution.
    - Lucide micro-iconography & tactile spring micro-animations.
    - Collapsible Left Panel for node labels and relationship types.
    - Multi-Layout Engine: Force-Directed (alpha-cooling), Circular, and Hierarchical DAG.
    - Smooth 400ms coordinate interpolation tween when switching layouts.
    - Adaptive node radius scaling with degree centrality & glow aura.
    - Quadratic curved opposing multi-edges and reflexive self-loops.
    - Level-of-Detail Cartesian dot grid with zoom clamping and high-res PNG export.
    - Linear-grade floating glass Inspector drawer with 1-click JSON copy.
    - Paginated / virtualized Table View with RFC-4180 CSV export.
    - Multi-Dialect Syntax-Highlighted Query Studio (Cypher, ISO GQL, SQL:2023 PGQ).
    """

    if HAS_ANYWIDGET:
        _esm = _ESM_FILE if _ESM_FILE.exists() else _read_esm_source()
        _css = _CSS_FILE if _CSS_FILE.exists() else _read_css_source()

        nodes = traitlets.List(traitlets.Dict()).tag(sync=True)
        edges = traitlets.List(traitlets.Dict()).tag(sync=True)
        records = traitlets.List(traitlets.Dict()).tag(sync=True)
        column_types = traitlets.Dict().tag(sync=True)
        query_statement = traitlets.Unicode(default_value="").tag(sync=True)
        gql_statement = traitlets.Unicode(default_value="").tag(sync=True)
        pgq_statement = traitlets.Unicode(default_value="").tag(sync=True)
        selected_node = traitlets.Unicode(default_value="").tag(sync=True)
        selected_edge = traitlets.Unicode(default_value="").tag(sync=True)
        default_view = traitlets.Unicode(default_value="auto").tag(sync=True)
        height = traitlets.Unicode(default_value="620px").tag(sync=True)
        theme = traitlets.Unicode(default_value="light").tag(sync=True)

    def __init__(
        self,
        nodes: Sequence[dict[str, Any]] | None = None,
        edges: Sequence[dict[str, Any]] | None = None,
        records: Sequence[dict[str, Any]] | None = None,
        column_types: dict[str, str] | None = None,
        query_statement: str = "",
        gql_statement: str = "",
        pgq_statement: str = "",
        default_view: str = "auto",
        height: str = "620px",
        theme: str = "light",
        **kwargs: Any,
    ) -> None:
        if HAS_ANYWIDGET:
            super().__init__(**kwargs)
            self.nodes = list(nodes) if nodes else []
            self.edges = list(edges) if edges else []
            self.records = list(records) if records else []
            self.column_types = column_types or {}
            self.query_statement = query_statement
            self.gql_statement = gql_statement or query_statement
            self.pgq_statement = pgq_statement or query_statement
            self.default_view = default_view
            self.height = height
            self.theme = theme
        else:
            self.nodes = list(nodes) if nodes else []
            self.edges = list(edges) if edges else []
            self.records = list(records) if records else []
            self.column_types = column_types or {}
            self.query_statement = query_statement
            self.gql_statement = gql_statement or query_statement
            self.pgq_statement = pgq_statement or query_statement
            self.selected_node = ""
            self.selected_edge = ""
            self.default_view = default_view
            self.height = height
            self.theme = theme

    @property
    def dataframe(self) -> pl.DataFrame:
        """Returns the underlying records as a Polars DataFrame."""
        return self.to_polars()

    def to_polars(self) -> pl.DataFrame:
        """Exports the records stored in the viewer as a Polars DataFrame."""
        if self.records:
            return pl.DataFrame(self.records)
        if self.nodes:
            return pl.DataFrame(self.nodes)
        return pl.DataFrame()

    @classmethod
    def from_polars(
        cls,
        df: pl.DataFrame,
        source_col: str | None = None,
        target_col: str | None = None,
        label_col: str | None = None,
        edge_label_col: str | None = None,
        default_view: str = "auto",
        height: str = "620px",
        theme: str = "light",
        **kwargs: Any,
    ) -> GraphViewer:
        """Constructs a GraphViewer directly from a Polars DataFrame, auto-detecting graph topology."""
        nodes_dict: dict[str, dict[str, Any]] = {}
        edges_list: list[dict[str, Any]] = []

        if df.is_empty():
            return cls(
                nodes=[],
                edges=[],
                records=[],
                column_types={},
                default_view="table",
                height=height,
                theme=theme,
            )

        # Sanitize Polars types for AnyWidget / JSON serialization
        exprs = []
        for col, dtype in zip(df.columns, df.dtypes, strict=False):
            if dtype in (pl.Date, pl.Datetime, pl.Time, pl.Duration):
                exprs.append(pl.col(col).cast(pl.String))
            elif isinstance(dtype, pl.Decimal):
                exprs.append(pl.col(col).cast(pl.Float64))
            elif dtype == pl.Binary:
                exprs.append(pl.col(col).cast(pl.String))
        sanitized_df = df.with_columns(exprs) if exprs else df
        rows = sanitized_df.to_dicts()
        col_names = sanitized_df.columns
        col_types = {
            col: str(dtype)
            for col, dtype in zip(sanitized_df.columns, sanitized_df.dtypes, strict=False)
        }

        src_candidate = source_col
        tgt_candidate = target_col
        rel_candidate = edge_label_col

        if not src_candidate:
            for c in ["source", "from", "src", "start", "head", "u", "from_id", "source_id"]:
                if c in col_names:
                    src_candidate = c
                    break

        if not tgt_candidate:
            for c in ["target", "to", "dst", "end", "tail", "v", "to_id", "target_id", "friend"]:
                if c in col_names:
                    tgt_candidate = c
                    break

        if not rel_candidate:
            for c in ["rel", "relationship", "type", "label", "edge_type", "r"]:
                if c in col_names:
                    rel_candidate = c
                    break

        has_edges = bool(src_candidate and tgt_candidate)

        if has_edges:
            for r in rows:
                src_raw = r.get(src_candidate)
                tgt_raw = r.get(tgt_candidate)

                if src_raw is None or tgt_raw is None:
                    continue

                src_val = str(src_raw)
                tgt_val = str(tgt_raw)

                if src_val and src_val not in nodes_dict:
                    lbl = (
                        str(r.get(label_col, src_val))
                        if label_col and label_col in r and r[label_col] is not None
                        else src_val
                    )
                    nodes_dict[src_val] = {
                        "id": src_val,
                        "label": lbl,
                        "size": 11,
                        "data": {
                            k: v for k, v in r.items() if k not in (tgt_candidate, rel_candidate)
                        },
                    }

                if tgt_val and tgt_val not in nodes_dict:
                    tgt_lbl = (
                        str(r.get(label_col, tgt_val))
                        if label_col and label_col in r and r[label_col] is not None
                        else tgt_val
                    )
                    nodes_dict[tgt_val] = {
                        "id": tgt_val,
                        "label": tgt_lbl,
                        "size": 11,
                        "data": {
                            k: v for k, v in r.items() if k not in (src_candidate, rel_candidate)
                        },
                    }

                if src_val and tgt_val:
                    e_lbl = (
                        str(r.get(rel_candidate, ""))
                        if rel_candidate and rel_candidate in r and r[rel_candidate] is not None
                        else ""
                    )
                    edge_props = {
                        k: v
                        for k, v in r.items()
                        if k not in (src_candidate, tgt_candidate, rel_candidate)
                    }
                    edges_list.append(
                        {
                            "source": src_val,
                            "target": tgt_val,
                            "label": e_lbl,
                            "color": "#64748b",
                            "data": edge_props,
                        }
                    )
        elif "id" in col_names or "node_id" in col_names or (label_col and label_col in col_names):
            for r in rows:
                n_id = str(r.get("id", r.get("node_id", "")))
                if n_id and n_id not in nodes_dict:
                    nodes_dict[n_id] = {
                        "id": n_id,
                        "label": str(r.get(label_col, n_id)) if label_col else n_id,
                        "size": 11,
                        "data": r,
                    }

        detected_default = (
            default_view if default_view != "auto" else ("graph" if has_edges else "table")
        )

        return cls(
            nodes=list(nodes_dict.values()),
            edges=edges_list,
            records=rows,
            column_types=col_types,
            default_view=detected_default,
            height=height,
            theme=theme,
            **kwargs,
        )

    @classmethod
    def from_records(
        cls,
        raw_records: Sequence[dict[str, Any]] | None = None,
        nodes: Sequence[dict[str, Any]] | None = None,
        edges: Sequence[dict[str, Any]] | None = None,
        source_key: str | None = None,
        target_key: str | None = None,
        label_key: str | None = None,
        edge_label_key: str | None = None,
        query_statement: str = "",
        default_view: str = "auto",
        height: str = "620px",
        theme: str = "light",
        **kwargs: Any,
    ) -> GraphViewer:
        """Constructs a GraphViewer from dictionary records or explicit node/edge lists."""
        if raw_records is not None and not nodes and not edges:
            extracted_nodes, extracted_edges = extract_graph_entities_from_records(
                records=raw_records,
                statement=query_statement,
            )
            detected_default = (
                default_view
                if default_view != "auto"
                else ("graph" if extracted_edges or extracted_nodes else "table")
            )
            return cls(
                nodes=extracted_nodes,
                edges=extracted_edges,
                records=raw_records,
                query_statement=query_statement,
                default_view=detected_default,
                height=height,
                theme=theme,
                **kwargs,
            )
        return cls(
            nodes=nodes,
            edges=edges,
            records=raw_records,
            query_statement=query_statement,
            default_view=default_view,
            height=height,
            theme=theme,
            **kwargs,
        )

    @classmethod
    def from_query(
        cls,
        query: Query | CompiledQuery | str,
        session: Session | Any | None = None,
        default_view: str = "auto",
        height: str = "620px",
        theme: str = "light",
        **kwargs: Any,
    ) -> GraphViewer:
        """Constructs a GraphViewer directly from a Voyager Query with multi-dialect compilation.

        If `session` is provided, executes the query and visualizes the resulting records and graph entities.
        If `session` is None, automatically extracts any graph path patterns (MATCH / CREATE / MERGE)
        and visualizes the query topology immediately in Graph View.
        """
        from voyager_ogm.query import CompiledQuery, Query

        cypher_stmt = ""
        gql_stmt = ""
        pgq_stmt = ""

        if isinstance(query, Query):
            try:
                compiled = query.compile("cypher")
                cypher_stmt = compiled.statement
            except Exception:
                cypher_stmt = str(query)
            try:
                gql_stmt = query.compile("iso_gql").statement
            except Exception:
                gql_stmt = cypher_stmt
            try:
                pgq_stmt = query.compile("sql_pgq", graph_name="graph_catalog").statement
            except Exception:
                pgq_stmt = cypher_stmt
        elif isinstance(query, CompiledQuery):
            cypher_stmt = query.statement
            gql_stmt = query.statement
            pgq_stmt = query.statement
        else:
            cypher_stmt = str(query)

        if session is not None:
            exec_res = session.execute(query)
            extracted_nodes, extracted_edges = exec_res.to_graph()
            detected_default = (
                default_view
                if default_view != "auto"
                else ("graph" if extracted_edges or extracted_nodes else "table")
            )
            return cls(
                nodes=extracted_nodes,
                edges=extracted_edges,
                records=exec_res.all(),
                query_statement=cypher_stmt,
                gql_statement=gql_stmt,
                pgq_statement=pgq_stmt,
                default_view=detected_default,
                height=height,
                theme=theme,
                **kwargs,
            )

        extracted_nodes, extracted_edges = extract_graph_pattern_from_cypher(cypher_stmt)

        detected_default = (
            default_view if default_view != "auto" else ("graph" if extracted_nodes else "query")
        )

        return cls(
            nodes=extracted_nodes,
            edges=extracted_edges,
            records=[],
            query_statement=cypher_stmt,
            gql_statement=gql_stmt,
            pgq_statement=pgq_stmt,
            default_view=detected_default,
            height=height,
            theme=theme,
            **kwargs,
        )

    @classmethod
    def from_cypher(
        cls,
        cypher: str,
        session: Session | Any | None = None,
        default_view: str = "auto",
        height: str = "620px",
        theme: str = "light",
        **kwargs: Any,
    ) -> GraphViewer:
        """Constructs a GraphViewer directly from a Cypher query string."""
        return cls.from_query(
            query=cypher,
            session=session,
            default_view=default_view,
            height=height,
            theme=theme,
            **kwargs,
        )

    def to_html(self) -> str:
        """Generates a standalone, self-contained HTML page embedding the viewer and data."""
        import json

        data_json = json.dumps(
            {
                "nodes": self.nodes,
                "edges": self.edges,
                "records": self.records,
                "column_types": self.column_types,
                "query_statement": self.query_statement,
                "gql_statement": self.gql_statement,
                "pgq_statement": self.pgq_statement,
                "default_view": self.default_view,
                "height": self.height,
                "theme": self.theme,
            },
            default=str,
        )

        esm_code = _read_esm_source()
        css_code = _read_css_source()

        bg_color = "#f8fafc" if self.theme == "light" else "#080c14"

        return f"""<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Voyager Graph Studio</title>
  <style>
    *, *::before, *::after {{ box-sizing: border-box; }}
    html, body {{
      margin: 0;
      padding: 0;
      width: 100%;
      height: 100%;
      overflow: hidden;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      background: {bg_color};
    }}
    #voyager-app-root {{
      width: 100%;
      height: 100%;
      display: flex;
      flex-direction: column;
    }}
    {css_code}
  </style>
</head>
<body>
  <div id="voyager-app-root"></div>
  <script type="module">
    {esm_code}

    const state = {data_json};
    const container = document.getElementById("voyager-app-root");

    const mockModel = {{
      get: (key) => state[key],
      set: (key, val) => {{ state[key] = val; }},
      on: () => {{}},
      off: () => {{}},
      save_changes: () => {{}}
    }};

    render({{ model: mockModel, el: container }});
  </script>
</body>
</html>"""

    def _repr_html_(self) -> str:
        """HTML representation embedding the interactive studio inside a responsive sandboxed iframe.

        Guarantees zero-dependency, 100% offline rendering across VS Code Notebooks,
        JupyterLab, Marimo, Google Colab, and static HTML reports without requiring
        custom widget manager registration or CDN downloads.
        """
        raw_html = self.to_html()
        escaped_srcdoc = html.escape(raw_html, quote=True)
        h = str(self.height) if self.height else "620px"
        if h.isdigit():
            h = f"{h}px"
        return (
            f'<iframe srcdoc="{escaped_srcdoc}" '
            f'style="width: 100%; height: {h}; min-height: 480px; border: 1px solid #cbd5e1; border-radius: 8px; box-shadow: 0 1px 3px 0 rgb(0 0 0 / 0.1);" '
            f'sandbox="allow-scripts allow-downloads allow-same-origin allow-modals" '
            f'loading="lazy"></iframe>'
        )

    def _repr_mimebundle_(
        self,
        **kwargs: Any,
    ) -> tuple[dict[str, Any], dict[str, Any]] | None:
        """Provides MIME bundle prioritizing self-contained HTML for resilient notebook display."""
        return (
            {
                "text/plain": repr(self),
                "text/html": self._repr_html_(),
            },
            {},
        )
