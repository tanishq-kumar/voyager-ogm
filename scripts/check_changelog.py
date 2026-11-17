#!/usr/bin/env python3
"""Changelog verification, extraction, and release automation.

Standard: Keep a Changelog 1.1.0 and Semantic Versioning 2.0.0.
Single source of truth for changelog format validation and release cutting.
"""

from __future__ import annotations

import argparse
import datetime
import re
import subprocess
import sys
from pathlib import Path

ALLOWED_HEADINGS = {
    "Added",
    "Changed",
    "Deprecated",
    "Removed",
    "Fixed",
    "Security",
}

VERSION_HEADING_RE = re.compile(
    r"^## \[([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?)\] - (\d{4}-\d{2}-\d{2})$"
)
UNRELEASED_HEADING_RE = re.compile(r"^## \[Unreleased\]$")
SECTION_HEADING_RE = re.compile(r"^### (.+)$")
LINK_REF_RE = re.compile(r"^\[([^\]]+)\]:\s+(https?://\S+)$")


def parse_semver(v: str) -> tuple[int, int, int, str]:
    """Parses a SemVer string into a comparable tuple: (major, minor, patch, prerelease)."""
    # Strips leading 'v' if passed
    if v.startswith("v"):
        v = v[1:]
    parts = v.split("-", 1)
    nums = parts[0].split(".")
    if len(nums) != 3:
        raise ValueError(f"Invalid SemVer: '{v}' (must have major.minor.patch)")
    try:
        major, minor, patch = int(nums[0]), int(nums[1]), int(nums[2])
    except ValueError as e:
        raise ValueError(f"Invalid SemVer numeric parts in '{v}'") from e

    prerelease = parts[1] if len(parts) > 1 else ""
    return (major, minor, patch, prerelease)


def semver_sort_key(v: str) -> tuple[int, int, int, int, str]:
    """Sort key for versions so newest/highest comes first in reverse sorting."""
    major, minor, patch, pre = parse_semver(v)
    # Releases without pre-release are higher than releases with pre-release
    # 0.4.0 > 0.4.0-rc.1 > 0.4.0-beta.1 > 0.4.0-alpha.6
    is_stable = 1 if not pre else 0
    return (major, minor, patch, is_stable, pre)


class ChangelogError(Exception):
    """Raised when changelog format or operation fails."""


def validate_format(changelog_path: Path) -> list[str]:
    """Validates that changelog strictly adheres to Keep a Changelog 1.1.0."""
    errors: list[str] = []
    if not changelog_path.exists():
        return [f"File not found: {changelog_path}"]

    content = changelog_path.read_text(encoding="utf-8")
    lines = content.splitlines()

    # Check header
    if not lines or lines[0].strip() != "# Changelog":
        errors.append("First line must be '# Changelog'")

    # Check preamble
    preamble_found = False
    for line in lines[:10]:
        if "Keep a Changelog" in line:
            preamble_found = True
            break
    if not preamble_found:
        errors.append("Missing preamble mentioning Keep a Changelog")

    # Check for disallowed horizontal rules
    for idx, line in enumerate(lines, 1):
        if line.strip() in ("---", "___", "***"):
            errors.append(f"Line {idx}: horizontal rule '{line.strip()}' is disallowed")

    # Parse version sections and links
    has_unreleased = False
    versions: list[tuple[str, str, int]] = []  # (version, date, line_no)
    link_refs: dict[str, str] = {}

    for idx, line in enumerate(lines, 1):
        line_str = line.strip()

        # Check link reference lines
        link_m = LINK_REF_RE.match(line_str)
        if link_m:
            lbl, url = link_m.group(1), link_m.group(2)
            link_refs[lbl] = url
            continue

        # Check Unreleased heading
        if UNRELEASED_HEADING_RE.match(line_str):
            if has_unreleased:
                errors.append(f"Line {idx}: duplicate '## [Unreleased]' heading")
            if versions:
                errors.append(
                    f"Line {idx}: '## [Unreleased]' must appear before all dated versions"
                )
            has_unreleased = True
            continue

        # Check dated version heading
        ver_m = VERSION_HEADING_RE.match(line_str)
        if ver_m:
            v_str, d_str = ver_m.group(1), ver_m.group(2)
            # Validate ISO date
            try:
                datetime.date.fromisoformat(d_str)
            except ValueError:
                errors.append(f"Line {idx}: invalid ISO date '{d_str}' in version heading")
            # Validate SemVer
            try:
                parse_semver(v_str)
            except ValueError as e:
                errors.append(f"Line {idx}: {e}")
            versions.append((v_str, d_str, idx))
            continue

        # Check if an invalid version heading was used (e.g. ## [0.4.0] without date)
        if line_str.startswith("## ") and not line_str.startswith("### "):
            errors.append(
                f"Line {idx}: invalid version heading '{line_str}'. "
                "Must be '## [Unreleased]' or '## [X.Y.Z] - YYYY-MM-DD'"
            )
            continue

        # Check section headings (### ...)
        sec_m = SECTION_HEADING_RE.match(line_str)
        if sec_m:
            heading = sec_m.group(1).strip()
            if heading not in ALLOWED_HEADINGS:
                errors.append(
                    f"Line {idx}: disallowed heading '### {heading}'. "
                    f"Allowed types: {', '.join(sorted(ALLOWED_HEADINGS))}"
                )

    if not has_unreleased:
        errors.append("Missing required '## [Unreleased]' section")

    # Validate versions are in descending order
    for i in range(len(versions) - 1):
        v_curr = versions[i][0]
        v_next = versions[i + 1][0]
        curr_key = semver_sort_key(v_curr)
        next_key = semver_sort_key(v_next)
        if curr_key <= next_key:
            errors.append(
                f"Versions not in descending order: '{v_curr}' (line {versions[i][2]}) "
                f"appears before '{v_next}' (line {versions[i + 1][2]})"
            )

    # Validate link references
    expected_links = ["Unreleased"] + [v[0] for v in versions]
    for exp in expected_links:
        if exp not in link_refs:
            errors.append(f"Missing link definition at bottom for '[{exp}]'")

    for ref in link_refs:
        if ref not in expected_links:
            errors.append(
                f"Extraneous link definition '[{ref}]' does not match any version heading"
            )

    return errors


