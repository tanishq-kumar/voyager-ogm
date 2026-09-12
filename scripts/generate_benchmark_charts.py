#!/usr/bin/env python3
"""Generates clean, responsive SVG benchmark charts for README documentation.

Produces crisp vector SVG graphics with dark/light mode support:
1. benchmarks/assets/hydration_throughput.svg
2. benchmarks/assets/concurrency_latency.svg
3. benchmarks/assets/memory_footprint.svg

Zero external dependencies required (pure Python string formatting).
"""

from pathlib import Path

ASSETS_DIR = Path("benchmarks/assets")
ASSETS_DIR.mkdir(parents=True, exist_ok=True)


def generate_hydration_chart():
    """Generates horizontal bar chart for hydration throughput."""
    svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 840 320" width="100%" height="100%" style="background:#0d1117; font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif; border-radius:12px; border:1px solid #30363d;">
  <style>
    .title { fill: #f0f6fc; font-size: 18px; font-weight: 700; }
    .subtitle { fill: #8b949e; font-size: 13px; }
    .label { fill: #c9d1d9; font-size: 14px; font-weight: 600; }
    .sublabel { fill: #8b949e; font-size: 11px; }
    .val { fill: #ffffff; font-size: 14px; font-weight: 700; }
    .badge { fill: #238636; font-size: 11px; font-weight: 700; }
    .badge-bg { fill: rgba(35, 134, 54, 0.2); stroke: #238636; stroke-width: 1; }
    .grid { stroke: #21262d; stroke-dasharray: 4,4; stroke-width: 1; }
  </style>

  <!-- Header -->
  <text x="32" y="42" class="title">In-Memory Entity Hydration Speed</text>
  <text x="32" y="64" class="subtitle">Higher is better -- 10,000 graph nodes with 6 properties (Nodes / Second)</text>

  <!-- Grid Lines -->
  <line x1="260" y1="90" x2="260" y2="280" class="grid" />
  <line x1="480" y1="90" x2="480" y2="280" class="grid" />
  <line x1="700" y1="90" x2="700" y2="280" class="grid" />

  <!-- Bar 1: Voyager OGM -->
  <text x="32" y="118" class="label">Voyager OGM</text>
  <text x="32" y="134" class="sublabel">Zero-Copy Arrow C Stream</text>
  <rect x="260" y="102" width="440" height="36" rx="6" fill="#f97316" />
  <text x="712" y="125" class="val">1,013,607 /s</text>
  <rect x="585" y="108" width="105" height="24" rx="12" class="badge-bg" />
  <text x="637" y="124" text-anchor="middle" class="badge">44.0x FASTER</text>

  <!-- Bar 2: Pydantic v2 -->
  <text x="32" y="180" class="label">Pydantic v2</text>
  <text x="32" y="196" class="sublabel">Official Driver + BaseModel</text>
  <rect x="260" y="164" width="122" height="36" rx="6" fill="#3b82f6" />
  <text x="392" y="187" class="val">280,899 /s</text>

  <!-- Bar 3: Neomodel -->
  <text x="32" y="242" class="label">neomodel</text>
  <text x="32" y="258" class="sublabel">StructuredNode.inflate()</text>
  <rect x="260" y="226" width="18" height="36" rx="6" fill="#8b5cf6" />
  <text x="288" y="249" class="val">23,016 /s</text>

  <!-- Footer Axis -->
  <line x1="260" y1="280" x2="780" y2="280" stroke="#30363d" stroke-width="1" />
  <text x="260" y="300" class="sublabel" text-anchor="middle">0</text>
  <text x="480" y="300" class="sublabel" text-anchor="middle">500,000</text>
  <text x="700" y="300" class="sublabel" text-anchor="middle">1,000,000 nodes/sec</text>
</svg>"""
    with open(ASSETS_DIR / "hydration_throughput.svg", "w", encoding="utf-8") as f:
        f.write(svg)
    print("  [OK] Generated benchmarks/assets/hydration_throughput.svg")


def generate_latency_chart():
    """Generates clustered bar chart for p50 vs p99 tail latency."""
    svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 840 320" width="100%" height="100%" style="background:#0d1117; font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif; border-radius:12px; border:1px solid #30363d;">
  <style>
    .title { fill: #f0f6fc; font-size: 18px; font-weight: 700; }
    .subtitle { fill: #8b949e; font-size: 13px; }
    .label { fill: #c9d1d9; font-size: 14px; font-weight: 600; }
    .sublabel { fill: #8b949e; font-size: 11px; }
    .val { fill: #ffffff; font-size: 13px; font-weight: 700; }
    .badge { fill: #238636; font-size: 11px; font-weight: 700; }
    .badge-bg { fill: rgba(35, 134, 54, 0.2); stroke: #238636; stroke-width: 1; }
    .grid { stroke: #21262d; stroke-dasharray: 4,4; stroke-width: 1; }
  </style>

  <!-- Header -->
  <text x="32" y="42" class="title">High-Concurrency Tail Latency (p50 vs p99)</text>
  <text x="32" y="64" class="subtitle">Lower is better -- 1,000 Point Reads across 20 Concurrent Connections (Milliseconds)</text>

  <!-- Grid Lines -->
  <line x1="280" y1="90" x2="280" y2="280" class="grid" />
  <line x1="480" y1="90" x2="480" y2="280" class="grid" />
  <line x1="680" y1="90" x2="680" y2="280" class="grid" />

  <!-- Section 1: p50 Median -->
  <text x="32" y="115" class="label">Median (p50)</text>
  <text x="32" y="130" class="sublabel">Voyager Native</text>
  <rect x="280" y="108" width="45" height="20" rx="4" fill="#06b6d4" />
  <text x="335" y="123" class="val">12.5 ms</text>

  <text x="32" y="152" class="sublabel">neo4rs (Standard)</text>
  <rect x="280" y="138" width="43" height="20" rx="4" fill="#64748b" />
  <text x="333" y="153" class="val">12.1 ms</text>

  <!-- Section 2: p99 Tail Latency -->
  <text x="32" y="200" class="label">Tail Latency (p99)</text>
  <text x="32" y="215" class="sublabel">Voyager Native</text>
  <rect x="280" y="195" width="177" height="24" rx="4" fill="#f97316" />
  <text x="467" y="212" class="val">49.5 ms</text>
  <rect x="365" y="197" width="90" height="20" rx="10" class="badge-bg" />
  <text x="410" y="211" text-anchor="middle" class="badge">2.5x LOWER</text>

  <text x="32" y="247" class="sublabel">neo4rs (Standard)</text>
  <rect x="280" y="232" width="442" height="24" rx="4" fill="#ef4444" />
  <text x="732" y="249" class="val">123.8 ms</text>

  <!-- Footer Axis -->
  <line x1="280" y1="280" x2="760" y2="280" stroke="#30363d" stroke-width="1" />
  <text x="280" y="300" class="sublabel" text-anchor="middle">0 ms</text>
  <text x="480" y="300" class="sublabel" text-anchor="middle">50 ms</text>
  <text x="680" y="300" class="sublabel" text-anchor="middle">100 ms</text>
</svg>"""
    with open(ASSETS_DIR / "concurrency_latency.svg", "w", encoding="utf-8") as f:
        f.write(svg)
    print("  [OK] Generated benchmarks/assets/concurrency_latency.svg")


def generate_memory_chart():
    """Generates bar chart for physical memory footprint."""
    svg = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 840 320" width="100%" height="100%" style="background:#0d1117; font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif; border-radius:12px; border:1px solid #30363d;">
  <style>
    .title { fill: #f0f6fc; font-size: 18px; font-weight: 700; }
    .subtitle { fill: #8b949e; font-size: 13px; }
    .label { fill: #c9d1d9; font-size: 14px; font-weight: 600; }
    .sublabel { fill: #8b949e; font-size: 11px; }
    .val { fill: #ffffff; font-size: 14px; font-weight: 700; }
    .badge { fill: #238636; font-size: 11px; font-weight: 700; }
    .badge-bg { fill: rgba(35, 134, 54, 0.2); stroke: #238636; stroke-width: 1; }
    .grid { stroke: #21262d; stroke-dasharray: 4,4; stroke-width: 1; }
  </style>

  <!-- Header -->
  <text x="32" y="42" class="title">Physical Memory Footprint (50,000 Entities)</text>
  <text x="32" y="64" class="subtitle">Lower is better -- Process Resident Set Size (RSS) Memory Delta (Megabytes)</text>

  <!-- Grid Lines -->
  <line x1="260" y1="90" x2="260" y2="280" class="grid" />
  <line x1="420" y1="90" x2="420" y2="280" class="grid" />
  <line x1="580" y1="90" x2="580" y2="280" class="grid" />
  <line x1="740" y1="90" x2="740" y2="280" class="grid" />

  <!-- Bar 1: Voyager Arrow -->
  <text x="32" y="118" class="label">Voyager / PyArrow</text>
  <text x="32" y="134" class="sublabel">Buffer raw nbytes: 2.38 MB</text>
  <rect x="260" y="102" width="37" height="36" rx="6" fill="#22c55e" />
  <text x="307" y="125" class="val">3.32 MB</text>
  <rect x="380" y="108" width="105" height="24" rx="12" class="badge-bg" />
  <text x="432" y="124" text-anchor="middle" class="badge">92% LESS RAM</text>

  <!-- Bar 2: Python Driver Dictionaries -->
  <text x="32" y="180" class="label">Python list[dict]</text>
  <text x="32" y="196" class="sublabel">Standard Driver Records</text>
  <rect x="260" y="164" width="112" height="36" rx="6" fill="#eab308" />
  <text x="382" y="187" class="val">9.83 MB</text>

  <!-- Bar 3: Pydantic v2 Models -->
  <text x="32" y="242" class="label">Pydantic v2 Models</text>
  <text x="32" y="258" class="sublabel">50,000 Model Instances</text>
  <rect x="260" y="226" width="480" height="36" rx="6" fill="#ef4444" />
  <text x="750" y="249" class="val">42.23 MB</text>

  <!-- Footer Axis -->
  <line x1="260" y1="280" x2="760" y2="280" stroke="#30363d" stroke-width="1" />
  <text x="260" y="300" class="sublabel" text-anchor="middle">0 MB</text>
  <text x="420" y="300" class="sublabel" text-anchor="middle">15 MB</text>
  <text x="580" y="300" class="sublabel" text-anchor="middle">30 MB</text>
  <text x="740" y="300" class="sublabel" text-anchor="middle">45 MB</text>
</svg>"""
    with open(ASSETS_DIR / "memory_footprint.svg", "w", encoding="utf-8") as f:
        f.write(svg)
    print("  [OK] Generated benchmarks/assets/memory_footprint.svg")


def main():
    print("=== Generating Vector SVG Benchmark Charts ===")
    generate_hydration_chart()
    generate_latency_chart()
    generate_memory_chart()
    print("=== All SVG Benchmark Charts Successfully Generated! ===")


if __name__ == "__main__":
    main()
