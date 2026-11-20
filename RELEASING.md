# Release Standard and Process

This document defines the release standard, versioning conventions, branching model, and automated release procedure for Voyager OGM.

---

## 1. Release Philosophy and `main` Branch Semantics

- **`main` is always the UPCOMING release.**
- Every user-facing pull request must add an entry under `## [Unreleased]` in `CHANGELOG.md` following [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/).
- Internal-only pull requests (CI changes, internal refactoring, dependency updates via Dependabot) must carry the `skip-changelog` GitHub label and are exempt from changelog additions.
- The `changelog` job in CI enforces that either `CHANGELOG.md` is modified under `## [Unreleased]` or the `skip-changelog` label is present on the PR.

---

## 2. Versioning Specification (SemVer Pre-1.0)

Voyager OGM follows [Semantic Versioning 2.0.0](https://semver.org/), with standard pre-1.0 conventions:

- **Breaking changes**: Bump the **minor** version (`0.X.0`).
- **New features / capabilities**: Bump the **minor** version (`0.X.0`).
- **Bug fixes / patches / hardening**: Bump the **patch** version (`0.X.Y`).

### Pre-Releases vs. Stable Releases

- **Pre-releases**: Tagged as `vX.Y.Z-alpha.N`, `vX.Y.Z-beta.N`, or `vX.Y.Z-rc.N` in Cargo. Maturin automatically maps these to PEP 440 formats on PyPI (`X.Y.ZaN`, `X.Y.ZbN`, `X.Y.ZrcN`).
  - Pre-releases are marked as **Pre-release** on GitHub Releases.
  - On PyPI, pre-releases are only installed when users pass `--pre` to `pip` or configure appropriate prerelease settings in package managers. This clearly demarcates upcoming development builds from production-ready releases.
- **Stable releases**: Plain tags without suffixes (`vX.Y.Z`). These become GitHub "Latest" releases and the default pip installs.

---

## 3. Single Source of Truth for Versioning

- The canonical version is declared once in workspace root `Cargo.toml` (`[workspace.package] version = "..."`).
- `pyproject.toml` uses `dynamic = ["version"]`. Maturin derives the Python wheel/sdist version directly from `Cargo.toml`.
- Version bumps must never edit `pyproject.toml` manually.

---

## 4. Maintenance and Backport Branches

- A maintenance branch `release/X.Y` is cut from the `vX.Y.Z` tag **only** when an older stable release line requires a critical fix.
- All fixes land on `main` first and are cherry-picked to the maintenance branch using `git cherry-pick -x <sha>`.
- The patch release is tagged directly from that maintenance branch.

---

## 5. Tagging Discipline

- Tags must always be annotated: `vX.Y.Z[-pre.N]`.
- Tags are created **only** via `just release-tag <version>`, never manually, and only on `main` after the release PR is merged.
- Tags are pushed by a human maintainer (`git push origin vX.Y.Z`). Automation never tags or pushes commits autonomously.

---

## 6. PyPI Trusted Publishing Setup

Release wheels and source distributions are published to PyPI using GitHub Actions OpenID Connect (OIDC) trusted publishing:

- **Environment**: `pypi`
- **Permissions**: `id-token: write`
- **Prerequisite**: The repository owner must configure Voyager OGM as a trusted publisher in the PyPI administrative portal (linking repo `tanishq-kumar/voyager`, workflow `.github/workflows/release.yml`, and environment `pypi`). Note: Following the repository rename, the trusted publisher configuration on PyPI must be updated to reference `tanishq-kumar/voyager` (not `voyager-ogm`) for GitHub Actions OIDC authentication to succeed.
- **Opt-in control**: The publish job runs only when repository variable `PYPI_PUBLISH == 'true'`. Until enabled, release artifacts (sdist and wheels) are built, verified, and attached exclusively to the GitHub Release.

---

## 7. Release Checklist

`main` is protected, so a release goes through a pull request and the tag is created on the merge commit.

1. Ensure the `main` branch CI is completely green.
2. From an up-to-date `main`, create a release branch in a clean checkout (a fresh `git worktree` avoids untracked local files):
   ```bash
   git switch -c chore/release-<version> origin/main
   ```
3. Run the preparation recipe:
   ```bash
   just release-prepare <version>      # e.g. just release-prepare 0.4.0-alpha.7
   ```
   This recipe:
   - Refuses to run on `main`, on a dirty working tree, or if the tag already exists.
   - Runs full CI verification (`just ci`).
   - Updates `[workspace.package].version` (and internal path-dependency versions) in `Cargo.toml`.
   - Runs `cargo update -w` and `uv lock` to update lockfiles.
   - Runs `python3 scripts/check_changelog.py --release <version> <date>` to convert `[Unreleased]` to a dated section and add link definitions.
   - Creates the commit `chore(release): v<version>`.
4. Push the branch, open a pull request to `main` (label `full-ci`), and merge it once CI is green.
5. Update your local `main` and create the tag:
   ```bash
   git switch main && git pull --ff-only
   just release-tag <version>
   ```
   `release-tag` checks that local `main` equals `origin/main`, that `Cargo.toml` carries `<version>` and that `CHANGELOG.md` has its section, then creates the annotated tag `v<version>` and prints the push command.
6. Push the tag:
   ```bash
   git push origin v<version>
   ```
7. The `.github/workflows/release.yml` workflow triggers on the tag push, compiles multi-platform wheels and sdist, creates the GitHub Release with the extracted changelog notes, and publishes to PyPI (if `PYPI_PUBLISH == 'true'`).
