#!/usr/bin/env python3
"""Automated Benchmark Runner and Compact CSV Summary Generator for Voyager OGM.

Executes pytest-benchmark suite and extracts statistical metrics (min, max,
mean, median, stddev, IQR, ops/sec) into a lightweight, human-readable CSV:
`benchmarks/latest_summary.csv`.

Usage:
    uv run python scripts/save_benchmarks.py [optional_custom_name]
"""

from __future__ import annotations

import csv
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


def main() -> int:
    """Runs the benchmark test suite and writes compact summary metrics to CSV."""
    benchmarks_dir = Path("benchmarks")
    benchmarks_dir.mkdir(parents=True, exist_ok=True)
    summary_csv = benchmarks_dir / "latest_summary.csv"

    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tmp_file:
        tmp_json_path = Path(tmp_file.name)

    try:
        print("[Benchmark Runner] Running benchmark test suite...")
        cmd = [
            "uv",
            "run",
            "pytest",
            "packages/python/benches/bench_compilation.py",
            "packages/python/benches/bench_hydration.py",
            "packages/python/benches/bench_network.py",
            "-k",
            "bench",
            "--benchmark-only",
            f"--benchmark-json={tmp_json_path}",
        ]

        res = subprocess.run(cmd)
        if res.returncode != 0:
            print(
                f"[Benchmark Runner] Error: Benchmark execution failed with code {res.returncode}"
            )
            return res.returncode

        if not tmp_json_path.exists() or tmp_json_path.stat().st_size == 0:
            print("[Benchmark Runner] Error: Benchmark output JSON was not generated.")
            return 1

        with open(tmp_json_path, encoding="utf-8") as f:
            bench_data = json.load(f)

        benchmarks = bench_data.get("benchmarks", [])
        if not benchmarks:
            print("[Benchmark Runner] Warning: No benchmarks recorded in test run.")
            return 0

        # Sort benchmarks deterministically by test name
        benchmarks.sort(key=lambda b: b.get("name", ""))

        fieldnames = [
            "benchmark",
            "group",
            "min_us",
            "max_us",
            "mean_us",
            "median_us",
            "stddev_us",
            "iqr_us",
            "ops_sec",
            "rounds",
            "iterations",
        ]

        rows = []
        for b in benchmarks:
            name = b.get("name", "")
            group = b.get("group") or "default"
            stats = b.get("stats", {})

            # Convert timing from seconds to microseconds
            min_us = round(stats.get("min", 0.0) * 1e6, 4)
            max_us = round(stats.get("max", 0.0) * 1e6, 4)
            mean_us = round(stats.get("mean", 0.0) * 1e6, 4)
            median_us = round(stats.get("median", 0.0) * 1e6, 4)
            stddev_us = round(stats.get("stddev", 0.0) * 1e6, 4)
            iqr_us = round(stats.get("iqr", 0.0) * 1e6, 4)
            ops_sec = round(stats.get("ops", 0.0), 2)
            rounds = stats.get("rounds", 0)
            iterations = stats.get("iterations", 1)

            rows.append(
                {
                    "benchmark": name,
                    "group": group,
                    "min_us": min_us,
                    "max_us": max_us,
                    "mean_us": mean_us,
                    "median_us": median_us,
                    "stddev_us": stddev_us,
                    "iqr_us": iqr_us,
                    "ops_sec": ops_sec,
                    "rounds": rounds,
                    "iterations": iterations,
                }
            )

        # Read existing benchmark rows to preserve baselines when live network/database tests are skipped
        merged_rows: dict[str, dict[str, Any]] = {}
        if summary_csv.exists():
            try:
                with open(summary_csv, encoding="utf-8") as f:
                    reader = csv.DictReader(f)
                    for r in reader:
                        b_name = r.get("benchmark")
                        if b_name:
                            merged_rows[b_name] = r
            except Exception:
                pass

        for r in rows:
            merged_rows[r["benchmark"]] = r

        final_rows = sorted(merged_rows.values(), key=lambda r: r.get("benchmark", ""))

        with open(summary_csv, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=fieldnames)
            writer.writeheader()
            writer.writerows(final_rows)

        print(
            f"[Benchmark Runner] Successfully wrote {len(final_rows)} benchmarks to: {summary_csv}"
        )

    finally:
        if tmp_json_path.exists():
            try:
                tmp_json_path.unlink()
            except Exception:
                pass

    return 0


if __name__ == "__main__":
    sys.exit(main())
