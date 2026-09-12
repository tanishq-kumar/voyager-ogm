#!/usr/bin/env python3
"""Generates clean, responsive SVG benchmark charts for README documentation.

Produces vector SVG graphics with dark mode styling:
1. benchmarks/assets/hydration_throughput.svg
2. benchmarks/assets/concurrency_latency.svg
3. benchmarks/assets/memory_footprint.svg

"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

# Default output directory
DEFAULT_ASSETS_DIR = Path("benchmarks/assets")
DEFAULT_DATA_FILE = Path("benchmarks/benchmark_summary.json")

# Semantic Color Palette
PALETTE: dict[str, dict[str, str]] = {
    "voyager": {
        "default": "#f97316",  # Orange
        "light": "#fb923c",  # Orange (lighter variant for p50)
    },
    "driver": {
        "default": "#3b82f6",  # Driver Sky Blue (neo4rs / python dicts)
        "light": "#60a5fa",  # Blue (lighter variant for p50)
    },
    "pydantic": {
        "default": "#8b5cf6",  # Pydantic Violet
        "light": "#a78bfa",
    },
    "neomodel": {
        "default": "#64748b",  # Neomodel Slate Gray
        "light": "#94a3b8",
    },
}

# Shared SVG styling
SVG_STYLES = """
    .title { fill: #f0f6fc; font-size: 18px; font-weight: 700; }
    .subtitle { fill: #8b949e; font-size: 13px; }
    .label { fill: #c9d1d9; font-size: 14px; font-weight: 600; }
    .sublabel { fill: #8b949e; font-size: 11px; }
    .val { fill: #ffffff; font-size: 13px; font-weight: 700; }
    .badge { fill: #3fb950; font-size: 11px; font-weight: 700; }
    .badge-bg { fill: rgba(35, 134, 54, 0.2); stroke: #238636; stroke-width: 1; }
    .grid { stroke: #21262d; stroke-dasharray: 4,4; stroke-width: 1; }
"""

# Fixed, perfectly aligned badge coordinates across all charts
BADGE_X = 690
BADGE_Y = 108
BADGE_WIDTH = 118
BADGE_HEIGHT = 24
BADGE_RX = 12
BADGE_TEXT_X = 749
BADGE_TEXT_Y = 124

# Usable chart bar region
CHART_X_START = 250
CHART_X_END = 660
CHART_USABLE_WIDTH = CHART_X_END - CHART_X_START  # 410 px


def get_color(entity_type: str, tone: str = "default") -> str:
    """Returns the harmonized color token for a given entity type."""
    type_colors = PALETTE.get(entity_type, PALETTE["driver"])
    return type_colors.get(tone, type_colors["default"])


def format_number(val: float | int, unit: str = "", precision: int | None = None) -> str:
    """Formats numeric values dynamically with commas or decimals."""
    if precision is not None:
        formatted = f"{val:.{precision}f}"
    elif isinstance(val, int) or (isinstance(val, float) and val.is_integer()):
        formatted = f"{int(val):,}"
    elif val >= 100:
        formatted = f"{val:.1f}"
    else:
        formatted = f"{val:.2f}"
    return f"{formatted}{(' ' + unit) if unit else ''}"


def compute_annotation_badge(annotation_spec: dict[str, Any], items: list[dict[str, Any]]) -> str:
    """Computes the annotation badge text dynamically from the data."""
    val_map = {item["id"]: float(item["value"]) for item in items}
    target_val = val_map.get(annotation_spec.get("target", ""))
    baseline_val = val_map.get(annotation_spec.get("baseline", ""))

    if target_val is None or baseline_val is None:
        return ""

    badge_type = annotation_spec.get("type", "speedup")
    fmt_str = annotation_spec.get("format", "")

    if badge_type == "speedup":
        ratio = target_val / baseline_val if baseline_val > 0 else 0.0
        return fmt_str.format(ratio=ratio) if fmt_str else f"{ratio:.1f}x FASTER"
    elif badge_type == "ratio_lower":
        ratio = baseline_val / target_val if target_val > 0 else 0.0
        return fmt_str.format(ratio=ratio) if fmt_str else f"{ratio:.1f}x LOWER"
    elif badge_type == "percent_less":
        percent = (1.0 - (target_val / baseline_val)) * 100.0 if baseline_val > 0 else 0.0
        return fmt_str.format(percent=percent) if fmt_str else f"{percent:.0f}% LESS RAM"

    return ""


def generate_bar_chart(data: dict[str, Any], output_path: Path) -> None:
    """Generates a dynamic 3-bar horizontal benchmark chart."""
    title = data["title"]
    subtitle = data["subtitle"]
    axis_max = float(data["axis_max"])
    axis_ticks = data["axis_ticks"]
    unit = data.get("display_unit", "")
    precision = data.get("precision")
    items = data["items"]

    # Compute annotation badge dynamically
    badge_text = ""
    if "annotation" in data:
        badge_text = compute_annotation_badge(data["annotation"], items)

    # Build grid lines & axis labels
    grid_svg = []
    tick_svg = []
    for tick in axis_ticks:
        tick_x = CHART_X_START + int((tick / axis_max) * CHART_USABLE_WIDTH)
        grid_svg.append(f'  <line x1="{tick_x}" y1="90" x2="{tick_x}" y2="270" class="grid" />')
        if data.get("unit") and ("nodes" in data["unit"]):
            tick_label = f"{tick:,}" if tick > 0 else "0"
            if tick == axis_ticks[-1]:
                tick_label = f"{tick:,} {data['unit']}"
        else:
            tick_label = f"{tick} {unit}".strip()
        tick_svg.append(
            f'  <text x="{tick_x}" y="292" class="sublabel" text-anchor="middle">{tick_label}</text>'
        )

    # Build bars dynamically
    bar_svg = []
    y_starts = [102, 164, 226]
    for i, item in enumerate(items[:3]):
        y = y_starts[i]
        val = float(item["value"])
        bar_w = max(int((val / axis_max) * CHART_USABLE_WIDTH), 8)
        color = get_color(item.get("entity_type", "driver"), item.get("tone", "default"))
        val_str = format_number(val, unit, precision)

        # Labels
        bar_svg.append(f"  <!-- Bar {i + 1}: {item['label']} -->")
        bar_svg.append(f'  <text x="32" y="{y + 16}" class="label">{item["label"]}</text>')
        bar_svg.append(f'  <text x="32" y="{y + 32}" class="sublabel">{item["sublabel"]}</text>')

        # Bar Rect
        bar_svg.append(
            f'  <rect x="{CHART_X_START}" y="{y}" width="{bar_w}" height="36" rx="6" fill="{color}" />'
        )

        # Value text placed right after the bar
        val_x = CHART_X_START + bar_w + 12
        bar_svg.append(f'  <text x="{val_x}" y="{y + 23}" class="val">{val_str}</text>')

        # Pin badge on row 0 (Voyager) at exact unified coordinate
        if i == 0 and badge_text:
            bar_svg.append(
                f'  <rect x="{BADGE_X}" y="{BADGE_Y}" width="{BADGE_WIDTH}" height="{BADGE_HEIGHT}" rx="{BADGE_RX}" class="badge-bg" />'
            )
            bar_svg.append(
                f'  <text x="{BADGE_TEXT_X}" y="{BADGE_TEXT_Y}" text-anchor="middle" class="badge">{badge_text}</text>'
            )

    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 840 320" width="100%" height="100%" style="background:#0d1117; font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif; border-radius:12px; border:1px solid #30363d;">
  <style>{SVG_STYLES}  </style>

  <!-- Header -->
  <text x="32" y="42" class="title">{title}</text>
  <text x="32" y="64" class="subtitle">{subtitle}</text>

  <!-- Grid Lines -->
{chr(10).join(grid_svg)}

{chr(10).join(bar_svg)}

  <!-- Footer Axis -->
  <line x1="{CHART_X_START}" y1="270" x2="{CHART_X_END}" y2="270" stroke="#30363d" stroke-width="1" />
{chr(10).join(tick_svg)}
</svg>"""

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as f:
        f.write(svg)
    print(f"  [OK] Generated {output_path}")


def generate_latency_chart(data: dict[str, Any], output_path: Path) -> None:
    """Generates a dynamic multi-section latency chart (p99 tail vs p50 median)."""
    title = data["title"]
    subtitle = data["subtitle"]
    axis_max = float(data["axis_max"])
    axis_ticks = data["axis_ticks"]
    unit = data.get("display_unit", "ms")
    precision = data.get("precision")
    sections = data["sections"]

    # Compute annotation badge for section 1 (p99 tail latency)
    badge_text = ""
    sec1 = sections[0]
    if "annotation" in sec1:
        badge_text = compute_annotation_badge(sec1["annotation"], sec1["items"])

    # Build grid lines & axis labels
    grid_svg = []
    tick_svg = []
    for tick in axis_ticks:
        tick_x = CHART_X_START + int((tick / axis_max) * CHART_USABLE_WIDTH)
        grid_svg.append(f'  <line x1="{tick_x}" y1="90" x2="{tick_x}" y2="270" class="grid" />')
        tick_label = f"{tick} {unit}"
        tick_svg.append(
            f'  <text x="{tick_x}" y="292" class="sublabel" text-anchor="middle">{tick_label}</text>'
        )

    content_svg = []

    # Section 1: p99 Tail Latency (at top, anchoring badge at y=108)
    p99_items = sec1["items"]
    content_svg.append(f"  <!-- Section 1: {sec1['name']} -->")
    content_svg.append(f'  <text x="32" y="100" class="label">{sec1["name"]}</text>')

    # Item 1: Voyager Native (p99)
    voy_p99 = p99_items[0]
    voy_p99_val = float(voy_p99["value"])
    voy_p99_w = int((voy_p99_val / axis_max) * CHART_USABLE_WIDTH)
    voy_p99_color = get_color(voy_p99.get("entity_type", "voyager"), voy_p99.get("tone", "default"))
    content_svg.append(f'  <text x="32" y="126" class="sublabel">{voy_p99["label"]}</text>')
    content_svg.append(
        f'  <rect x="{CHART_X_START}" y="110" width="{voy_p99_w}" height="24" rx="4" fill="{voy_p99_color}" />'
    )
    content_svg.append(
        f'  <text x="{CHART_X_START + voy_p99_w + 10}" y="127" class="val">{format_number(voy_p99_val, unit, precision)}</text>'
    )

    # Pin unified badge at exact identical position (x=690, y=108)
    if badge_text:
        content_svg.append(
            f'  <rect x="{BADGE_X}" y="{BADGE_Y}" width="{BADGE_WIDTH}" height="{BADGE_HEIGHT}" rx="{BADGE_RX}" class="badge-bg" />'
        )
        content_svg.append(
            f'  <text x="{BADGE_TEXT_X}" y="{BADGE_TEXT_Y}" text-anchor="middle" class="badge">{badge_text}</text>'
        )

    # Item 2: neo4rs Driver (p99)
    neo_p99 = p99_items[1]
    neo_p99_val = float(neo_p99["value"])
    neo_p99_w = int((neo_p99_val / axis_max) * CHART_USABLE_WIDTH)
    neo_p99_color = get_color(neo_p99.get("entity_type", "driver"), neo_p99.get("tone", "default"))
    content_svg.append(f'  <text x="32" y="160" class="sublabel">{neo_p99["label"]}</text>')
    content_svg.append(
        f'  <rect x="{CHART_X_START}" y="144" width="{neo_p99_w}" height="24" rx="4" fill="{neo_p99_color}" />'
    )
    content_svg.append(
        f'  <text x="{CHART_X_START + neo_p99_w + 10}" y="161" class="val">{format_number(neo_p99_val, unit, precision)}</text>'
    )

    # Section 2: p50 Median (at bottom)
    sec2 = sections[1]
    p50_items = sec2["items"]
    content_svg.append(f"  <!-- Section 2: {sec2['name']} -->")
    content_svg.append(f'  <text x="32" y="198" class="label">{sec2["name"]}</text>')

    # Item 3: Voyager Native (p50)
    voy_p50 = p50_items[0]
    voy_p50_val = float(voy_p50["value"])
    voy_p50_w = max(int((voy_p50_val / axis_max) * CHART_USABLE_WIDTH), 8)
    voy_p50_color = get_color(voy_p50.get("entity_type", "voyager"), voy_p50.get("tone", "light"))
    content_svg.append(f'  <text x="32" y="222" class="sublabel">{voy_p50["label"]}</text>')
    content_svg.append(
        f'  <rect x="{CHART_X_START}" y="206" width="{voy_p50_w}" height="20" rx="4" fill="{voy_p50_color}" />'
    )
    content_svg.append(
        f'  <text x="{CHART_X_START + voy_p50_w + 10}" y="221" class="val">{format_number(voy_p50_val, unit, precision)}</text>'
    )

    # Item 4: neo4rs Driver (p50)
    neo_p50 = p50_items[1]
    neo_p50_val = float(neo_p50["value"])
    neo_p50_w = max(int((neo_p50_val / axis_max) * CHART_USABLE_WIDTH), 8)
    neo_p50_color = get_color(neo_p50.get("entity_type", "driver"), neo_p50.get("tone", "light"))
    content_svg.append(f'  <text x="32" y="254" class="sublabel">{neo_p50["label"]}</text>')
    content_svg.append(
        f'  <rect x="{CHART_X_START}" y="238" width="{neo_p50_w}" height="20" rx="4" fill="{neo_p50_color}" />'
    )
    content_svg.append(
        f'  <text x="{CHART_X_START + neo_p50_w + 10}" y="253" class="val">{format_number(neo_p50_val, unit, precision)}</text>'
    )

    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 840 320" width="100%" height="100%" style="background:#0d1117; font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif; border-radius:12px; border:1px solid #30363d;">
  <style>{SVG_STYLES}  </style>

  <!-- Header -->
  <text x="32" y="42" class="title">{title}</text>
  <text x="32" y="64" class="subtitle">{subtitle}</text>

  <!-- Grid Lines -->
{chr(10).join(grid_svg)}

{chr(10).join(content_svg)}

  <!-- Footer Axis -->
  <line x1="{CHART_X_START}" y1="270" x2="{CHART_X_END}" y2="270" stroke="#30363d" stroke-width="1" />
{chr(10).join(tick_svg)}
</svg>"""

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as f:
        f.write(svg)
    print(f"  [OK] Generated {output_path}")


def load_benchmark_data(data_path: Path) -> dict[str, Any]:
    """Loads benchmark summary data from JSON file with safe fallbacks."""
    if not data_path.exists():
        raise FileNotFoundError(f"Benchmark summary data file not found: {data_path}")
    with open(data_path, encoding="utf-8") as f:
        return json.load(f)


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate dynamic SVG benchmark charts.")
    parser.add_argument(
        "--data",
        type=Path,
        default=DEFAULT_DATA_FILE,
        help="Path to benchmark summary JSON file.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_ASSETS_DIR,
        help="Directory to save generated SVG charts.",
    )
    args = parser.parse_args()

    print(f"=== Generating Vector SVG Benchmark Charts from {args.data} ===")
    data = load_benchmark_data(args.data)

    generate_bar_chart(data["hydration"], args.output_dir / "hydration_throughput.svg")
    generate_latency_chart(data["latency"], args.output_dir / "concurrency_latency.svg")
    generate_bar_chart(data["memory"], args.output_dir / "memory_footprint.svg")

    print("=== All SVG Benchmark Charts Successfully Generated! ===")


if __name__ == "__main__":
    main()
