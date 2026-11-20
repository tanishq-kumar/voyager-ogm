"""create_bridge() accepts the same optional URI namespace prefixes as the native parser."""

from __future__ import annotations

import pytest
from voyager_ogm import create_bridge
from voyager_ogm.bridge import MockBridge, _strip_uri_namespace_prefix


@pytest.mark.parametrize(
    "prefix",
    ["voyager+native:", "voyager+bridge:", "voyager-drivers:", "voyager:", "voy:", "vn:"],
)
def test_namespace_prefix_is_removed(prefix: str) -> None:
    assert _strip_uri_namespace_prefix(f"{prefix}bolt://localhost:7687") == "bolt://localhost:7687"


def test_uri_without_prefix_is_unchanged() -> None:
    assert _strip_uri_namespace_prefix("  bolt://localhost:7687 ") == "bolt://localhost:7687"


@pytest.mark.parametrize("prefix", ["voy:", "vn:", "voyager:"])
def test_create_bridge_accepts_prefixed_uri(prefix: str) -> None:
    bridge = create_bridge(f"{prefix}mock://?graph=prefixed_graph")
    assert isinstance(bridge, MockBridge)
    assert bridge.graph_name == "prefixed_graph"
