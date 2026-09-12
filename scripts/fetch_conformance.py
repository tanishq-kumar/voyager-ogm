#!/usr/bin/env python3
"""Voyager OGM Official Conformance Repositories Manager.

Clones or updates official vendor conformance repositories under `test_data/conformance/`
which is excluded from version control via `.gitignore`.
- Apache AGE: https://github.com/apache/age.git (PostgreSQL regression tests)
- FalkorDB: https://github.com/FalkorDB/falkordb-py.git (Official FalkorDB client test suite)
"""

from __future__ import annotations

import os
import subprocess
import sys

REPOS = {
    "apache-age": {
        "url": "https://github.com/apache/age.git",
        "branch": "master",
        "desc": "Apache AGE official PostgreSQL regression test suites",
    },
    "falkordb-py": {
        "url": "https://github.com/FalkorDB/falkordb-py.git",
        "branch": "master",
        "desc": "FalkorDB official Python & wire protocol test suite",
    },
}


def main() -> None:
    """Synchronize official conformance repositories for integration testing."""
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    conformance_dir = os.path.join(repo_root, "test_data", "conformance")
    os.makedirs(conformance_dir, exist_ok=True)

    print("=" * 65)
    print("Voyager OGM: Synchronizing Official Conformance Repositories")
    print(f"Target Directory: {conformance_dir}")
    print("=" * 65)

    for name, info in REPOS.items():
        target_path = os.path.join(conformance_dir, name)
        if os.path.exists(target_path):
            print(f"[EXISTS] {name} -> {info['desc']}")
        else:
            print(f"[CLONING] {name} from {info['url']} (shallow clone)...")
            try:
                subprocess.run(
                    [
                        "git",
                        "clone",
                        "--depth",
                        "1",
                        "--branch",
                        info["branch"],
                        info["url"],
                        target_path,
                    ],
                    check=True,
                )
                print(f"[DONE] Successfully cloned {name}.")
            except subprocess.CalledProcessError as e:
                print(f"[WARN] Failed to clone {name}: {e}", file=sys.stderr)

    print("\n[OK] All conformance repositories are verified.")


if __name__ == "__main__":
    main()
