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
  - Pre-releases are marked as **Pre-release** on GitHub Releases and are published to TestPyPI and crates.io (see section 6), never to PyPI. Cargo only resolves a pre-release when its exact version is requested, so crates.io pre-releases do not reach users of stable requirements. This clearly demarcates upcoming development builds from production-ready releases.
- **Stable releases**: Plain tags without suffixes (`vX.Y.Z`). These become GitHub "Latest" releases and the default pip installs.

---

## 3. Single Source of Truth for Versioning

- The canonical version is declared once in workspace root `Cargo.toml` (`[workspace.package] version = "..."`).
- `pyproject.toml` uses `dynamic = ["version"]`. Maturin derives the Python wheel/sdist version directly from `Cargo.toml`.
- Version bumps must never edit `pyproject.toml` manually.

---

## 4. Maintenance Branches (`release/X.Y`) and Backports

`main` is always the upcoming release. A maintenance branch `release/X.Y` keeps an already-shipped release line patchable after `main` has moved on to the next minor version.

### 4.1 When to cut `release/X.Y`

Cut `release/X.Y` the first time a bug fix must ship for an already-released `X.Y` line after `main` has started `X.(Y+1)` work. For example, once 0.5.0 development is on `main`, a fix that must reach 0.4 users needs `release/0.4`. A release line that is still the newest line on `main` does not need a maintenance branch: prepare that release from `main` as described in section 7. Use one branch per minor line (`release/0.4` serves 0.4.1, 0.4.2, and so on) and never reuse it for another line.

A security fix always qualifies (section 4.2), so a security fix for an older line is also a reason to cut its maintenance branch.

### 4.2 What gets backported (bug vs. feature)

A change qualifies for a patch release on `release/X.Y` only when all of these hold:

1. **It fixes something broken.** Documented or clearly intended behaviour does not work as described, the library produces wrong output, crashes, loses data, or has a security problem.
2. **It adds no public API and does not change behaviour that working code relies on.** No new functions, parameters, extras, or Cargo features, and code that works today keeps working the same way.
3. **It is small and safe to backport.** No large refactor and no new dependency.

Security fixes always qualify, even when the patch is larger than usual. Anything that adds capability, changes documented behaviour, or improves behaviour that is not broken (performance, error wording, stricter validation) waits for the next minor release on `main`. When in doubt, treat it as a feature and do not backport it.

Examples:
- **Bug, backport it:** the API accepts a construct but compiles it to the wrong query (for example a relationship direction or a `MERGE` key is emitted incorrectly). Intended behaviour is documented and currently produces wrong output.
- **Feature, do not backport it:** the API does not offer the construct at all, such as a new builder method, a new keyword parameter, a new optional extra, or a new Cargo feature.
- **Security fix, always backport it:** a crafted label or relationship type is interpolated into an emitted query instead of being validated, so it can inject query text or escape an identifier.
- **Feature, do not backport it:** adding TLS support or another connection capability. It is new capability, not a repair of documented behaviour.

### 4.3 Cutting and protecting the branch

Cut the branch from the `vX.Y.0` tag, not from the current branch tip: the tag is the exact released state of the line, including its `CHANGELOG.md` section and version. `just release-branch X.Y` prints the commands and does not run them:

```bash
just release-branch 0.4
git fetch origin --tags
git switch -c release/0.4 v0.4.0
git push -u origin release/0.4
```

Protect `release/X.Y` with the same branch protection / ruleset as `main` (required pull requests and the `CI Gate` status check). This is a maintainer action in the repository settings. The release workflow independently refuses to publish a tag that is not reachable from `origin/main` or the matching `origin/release/X.Y`, but protection keeps every change on the line reviewed and tested.

### 4.4 Backport flow

