"""Graph entity and path topology reconstruction engine for Voyager GraphViewer."""

from __future__ import annotations

import re
from collections.abc import Sequence
from typing import Any


def extract_graph_pattern_from_cypher(
    stmt: str,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Extracts node and edge topology directly from Cypher query path patterns."""
    nodes_dict: dict[str, dict[str, Any]] = {}
    edges_list: list[dict[str, Any]] = []

    if not stmt or not isinstance(stmt, str):
        return [], []

    pattern_clauses = re.findall(
        r"(?:MATCH|CREATE|MERGE|OPTIONAL MATCH)\s+(.*?)(?=\s+(?:WHERE|RETURN|WITH|CREATE|MERGE|SET|DELETE|REMOVE|ORDER BY|SKIP|LIMIT)|$)",
        stmt,
        re.IGNORECASE | re.DOTALL,
    )
    if not pattern_clauses:
        pattern_clauses = [stmt]

    full_pattern = " , ".join(pattern_clauses)
    subpaths = full_pattern.split(",")

    node_counter = 0

    node_regex = re.compile(
        r"\(\s*([a-zA-Z0-9_]*)(?:\s*:\s*([a-zA-Z0-9_]+))?(?:\s*\{([^}]*)\})?\s*\)"
    )
    rel_regex = re.compile(
        r"(<)?-\s*(?:\[\s*([a-zA-Z0-9_]*)(?:\s*:\s*([a-zA-Z0-9_]+))?(?:\s*\{([^}]*)\})?\s*\])?-(>)?\s*"
    )

    for path_str in subpaths:
        path_str = path_str.strip()
        if not path_str:
            continue

        pos = 0
        last_node_id = None

        while pos < len(path_str):
            if last_node_id is None:
                n_match = node_regex.match(path_str, pos)
                if n_match:
                    var_name, label_name, props_str = n_match.groups()
                    node_counter += 1
                    node_id = (
                        var_name
                        if var_name
                        else (
                            label_name.lower() + f"_{node_counter}"
                            if label_name
                            else f"n{node_counter}"
                        )
                    )
                    lbl = label_name if label_name else (var_name if var_name else node_id)
                    grp = label_name if label_name else "Entity"

                    if node_id not in nodes_dict:
                        nodes_dict[node_id] = {
                            "id": node_id,
                            "label": lbl,
                            "group": grp,
                            "size": 11,
                            "data": {"variable": var_name} if var_name else {},
                        }

                    last_node_id = node_id
                    pos = n_match.end()
                    continue
                else:
                    pos += 1
                    continue

            r_match = rel_regex.match(path_str, pos)
            if r_match:
                left_arrow, rel_var, rel_type, rel_props, right_arrow = r_match.groups()
                pos = r_match.end()

                next_n_match = node_regex.match(path_str, pos)
                if next_n_match:
                    next_var, next_label, next_props = next_n_match.groups()
                    node_counter += 1
                    next_node_id = (
                        next_var
                        if next_var
                        else (
                            next_label.lower() + f"_{node_counter}"
                            if next_label
                            else f"n{node_counter}"
                        )
                    )
                    next_lbl = (
                        next_label if next_label else (next_var if next_var else next_node_id)
                    )
                    next_grp = next_label if next_label else "Entity"

                    if next_node_id not in nodes_dict:
                        nodes_dict[next_node_id] = {
                            "id": next_node_id,
                            "label": next_lbl,
                            "group": next_grp,
                            "size": 11,
                            "data": {"variable": next_var} if next_var else {},
                        }

                    src = next_node_id if left_arrow else last_node_id
                    tgt = last_node_id if left_arrow else next_node_id
                    edges_list.append(
                        {
                            "source": src,
                            "target": tgt,
                            "label": rel_type if rel_type else "CONNECTED_TO",
                            "color": "#64748b",
                            "data": {"variable": rel_var} if rel_var else {},
                        }
                    )

                    last_node_id = next_node_id
                    pos = next_n_match.end()
                    continue

            last_node_id = None
            pos += 1

    return list(nodes_dict.values()), edges_list


_extract_graph_pattern_from_cypher = extract_graph_pattern_from_cypher


def extract_path_topology_from_query(
    query: Any,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Extracts graph path patterns directly from a Voyager Query or Cypher statement."""
    cypher_stmt = ""
    if hasattr(query, "compile"):
        try:
            cypher_stmt = query.compile("cypher").statement
        except Exception:
            cypher_stmt = str(query)
    else:
        cypher_stmt = str(query)

    return extract_graph_pattern_from_cypher(cypher_stmt)


def extract_graph_entities_from_records(
    records: Sequence[dict[str, Any]],
    statement: str = "",
    query: Any = None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Extracts live graph nodes, relationships, and reconstructed paths from database session records.

    Inspects returned records for direct entity objects (Node, Rel, Path) or aligns
    query path patterns (MATCH / CREATE / MERGE) to row column values.
    """
    if not records:
        return [], []

    nodes_dict: dict[str, dict[str, Any]] = {}
    edges_list: list[dict[str, Any]] = []

    has_entity_objects = False
    for r in records:
        if not isinstance(r, dict):
            continue
        for k, v in r.items():
            if isinstance(v, dict):
                if "labels" in v or "_labels" in v or "label" in v:
                    n_id = str(v.get("id", v.get("_id", k)))
                    lbls = v.get("labels", v.get("_labels", [v.get("label", "Node")]))
                    lbl = lbls[0] if isinstance(lbls, list) and lbls else str(lbls)
                    props = v.get(
                        "properties",
                        {
                            pk: pv
                            for pk, pv in v.items()
                            if pk not in ("labels", "_labels", "label", "id", "_id")
                        },
                    )
                    nodes_dict[n_id] = {
                        "id": n_id,
                        "label": lbl,
                        "group": lbl,
                        "size": 11,
                        "data": props,
                    }
                    has_entity_objects = True
                elif "type" in v or "_type" in v:
                    e_type = str(v.get("type", v.get("_type", "CONNECTED_TO")))
                    src = str(v.get("start", v.get("source", v.get("start_node_id", ""))))
                    tgt = str(v.get("end", v.get("target", v.get("end_node_id", ""))))
                    if src and tgt:
                        props = v.get(
                            "properties",
                            {
                                pk: pv
                                for pk, pv in v.items()
                                if pk not in ("type", "_type", "start", "source", "end", "target")
                            },
                        )
                        edges_list.append(
                            {
                                "id": f"rel_{len(edges_list)}",
                                "source": src,
                                "target": tgt,
                                "label": e_type,
                                "color": "#64748b",
                                "data": props,
                            }
                        )
                        has_entity_objects = True

    if has_entity_objects and (nodes_dict or edges_list):
        return list(nodes_dict.values()), edges_list

    cypher_stmt = ""
    if statement:
        cypher_stmt = statement
    elif query is not None:
        if hasattr(query, "compile"):
            try:
                cypher_stmt = query.compile("cypher").statement
            except Exception:
                cypher_stmt = str(query)
        else:
            cypher_stmt = str(query)

    if cypher_stmt:
        node_regex = re.compile(
            r"\(\s*([a-zA-Z0-9_]*)(?:\s*:\s*([a-zA-Z0-9_]+))?(?:\s*\{([^}]*)\})?\s*\)"
        )
        rel_regex = re.compile(
            r"(<)?-\s*(?:\[\s*([a-zA-Z0-9_]*)(?:\s*:\s*([a-zA-Z0-9_]+))?(?:\s*\{([^}]*)\})?\s*\])?-(>)?\s*"
        )

        pattern_clauses = re.findall(
            r"(?:MATCH|CREATE|MERGE|OPTIONAL MATCH)\s+(.*?)(?=\s+(?:WHERE|RETURN|WITH|CREATE|MERGE|SET|DELETE|REMOVE|ORDER BY|SKIP|LIMIT)|$)",
            cypher_stmt,
            re.IGNORECASE | re.DOTALL,
        )
        if not pattern_clauses:
            pattern_clauses = [cypher_stmt]

        full_pattern = " , ".join(pattern_clauses)
        subpaths = full_pattern.split(",")

        path_links = []
        for path_str in subpaths:
            path_str = path_str.strip()
            if not path_str:
                continue

            pos = 0
            last_node = None

            while pos < len(path_str):
                if last_node is None:
                    n_match = node_regex.match(path_str, pos)
                    if n_match:
                        var_name, label_name, _ = n_match.groups()
                        last_node = {
                            "var": var_name or "",
                            "label": label_name or (var_name or "Entity"),
                        }
                        pos = n_match.end()
                        continue
                    else:
                        pos += 1
                        continue

                r_match = rel_regex.match(path_str, pos)
                if r_match:
                    left_arrow, rel_var, rel_type, _, right_arrow = r_match.groups()
                    pos = r_match.end()
                    next_n_match = node_regex.match(path_str, pos)
                    if next_n_match:
                        next_var, next_label, _ = next_n_match.groups()
                        next_node = {
                            "var": next_var or "",
                            "label": next_label or (next_var or "Entity"),
                        }
                        pos = next_n_match.end()

                        src_node = next_node if left_arrow else last_node
                        tgt_node = last_node if left_arrow else next_node
                        path_links.append(
                            {
                                "src": src_node,
                                "tgt": tgt_node,
                                "rel_var": rel_var or "",
                                "rel_type": rel_type or "CONNECTED_TO",
                            }
                        )
                        last_node = next_node
                        continue

                last_node = None
                pos += 1

        if path_links:
            first_row = records[0]
            col_names = list(first_row.keys())

            var_to_col: dict[str, str] = {}
            for m in re.finditer(
                r"\b([a-zA-Z0-9_]+)(?:\.[a-zA-Z0-9_]+)?\s+AS\s+([a-zA-Z0-9_]+)\b",
                cypher_stmt,
                re.IGNORECASE,
            ):
                v_name, col_name = m.groups()
                var_to_col[v_name.lower()] = col_name

            for m in re.finditer(
                r"\b([a-zA-Z0-9_]+)\.([a-zA-Z0-9_]+)\b",
                cypher_stmt,
            ):
                v_name, prop_name = m.groups()
                if v_name.lower() not in var_to_col:
                    for c in col_names:
                        if (
                            c.lower() == f"{v_name.lower()}.{prop_name.lower()}"
                            or c.lower() == prop_name.lower()
                        ):
                            var_to_col[v_name.lower()] = c
                            break

            def find_col_for_entity(entity_info: dict[str, Any]) -> str | None:
                var = str(entity_info.get("var") or "").lower()
                if var and var in var_to_col and var_to_col[var] in col_names:
                    return var_to_col[var]

                lbl = str(entity_info.get("label") or "").lower()
                for c in col_names:
                    clow = c.lower()
                    if var and (clow == var or clow == f"{var}_id" or clow == f"id_{var}"):
                        return c
                    if lbl and (clow == lbl or clow == f"{lbl}_id" or clow == f"id_{lbl}"):
                        return c
                for c in col_names:
                    clow = c.lower()
                    if var and (
                        clow.startswith(f"{var}_")
                        or clow.endswith(f"_{var}")
                        or (len(var) > 2 and var in clow)
                    ):
                        return c
                    if lbl and (
                        clow.startswith(f"{lbl}_")
                        or clow.endswith(f"_{lbl}")
                        or (len(lbl) > 2 and lbl in clow)
                    ):
                        return c
                return None

            edge_idx = 0
            for r in records:
                for link in path_links:
                    src_info = link.get("src")
                    tgt_info = link.get("tgt")
                    if not isinstance(src_info, dict) or not isinstance(tgt_info, dict):
                        continue

                    src_col = find_col_for_entity(src_info)
                    tgt_col = find_col_for_entity(tgt_info)

                    if src_col and tgt_col:
                        src_val = r.get(src_col)
                        tgt_val = r.get(tgt_col)

                        if src_val is not None and tgt_val is not None:
                            s_id = str(src_val)
                            t_id = str(tgt_val)

                            if s_id not in nodes_dict:
                                nodes_dict[s_id] = {
                                    "id": s_id,
                                    "label": s_id,
                                    "group": str(src_info.get("label") or "Node"),
                                    "size": 11,
                                    "data": {src_col: src_val},
                                }

                            if t_id not in nodes_dict:
                                nodes_dict[t_id] = {
                                    "id": t_id,
                                    "label": t_id,
                                    "group": str(tgt_info.get("label") or "Node"),
                                    "size": 11,
                                    "data": {tgt_col: tgt_val},
                                }

                            rel_props = {}
                            rel_var_str = str(link.get("rel_var") or "")
                            if rel_var_str:
                                rv = rel_var_str.lower()
                                for k, v in r.items():
                                    if k not in (src_col, tgt_col) and (
                                        rv in k.lower()
                                        or "amount" in k.lower()
                                        or "weight" in k.lower()
                                        or "since" in k.lower()
                                    ):
                                        rel_props[k] = v

                            edge_idx += 1
                            edges_list.append(
                                {
                                    "id": f"edge_{edge_idx}_{s_id}_{t_id}",
                                    "source": s_id,
                                    "target": t_id,
                                    "label": link["rel_type"],
                                    "color": "#64748b",
                                    "data": rel_props,
                                }
                            )

            if nodes_dict:
                return list(nodes_dict.values()), edges_list

    first_row = records[0]
    col_names = list(first_row.keys())
    src_col = next(
        (
            c
            for c in ["source", "from", "src", "start", "u", "from_id", "source_id"]
            if c in col_names
        ),
        None,
    )
    tgt_col = next(
        (c for c in ["target", "to", "dst", "end", "v", "to_id", "target_id"] if c in col_names),
        None,
    )
    rel_col = next(
        (c for c in ["rel", "relationship", "type", "label", "edge_type", "r"] if c in col_names),
        None,
    )

    if src_col and tgt_col:
        for idx, r in enumerate(records):
            s_val = r.get(src_col)
            t_val = r.get(tgt_col)
            if s_val is not None and t_val is not None:
                s_id = str(s_val)
                t_id = str(t_val)
                r_lbl = str(r.get(rel_col, "CONNECTED_TO")) if rel_col else "CONNECTED_TO"
                if s_id not in nodes_dict:
                    nodes_dict[s_id] = {
                        "id": s_id,
                        "label": s_id,
                        "group": "Node",
                        "size": 11,
                        "data": {k: v for k, v in r.items() if k not in (tgt_col, rel_col)},
                    }
                if t_id not in nodes_dict:
                    nodes_dict[t_id] = {
                        "id": t_id,
                        "label": t_id,
                        "group": "Node",
                        "size": 11,
                        "data": {k: v for k, v in r.items() if k not in (src_col, rel_col)},
                    }
                edges_list.append(
                    {
                        "id": f"e_{idx}_{s_id}_{t_id}",
                        "source": s_id,
                        "target": t_id,
                        "label": r_lbl,
                        "color": "#64748b",
                        "data": {
                            k: v for k, v in r.items() if k not in (src_col, tgt_col, rel_col)
                        },
                    }
                )
        return list(nodes_dict.values()), edges_list

    id_col = next((c for c in ["id", "node_id", "name", "key"] if c in col_names), None)
    if id_col:
        for r in records:
            val = r.get(id_col)
            if val is not None:
                val_id = str(val)
                if val_id not in nodes_dict:
                    nodes_dict[val_id] = {
                        "id": val_id,
                        "label": val_id,
                        "group": "Node",
                        "size": 11,
                        "data": r,
                    }
        return list(nodes_dict.values()), []

    return [], []
