#!/usr/bin/env python3
"""Fetches official openGQL TCK feature files from the official repository.

Downloads only the Gherkin .feature specification files directly from
https://github.com/opengql/tck without cloning the full repository or
introducing nested git submodules/third-party artifacts.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.request

GITHUB_TREE_API = "https://api.github.com/repos/opengql/tck/git/trees/main?recursive=1"
RAW_BASE = "https://raw.githubusercontent.com/opengql/tck/main"

# Canonical feature list fallback if GitHub API rate limit is exceeded
FALLBACK_FEATURES = [
    "features/Debug.feature",
    "features/expressions/aggregation/Aggregation1.feature",
    "features/expressions/aggregation/Aggregation2.feature",
    "features/expressions/aggregation/Aggregation3.feature",
    "features/expressions/boolean/Boolean1.feature",
    "features/expressions/boolean/Boolean2.feature",
    "features/expressions/boolean/Boolean3.feature",
    "features/expressions/boolean/Boolean4.feature",
    "features/expressions/boolean/Boolean5.feature",
    "features/statements/catalog-modifying/create/graph-types/Create1.feature",
    "features/statements/catalog-modifying/create/graph-types/Create2.feature",
    "features/statements/catalog-modifying/create/graphs/Create2.feature",
    "features/statements/catalog-modifying/create/schemas/Create1.feature",
    "features/statements/catalog-modifying/drop/drop1.feature",
]


def fetch_file(raw_url: str, dest_path: str) -> None:
    req = urllib.request.Request(raw_url, headers={"User-Agent": "Voyager-GQL-TCK-Downloader/1.0"})
    with urllib.request.urlopen(req) as resp:
        content = resp.read()
    os.makedirs(os.path.dirname(dest_path), exist_ok=True)
    with open(dest_path, "wb") as f:
        f.write(content)


def discover_features() -> list[str]:
    """Discovers feature files via GitHub API, falling back to canonical list."""
    try:
        req = urllib.request.Request(
            GITHUB_TREE_API, headers={"User-Agent": "Voyager-GQL-TCK-Downloader/1.0"}
        )
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            tree = data.get("tree", [])
            features = [
                item["path"]
                for item in tree
                if item.get("path", "").startswith("features/")
                and item.get("path", "").endswith(".feature")
            ]
            if features:
                return sorted(features)
    except Exception as e:
        print(
            f"Warning: GitHub API tree discovery unavailable ({e}). Using canonical manifest.",
            file=sys.stderr,
        )

    return FALLBACK_FEATURES


def main() -> None:
    base_dir = os.path.dirname(os.path.abspath(__file__))
    out_dir = os.path.join(base_dir, "opengql")
    os.makedirs(out_dir, exist_ok=True)

    print(f"Downloading official openGQL TCK features to: {out_dir}")
    features = discover_features()
    total_files = 0

    for rel_path in features:
        raw_url = f"{RAW_BASE}/{rel_path}"
        dest = os.path.join(out_dir, rel_path.replace("/", os.sep))
        print(f"  -> Downloading {rel_path}...")
        try:
            fetch_file(raw_url, dest)
            total_files += 1
        except Exception as e:
            print(f"     Failed {rel_path}: {e}", file=sys.stderr)

    print(f"\nSuccessfully synced {total_files} official openGQL TCK feature files!")


if __name__ == "__main__":
    main()
