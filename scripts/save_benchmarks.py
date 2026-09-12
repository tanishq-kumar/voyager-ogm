#!/usr/bin/env python3
"""Automated Benchmark Runner and Dynamic Artifact Saver for Voyager OGM.

Dynamically derives the active Git branch and commit SHA to name and persist
benchmark artifacts in the `benchmarks/` directory without hardcoding names.

Usage:
    uv run python scripts/save_benchmarks.py [optional_custom_name]
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
from pathlib import Path


def get_git_info() -> tuple[str, str]:
    """Retrieves the sanitized active Git branch and short commit hash."""
    try:
        branch = subprocess.check_output(
            ["git", "rev-parse", "--abbrev-ref", "HEAD"], text=True
        ).strip()
    except Exception:
        branch = "main"

    try:
        commit = subprocess.check_output(["git", "rev-parse", "--short", "HEAD"], text=True).strip()
    except Exception:
        commit = "head"

    # Sanitize branch name for safe filenames (replace / and special chars with _)
    sanitized_branch = re.sub(r"[^a-zA-Z0-9_\-\.]", "_", branch)
    return sanitized_branch, commit


def main() -> int:
    """Runs the benchmark test suite and writes the JSON results to disk."""
    benchmarks_dir = Path("benchmarks")
    benchmarks_dir.mkdir(parents=True, exist_ok=True)

    if len(sys.argv) > 1 and sys.argv[1].strip():
        base_name = re.sub(r"[^a-zA-Z0-9_\-\.]", "_", sys.argv[1].strip())
    else:
        branch, commit = get_git_info()
        base_name = f"benchmark_{branch}"

    target_json = benchmarks_dir / f"{base_name}.json"
    latest_json = benchmarks_dir / "latest_benchmark_results.json"

    print(f"[Benchmark Runner] Capturing benchmarks into: {target_json} ...")

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
        f"--benchmark-json={target_json}",
    ]

    res = subprocess.run(cmd)
    if res.returncode != 0:
        print(f"[Benchmark Runner] Error: Benchmark execution failed with code {res.returncode}")
        return res.returncode

    # Create / update canonical latest copy
    try:
        shutil.copyfile(target_json, latest_json)
        print(f"[Benchmark Runner] Successfully updated canonical {latest_json}")
    except Exception as e:
        print(f"[Benchmark Runner] Note: Could not update latest copy: {e}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