def get_unreleased_line_range(changelog_path: Path) -> tuple[int, int]:
    """Returns (start_line, end_line) 1-indexed for the [Unreleased] section."""
    lines = changelog_path.read_text(encoding="utf-8").splitlines()
    start_line = None
    end_line = None

    for idx, line in enumerate(lines, 1):
        if UNRELEASED_HEADING_RE.match(line.strip()):
            start_line = idx
            continue
        if start_line is not None:
            # Reached next version heading or link block
            if line.strip().startswith("## [") or LINK_REF_RE.match(line.strip()):
                end_line = idx - 1
                break

    if start_line is None:
        raise ChangelogError("No '## [Unreleased]' heading found in changelog")
    if end_line is None:
        end_line = len(lines)

    return start_line, end_line


def require_unreleased_change(changelog_path: Path, base_ref: str) -> None:
    """Fails unless git diff against base_ref modifies lines inside [Unreleased]."""
    start_line, end_line = get_unreleased_line_range(changelog_path)

    # Determine diff between base_ref and working tree / HEAD
    repo_root = changelog_path.parent
    cmd = ["git", "diff", f"{base_ref}...HEAD", "--unified=0", "--", str(changelog_path.name)]
    proc = subprocess.run(cmd, cwd=repo_root, capture_output=True, text=True)
    if proc.returncode != 0 or not proc.stdout.strip():
        # Fall back to diffing against working tree if 3-dot is empty or fails
        cmd = ["git", "diff", base_ref, "--unified=0", "--", str(changelog_path.name)]
        proc = subprocess.run(cmd, cwd=repo_root, capture_output=True, text=True)
        if proc.returncode != 0:
            raise ChangelogError(f"git diff failed: {proc.stderr.strip()}")

    diff_output = proc.stdout
    if not diff_output.strip():
        raise ChangelogError(
            f"No changes to {changelog_path.name} detected relative to '{base_ref}'. "
            "Please add an entry under '## [Unreleased]' or apply the 'skip-changelog' label."
        )

    # Parse hunk headers: @@ -old_start,old_count +new_start,new_count @@
    hunk_re = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")
    touches_unreleased = False

    for line in diff_output.splitlines():
        m = hunk_re.match(line)
        if m:
            new_start = int(m.group(1))
            new_count = int(m.group(2)) if m.group(2) is not None else 1
            # Check for overlap between [new_start, new_start + new_count] and [start_line, end_line]
            hunk_end = new_start + max(new_count, 1) - 1
            if not (hunk_end < start_line or new_start > end_line):
                touches_unreleased = True
                break

    if not touches_unreleased:
        raise ChangelogError(
            f"Changes to {changelog_path.name} do not touch the '## [Unreleased]' section "
            f"(lines {start_line}-{end_line})."
        )


def extract_version(changelog_path: Path, target_version: str) -> str:
    """Extracts markdown body for a specific version heading."""
    lines = changelog_path.read_text(encoding="utf-8").splitlines()
    in_section = False
    section_lines: list[str] = []

    for line in lines:
        stripped = line.strip()
        # Heading checks
        if target_version.lower() == "unreleased":
            if UNRELEASED_HEADING_RE.match(stripped):
                in_section = True
                continue
        else:
            ver_m = VERSION_HEADING_RE.match(stripped)
            if ver_m and ver_m.group(1) == target_version:
                in_section = True
                continue

        if in_section:
            # If hit next version heading or link references, stop
            if stripped.startswith("## [") or LINK_REF_RE.match(stripped):
                break
            section_lines.append(line)

    if not in_section:
        raise ChangelogError(f"Version '{target_version}' not found in {changelog_path.name}")

    return "\n".join(section_lines).strip()


