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
  - Pre-releases are marked as **Pre-release** on GitHub Releases and are published only to TestPyPI (see section 6), never to PyPI or crates.io. This clearly demarcates upcoming development builds from production-ready releases.
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

## 6. Registry Publishing

Releases are published with GitHub Actions OpenID Connect (OIDC) trusted publishing, so no registry API token is stored in the repository or in GitHub secrets. Every publish job runs only after the GitHub Release has been created and is gated by a repository variable, so a registry can be enabled after its trusted publisher exists.

### 6.1 GitHub Environments

Create these environments under **Settings → Environments**:

| Environment | Registry | URL |
| --- | --- | --- |
| `testpypi` | TestPyPI | `https://test.pypi.org/p/voyager-ogm` |
| `pypi` | PyPI | `https://pypi.org/p/voyager-ogm` |
| `crates-io` | crates.io | `https://crates.io/crates/voyager-ogm` |

Add the maintainer (`tanishq-kumar`) as a **required reviewer** on each environment so every registry publish waits for manual approval. The environment name is part of the OIDC claim and must match the trusted-publisher configuration exactly.

### 6.2 Repository Variables

Set these under **Settings → Secrets and variables → Actions → Variables**:

| Variable | Set to `true` when | Published jobs |
| --- | --- | --- |
| `TESTPYPI_PUBLISH` | The TestPyPI pending publisher is configured | TestPyPI on every tag (pre-releases included) |
| `PYPI_PUBLISH` | The PyPI pending publisher is configured and a stable release is ready | PyPI on stable tags only |
| `CRATES_PUBLISH` | Every crate has had its first manual publish and the trusted publishers are configured | crates.io on stable tags only |

A pre-release tag (`vX.Y.Z-alpha.N`, `-beta.N`, `-rc.N`) publishes to TestPyPI only. A stable tag (`vX.Y.Z`) publishes to TestPyPI and, when enabled, to PyPI and crates.io.

### 6.3 Trusted Publishers

Each registry trusts this repository's `release.yml` workflow running in the matching environment (`pypi`, `testpypi`, `crates-io`). Renaming the workflow file or an environment, or moving the repository, requires updating the trusted publisher on every registry. crates.io trusted publishers are configured per crate, and a crate must exist before its publisher can be added, so a **new crate needs one manual first publish** (`cargo publish -p <crate>` with a personal token) before CI can publish it. `voyager-pyo3` is not published as a Rust crate: it ships as the Python wheel and sdist (`publish = false`).

crates.io versions are **permanent**: a version can be yanked (`cargo yank --version <version> -p <crate>`) but never deleted or reused.

### 6.4 Re-runs and Scope

- PyPI and TestPyPI use `skip-existing: true`, so a re-run does not fail on files already uploaded.
- The crates.io job queries `https://crates.io/api/v1/crates/<crate>/<version>` before each publish and skips versions that already exist.
- Pre-releases stop at TestPyPI; PyPI and crates.io receive stable tags only.
- npm is not published yet.

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
4. Push the branch, open a pull request to `main` with the labels `full-ci` and `skip-changelog` (the release commit moves entries out of `[Unreleased]`, which the changelog gate would otherwise reject), and merge it once CI is green.
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
7. The `.github/workflows/release.yml` workflow triggers on the tag push, compiles multi-platform wheels and sdist, creates the GitHub Release with the extracted changelog notes, publishes to TestPyPI (if `TESTPYPI_PUBLISH == 'true'`), and on stable tags to PyPI (if `PYPI_PUBLISH == 'true'`) and crates.io (if `CRATES_PUBLISH == 'true'`). See section 6 for the one-time environment and trusted-publisher setup.
   - Linux `x86_64` and `aarch64` wheels are built inside the `manylinux_2_28` container (glibc >= 2.28) so both targets share a portable tag. The workflow fails before creating the release if a Linux wheel filename does not carry `manylinux_2_28`.
   - Each Linux `x86_64` wheel is installed into a fresh Python 3.11 and 3.12 environment without optional extras and exercised with `scripts/smoke_base_install.py` before the release is published.
