"""Tests for scripts/check_changelog.py (Keep a Changelog validator and release helper)."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

# Add scripts directory to path to import check_changelog directly
scripts_dir = Path(__file__).resolve().parents[3] / "scripts"
sys.path.insert(0, str(scripts_dir))

import check_changelog  # noqa: E402

SAMPLE_VALID_CHANGELOG = """# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- New feature A.

## [0.4.0] - 2026-10-15

### Added
- Feature B.

### Fixed
- Bug fix C.

## [0.3.0] - 2026-09-01

### Added
- Initial implementation.

[Unreleased]: https://github.com/tanishq-kumar/voyager/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/tanishq-kumar/voyager/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/tanishq-kumar/voyager/releases/tag/v0.3.0
"""


def test_validate_format_valid(tmp_path: Path) -> None:
    path = tmp_path / "CHANGELOG.md"
    path.write_text(SAMPLE_VALID_CHANGELOG, encoding="utf-8")
    errors = check_changelog.validate_format(path)
    assert not errors, f"Expected no errors, got: {errors}"


def test_validate_format_missing_header(tmp_path: Path) -> None:
    bad = SAMPLE_VALID_CHANGELOG.replace("# Changelog", "# Release Notes")
    path = tmp_path / "CHANGELOG.md"
    path.write_text(bad, encoding="utf-8")
    errors = check_changelog.validate_format(path)
    assert any("First line must be '# Changelog'" in e for e in errors)


def test_validate_format_missing_preamble(tmp_path: Path) -> None:
    bad = SAMPLE_VALID_CHANGELOG.replace("Keep a Changelog", "Changelog guidelines")
    path = tmp_path / "CHANGELOG.md"
    path.write_text(bad, encoding="utf-8")
    errors = check_changelog.validate_format(path)
    assert any("Missing preamble" in e for e in errors)


def test_validate_format_horizontal_rules(tmp_path: Path) -> None:
    bad = SAMPLE_VALID_CHANGELOG.replace(
        "## [0.3.0] - 2026-09-01", "---\n\n## [0.3.0] - 2026-09-01"
    )
    path = tmp_path / "CHANGELOG.md"
    path.write_text(bad, encoding="utf-8")
    errors = check_changelog.validate_format(path)
    assert any("horizontal rule" in e for e in errors)


def test_validate_format_disallowed_heading(tmp_path: Path) -> None:
    bad = SAMPLE_VALID_CHANGELOG.replace("### Added", "### Removed (Breaking Changes)", 1)
    path = tmp_path / "CHANGELOG.md"
    path.write_text(bad, encoding="utf-8")
    errors = check_changelog.validate_format(path)
    assert any("disallowed heading '### Removed (Breaking Changes)'" in e for e in errors)


def test_validate_format_invalid_version_order(tmp_path: Path) -> None:
    bad = SAMPLE_VALID_CHANGELOG.replace("## [0.4.0] - 2026-10-15", "## [0.2.0] - 2026-10-15")
    path = tmp_path / "CHANGELOG.md"
    path.write_text(bad, encoding="utf-8")
    errors = check_changelog.validate_format(path)
    assert any("Versions not in descending order" in e for e in errors)


def test_validate_format_missing_unreleased(tmp_path: Path) -> None:
    bad = SAMPLE_VALID_CHANGELOG.replace("## [Unreleased]\n\n### Added\n- New feature A.\n\n", "")
    bad = bad.replace(
        "[Unreleased]: https://github.com/tanishq-kumar/voyager/compare/v0.4.0...HEAD\n", ""
    )
    path = tmp_path / "CHANGELOG.md"
    path.write_text(bad, encoding="utf-8")
    errors = check_changelog.validate_format(path)
    assert any("Missing required '## [Unreleased]' section" in e for e in errors)


def test_validate_format_missing_link_def(tmp_path: Path) -> None:
    bad = SAMPLE_VALID_CHANGELOG.replace(
        "[0.4.0]: https://github.com/tanishq-kumar/voyager/compare/v0.3.0...v0.4.0\n",
        "",
    )
    path = tmp_path / "CHANGELOG.md"
    path.write_text(bad, encoding="utf-8")
    errors = check_changelog.validate_format(path)
    assert any("Missing link definition at bottom for '[0.4.0]'" in e for e in errors)


def test_extract_version(tmp_path: Path) -> None:
    path = tmp_path / "CHANGELOG.md"
    path.write_text(SAMPLE_VALID_CHANGELOG, encoding="utf-8")

    unreleased = check_changelog.extract_version(path, "Unreleased")
    assert "### Added" in unreleased
    assert "New feature A." in unreleased
    assert "Feature B." not in unreleased

    v040 = check_changelog.extract_version(path, "0.4.0")
    assert "### Added\n- Feature B." in v040
    assert "### Fixed\n- Bug fix C." in v040
    assert "New feature A." not in v040

    with pytest.raises(check_changelog.ChangelogError, match="not found"):
        check_changelog.extract_version(path, "9.9.9")


def test_release_version(tmp_path: Path) -> None:
    path = tmp_path / "CHANGELOG.md"
    path.write_text(SAMPLE_VALID_CHANGELOG, encoding="utf-8")

    check_changelog.release_version(path, "0.5.0", "2026-11-20")

    # Verify that format check passes on the newly released changelog
    errors = check_changelog.validate_format(path)
    assert not errors, f"Released changelog has format errors: {errors}"

    content = path.read_text(encoding="utf-8")
    assert "## [Unreleased]\n\n## [0.5.0] - 2026-11-20" in content
    assert "[Unreleased]: https://github.com/tanishq-kumar/voyager/compare/v0.5.0...HEAD" in content
    assert "[0.5.0]: https://github.com/tanishq-kumar/voyager/compare/v0.4.0...v0.5.0" in content

    # New unreleased section is empty
    notes_050 = check_changelog.extract_version(path, "0.5.0")
    assert "New feature A." in notes_050

    notes_unreleased = check_changelog.extract_version(path, "Unreleased")
    assert notes_unreleased == ""


def test_semver_parsing() -> None:
    assert check_changelog.parse_semver("0.4.0") == (0, 4, 0, "")
    assert check_changelog.parse_semver("0.4.0-alpha.6") == (0, 4, 0, "alpha.6")
    assert check_changelog.parse_semver("v1.2.3-rc.1") == (1, 2, 3, "rc.1")

    with pytest.raises(ValueError):
        check_changelog.parse_semver("0.4")