1. **Fix on `main` first.** Land the fix through the normal pull request to `main`, including its `## [Unreleased]` changelog entry. Fixes never start on a maintenance branch.
2. **Open the backport PR to `release/X.Y`.** Branch off `release/X.Y`, cherry-pick the merged fix with its provenance (`git cherry-pick -x <sha>...` for the PR's commits, or `git cherry-pick -x -m 1 <merge-sha>` for the PR's merge commit on `main`), and resolve the expected `CHANGELOG.md` conflict by putting the entry under the branch's own `## [Unreleased]`. Open the PR against `release/X.Y` with the labels `full-ci` and `skip-changelog`.
3. **Mark the original PR or issue.** Add the `backport-X.Y` label (for example `backport-0.4`) to the PR or issue whose fix should also ship on that line. The convention is `backport-<major>.<minor>`, one label per maintained line; create the label the first time it is needed (section 4.6).
4. **Prepare the patch release.** Create `chore/release-X.Y.N` off `release/X.Y` and run:
   ```bash
   git switch -c chore/release-X.Y.N origin/release/X.Y
   just release-prepare X.Y.N
   ```
   `release-prepare` accepts only versions greater than the workspace version. On a branch based on `release/X.Y` it accepts only `X.Y.N` (a patch on that line); on a branch based on `main` it accepts any greater version. The base is detected from the branch's upstream tracking ref, then from the closest `git merge-base` among `origin/main` and `origin/release/*`. If detection ties, pass it explicitly: `just release-prepare X.Y.N base=release/X.Y`.
5. **Merge the release PR into `release/X.Y`** with the labels `full-ci` and `skip-changelog` (the release commit moves entries out of `[Unreleased]`).
6. **Tag on `release/X.Y`.** Update the branch, then run the tag recipe:
   ```bash
   git switch release/X.Y && git pull --ff-only
   just release-tag X.Y.N
   git push origin vX.Y.N
   ```
   `release-tag` allows `main` or `release/X.Y` (where `<version>` must be `X.Y.N`) and keeps every safety check: a clean tracked tree, `HEAD` equal to `origin/<branch>`, the matching `Cargo.toml` version, an existing `CHANGELOG.md` section, and no existing tag.
7. **Let the release workflow run.** It verifies the tag is reachable from `main` or the matching release branch, publishes the artifacts, and only marks the release **Latest** when the tag is the highest `v*` version (a 0.4.2 patch never replaces a newer 0.5.0).

### 4.5 How `CHANGELOG.md` works across release lines

- The fix keeps its entry under `main`'s `## [Unreleased]`. It is still part of the next minor release, so do not delete it when backporting.
- On `release/X.Y`, the cherry-picked entry goes under that branch's `## [Unreleased]`. `just release-prepare X.Y.N` cuts that section into `## [X.Y.N] - <date>`, exactly like a normal release.
- After the patch is published, open a small PR to `main` that adds the same `## [X.Y.N] - <date>` section to `main`'s `CHANGELOG.md` (and its link definition), so the repository history includes the patch release.
- Keep the original bullets in `main`'s `## [Unreleased]` as well. The duplication is intentional: those changes also ship in the next minor release. The archived `[X.Y.N]` section records what the patch shipped without removing it from the upcoming release notes.

### 4.6 Labels for backports

Use a `backport-X.Y` label (for example `backport-0.4`) on a PR or issue when its fix should also ship on `release/X.Y`. The label is a maintainer signal for the backport PR described in section 4.4; it does not change CI. Create labels only as lines become maintained, and do not create one for a line that will receive no patches.

---

## 5. Tagging Discipline

- Tags must always be annotated: `vX.Y.Z[-pre.N]`.
- Tags are created **only** via `just release-tag <version>`, never manually: on `main` after the release PR is merged, or on `release/X.Y` for a patch release prepared as described in section 4.
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
| `CRATES_PUBLISH` | Every crate has had its first manual publish and the trusted publishers are configured | crates.io on every tag (pre-releases included) |

A pre-release tag (`vX.Y.Z-alpha.N`, `-beta.N`, `-rc.N`) publishes to TestPyPI and crates.io. A stable tag (`vX.Y.Z`) publishes to TestPyPI, PyPI and crates.io (each when enabled).

### 6.3 Trusted Publishers

