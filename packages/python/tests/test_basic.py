"""Basic initialization and import tests for Voyager OGM Python SDK."""

import voyager_ogm


def test_package_metadata():
    assert voyager_ogm.__version__.startswith("0.3.0")