def release_version(changelog_path: Path, version: str, release_date: str) -> None:
    """Rewrites [Unreleased] into a dated version section and updates link references."""
    # Validate inputs
    parse_semver(version)
    datetime.date.fromisoformat(release_date)

    content = changelog_path.read_text(encoding="utf-8")
    lines = content.splitlines()

    # Find unreleased range
    unreleased_idx = None
    first_version_idx = None
    prev_version: str | None = None
    link_start_idx = None

    for idx, line in enumerate(lines):
        stripped = line.strip()
        if UNRELEASED_HEADING_RE.match(stripped):
            unreleased_idx = idx
            continue
        if unreleased_idx is not None and first_version_idx is None:
            ver_m = VERSION_HEADING_RE.match(stripped)
            if ver_m:
                first_version_idx = idx
                prev_version = ver_m.group(1)
                continue
        if LINK_REF_RE.match(stripped):
            if link_start_idx is None:
                link_start_idx = idx

    if unreleased_idx is None:
        raise ChangelogError("No '## [Unreleased]' heading found")

    # If there are no existing versions, link_start_idx might be after unreleased
    split_idx = first_version_idx if first_version_idx is not None else link_start_idx
    if split_idx is None:
        split_idx = len(lines)

    # Everything between unreleased_idx + 1 and split_idx is the current Unreleased content
    unreleased_body = lines[unreleased_idx + 1 : split_idx]

    # New dated section
    new_version_heading = f"## [{version}] - {release_date}"
    new_unreleased_header = ["## [Unreleased]", ""]

    new_middle = new_unreleased_header + [new_version_heading] + unreleased_body

    before = lines[:unreleased_idx]
    after = lines[split_idx:link_start_idx] if link_start_idx is not None else lines[split_idx:]
    links = lines[link_start_idx:] if link_start_idx is not None else []

    # Update link block
    updated_links: list[str] = []
    github_base = "https://github.com/tanishq-kumar/voyager-ogm"
    inserted_version_link = False

    for l_line in links:
        m = LINK_REF_RE.match(l_line.strip())
        if m:
            lbl = m.group(1)
            if lbl == "Unreleased":
                updated_links.append(f"[Unreleased]: {github_base}/compare/v{version}...HEAD")
                # Insert the new version link right after Unreleased
                if prev_version:
                    updated_links.append(
                        f"[{version}]: {github_base}/compare/v{prev_version}...v{version}"
                    )
                else:
                    updated_links.append(f"[{version}]: {github_base}/releases/tag/v{version}")
                inserted_version_link = True
                continue
            if lbl == version:
                continue  # Avoid duplicate if re-running
            updated_links.append(l_line)
        else:
            if l_line.strip():
                updated_links.append(l_line)

    if not inserted_version_link:
        updated_links.insert(0, f"[Unreleased]: {github_base}/compare/v{version}...HEAD")
        if prev_version:
            updated_links.insert(
                1, f"[{version}]: {github_base}/compare/v{prev_version}...v{version}"
            )
        else:
            updated_links.insert(1, f"[{version}]: {github_base}/releases/tag/v{version}")

    new_lines = before + new_middle + after + [""] + updated_links
    new_content = "\n".join(new_lines).rstrip() + "\n"
    changelog_path.write_text(new_content, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--path",
        type=Path,
        default=Path("CHANGELOG.md"),
        help="Path to CHANGELOG.md (default: CHANGELOG.md)",
    )
    parser.add_argument("--format", action="store_true", help="Validate Keep a Changelog structure")
    parser.add_argument(
        "--require-unreleased-change",
        metavar="BASE_REF",
        help="Verify diff against base-ref touches lines within [Unreleased]",
    )
    parser.add_argument(
        "--extract",
        metavar="VERSION",
        help="Extract and print markdown notes for a specific version",
    )
    parser.add_argument(
        "--release",
        nargs=2,
        metavar=("VERSION", "DATE"),
        help="Cut [Unreleased] into a dated version and update link references",
    )

    args = parser.parse_args()

    # Find repo root if path is relative
    path = args.path
    if not path.is_absolute() and not path.exists():
        # Try finding relative to script dir
        candidate = Path(__file__).resolve().parent.parent / path
        if candidate.exists():
            path = candidate

    try:
        if args.format:
            errors = validate_format(path)
            if errors:
                print(
                    f"[FAIL] {path.name} formatting errors found ({len(errors)}):", file=sys.stderr
                )
                for err in errors:
                    print(f"  • {err}", file=sys.stderr)
                return 1
            print(f"[PASS] {path.name} adheres to Keep a Changelog 1.1.0.")
            return 0

        if args.require_unreleased_change:
            require_unreleased_change(path, args.require_unreleased_change)
            print(
                f"[PASS] {path.name} [Unreleased] updated relative to {args.require_unreleased_change}."
            )
            return 0

        if args.extract:
            notes = extract_version(path, args.extract)
            print(notes)
            return 0

        if args.release:
            version, release_date = args.release[0], args.release[1]
            release_version(path, version, release_date)
            print(f"[OK] Released version [{version}] ({release_date}) in {path.name}.")
            return 0

        parser.print_help()
        return 1

    except ChangelogError as e:
        print(f"[FAIL] {e}", file=sys.stderr)
        return 1
    except Exception as e:
        print(f"[ERROR] {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
