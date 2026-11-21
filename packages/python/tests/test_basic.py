"""Basic initialization and import tests for Voyager OGM Python SDK."""

import tomllib
from pathlib import Path

import voyager_ogm

REPO_ROOT = Path(__file__).resolve().parents[3]


def test_package_metadata():
    """The Python package reports the workspace version from Cargo.toml."""
    cargo = tomllib.loads((REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    assert voyager_ogm.__version__ == cargo["workspace"]["package"]["version"]