Each registry trusts this repository's `release.yml` workflow running in the matching environment (`pypi`, `testpypi`, `crates-io`). Renaming the workflow file or an environment, or moving the repository, requires updating the trusted publisher on every registry. crates.io trusted publishers are configured per crate, and a crate must exist before its publisher can be added, so a **new crate needs one manual first publish** (`cargo publish -p <crate>` with a personal token) before CI can publish it. `voyager-pyo3` is not published as a Rust crate: it ships as the Python wheel and sdist (`publish = false`).

crates.io versions are **permanent**: a version can be yanked (`cargo yank --version <version> -p <crate>`) but never deleted or reused.

### 6.4 Re-runs and Scope

- PyPI and TestPyPI use `skip-existing: true`, so a re-run does not fail on files already uploaded.
- The crates.io job queries `https://crates.io/api/v1/crates/<crate>/<version>` before each publish and skips versions that already exist.
- Pre-releases go to TestPyPI and crates.io; PyPI receives stable tags only.
- npm is not published yet.

---

## 7. Release Checklist

`main` is protected, so a release goes through a pull request and the tag is created on the merge commit. Patch releases for an older line follow section 4 instead: they use `release/X.Y` as both the target branch and the tagging branch.

1. Ensure the base branch CI is completely green.
2. From an up-to-date base branch (`main` for a minor/major release, `release/X.Y` for a patch), create a release branch in a clean checkout (a fresh `git worktree` avoids untracked local files):
   ```bash
   git switch -c chore/release-<version> origin/<base>
   ```
3. Run the preparation recipe:
   ```bash
   just release-prepare <version>      # e.g. just release-prepare 0.4.0-alpha.7
   ```
   This recipe:
   - Refuses to run on `main`, on a dirty working tree, or if the tag already exists.
   - Refuses a target version that is not greater than the current workspace version.
   - On a branch based on `release/X.Y`, refuses anything that is not a patch version `X.Y.N`.
   - Runs full CI verification (`just ci`).
   - Updates `[workspace.package].version` (and internal path-dependency versions) in `Cargo.toml`.
   - Runs `cargo update -w` and `uv lock` to update lockfiles.
   - Runs `python3 scripts/check_changelog.py --release <version> <date>` to convert `[Unreleased]` to a dated section and add link definitions.
   - Creates the commit `chore(release): v<version>`.
4. Push the branch, open a pull request to `<base>` with the labels `full-ci` and `skip-changelog` (the release commit moves entries out of `[Unreleased]`, which the changelog gate would otherwise reject), and merge it once CI is green.
5. Update your local base branch and create the tag:
   ```bash
   git switch <base> && git pull --ff-only
   just release-tag <version>
   ```
   `release-tag` accepts `main` or `release/X.Y`, checks that local `<base>` equals `origin/<base>`, that `Cargo.toml` carries `<version>` and that `CHANGELOG.md` has its section, then creates the annotated tag `v<version>` and prints the push command.
6. Push the tag:
   ```bash
   git push origin v<version>
   ```
7. The `.github/workflows/release.yml` workflow triggers on the tag push, compiles multi-platform wheels and sdist, creates the GitHub Release with the extracted changelog notes, publishes to TestPyPI (if `TESTPYPI_PUBLISH == 'true'`) and crates.io (if `CRATES_PUBLISH == 'true'`) on every tag, and to PyPI (if `PYPI_PUBLISH == 'true'`) on stable tags only. See section 6 for the one-time environment and trusted-publisher setup.
   - The workflow refuses to publish a tag whose commit is not reachable from `origin/main` or the matching `origin/release/X.Y`, and only marks a stable tag **Latest** when it is the highest `v*` version.
   - Linux `x86_64` and `aarch64` wheels are built inside the `manylinux_2_28` container (glibc >= 2.28) so both targets share a portable tag. The workflow fails before creating the release if a Linux wheel filename does not carry `manylinux_2_28`.
   - Each Linux `x86_64` wheel is installed into a fresh Python 3.11 and 3.12 environment without optional extras and exercised with `scripts/smoke_base_install.py` before the release is published.
