#!/usr/bin/env python3
"""Fetches official openCypher TCK feature files from the official repository."""

from __future__ import annotations

import json
import os
import sys
import urllib.request

GITHUB_API_BASE = "https://api.github.com/repos/opencypher/openCypher/contents/tck/features"
RAW_BASE = "https://raw.githubusercontent.com/opencypher/openCypher/main/tck/features"

# Target clauses to download for comprehensive conformance
TARGET_CLAUSES = [
    "clauses/match",
    "clauses/match-where",
    "clauses/create",
    "clauses/merge",
    "clauses/delete",
    "clauses/set",
    "clauses/remove",
    "clauses/return",
    "clauses/return-orderby",
    "clauses/return-skip-limit",
    "clauses/unwind",
    "clauses/with",
]


def fetch_json(url: str) -> list[dict[str, str]]:
    req = urllib.request.Request(url, headers={"User-Agent": "Voyager-TCK-Downloader/1.0"})
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read().decode("utf-8"))


def fetch_file(raw_url: str, dest_path: str) -> None:
    req = urllib.request.Request(raw_url, headers={"User-Agent": "Voyager-TCK-Downloader/1.0"})
    with urllib.request.urlopen(req) as resp:
        content = resp.read()
    os.makedirs(os.path.dirname(dest_path), exist_ok=True)
    with open(dest_path, "wb") as f:
        f.write(content)


def main() -> None:
    base_dir = os.path.dirname(os.path.abspath(__file__))
    out_dir = os.path.join(base_dir, "opencypher")
    os.makedirs(out_dir, exist_ok=True)

    print(f"Downloading official openCypher TCK features to: {out_dir}")
    total_files = 0

    for clause in TARGET_CLAUSES:
        clause_api_url = f"{GITHUB_API_BASE}/{clause}"
        try:
            items = fetch_json(clause_api_url)
        except Exception as e:
            print(f"Failed to list {clause}: {e}", file=sys.stderr)
            continue

        for item in items:
            name = item.get("name", "")
            if name.endswith(".feature"):
                rel_path = f"{clause}/{name}"
                raw_url = f"{RAW_BASE}/{rel_path}"
                dest = os.path.join(out_dir, clause.replace("/", os.sep), name)
                print(f"  -> Downloading {rel_path}...")
                try:
                    fetch_file(raw_url, dest)
                    total_files += 1
                except Exception as e:
                    print(f"     Failed {rel_path}: {e}", file=sys.stderr)

    print(f"\nSuccessfully downloaded {total_files} official openCypher TCK feature files!")


if __name__ == "__main__":
    main()
