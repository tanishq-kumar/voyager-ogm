# Contributing to Voyager

Welcome! Thank you for taking the time to contribute to Voyager.

---

## 1. Project Context & Philosophy

* **Solo Project**: Voyager is currently developed and maintained as a solo project by [@tanishq-kumar](https://github.com/tanishq-kumar).
* **Open to Constructive Criticism**: I am genuinely open to feedback, constructive critique, and alternative architectural ideas. If you see a cleaner way to design an API, optimize an algorithm, or structure a feature, your input is very welcome!
* **Maintainer Capacity**: Because I maintain this in my own personal capacity and available time, response and review times may vary. Your patience and kindness are deeply appreciated.

---

## 2. Finding Ways to Help

* **Good First Issues**: Beginner-friendly tasks are labeled as [`good first issue`](https://github.com/tanishq-kumar/voyager/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22). These are great starting points if you are new to the codebase.
* **Help Wanted**: Issues labeled [`help wanted`](https://github.com/tanishq-kumar/voyager/issues?q=is%3Aopen+is%3Aissue+label%3A%22help+wanted%22) are community tasks open for contribution.
* **Bug Fixes**: Issues labeled [`bug`](https://github.com/tanishq-kumar/voyager/issues?q=is%3Aopen+is%3Aissue+label%3A%22bug%22) are always great candidates for contribution.
* **Discuss Features First**: Before implementing large new features or major breaking changes, please open an issue or start a [GitHub Discussion](https://github.com/tanishq-kumar/voyager/discussions) first. This avoids wasted effort and ensures alignment on the design before code is written.
* **Avoiding Duplicate Work**: Check existing issues and open pull requests before starting to avoid duplicate work.

---

## 3. AI & Agentic Contributions Policy

You are welcome to use AI coding assistants and autonomous agents (e.g. Claude, Cursor, ChatGPT, Antigravity, Copilot) to help write code.

> [!WARNING]
> **You are responsible for any code you submit.**
> Because Voyager compiles database queries and manages zero-copy memory in Rust and Python, a small bug can easily cause incorrect query results or crashes. As the maintainer, I need to understand and trust every change merged into the project. Therefore:
> 1. **Understand your code**: If an AI tool or agent wrote or refactored the code, please make sure you personally understand how it works and can explain the logic.
> 2. **Explain in your own words**: Please write PR descriptions and replies yourself. Avoid pasting unreviewed AI text when discussing changes.
> 3. **Verify with tests**: Make sure tests pass locally (`just ci` or `just test`) and that new code includes tests to prove it works.
> 4. **Be transparent**: If an AI assistant or agent helped write the code, simply mention it in your PR description—transparency is always appreciated!

---

## 4. Development Prerequisites

To work on Voyager locally, make sure you have the following installed:

1. **Rust**: Stable toolchain (`rustup update stable`)
2. **Python**: Python 3.11 or newer
3. **uv**: Fast Python package installer (`curl -LsSf https://astral.sh/uv/install.sh` or `winget install astral-sh.uv`)
4. **just**: Command runner (`cargo install just` or `winget install Casey.Just`)
5. **Docker or Podman** (optional): Needed for running live database integration tests

---

## 5. Quick Setup

1. **Clone the repository**:
   ```bash
   git clone https://github.com/tanishq-kumar/voyager.git
   cd voyager
   ```

2. **Set up dependencies**:
   ```bash
   just setup
   ```
   This installs the Python dependencies into a local virtual environment and fetches the official conformance corpora. The recipe also runs `bun install`; that only matters once the TypeScript workspace (`packages/typescript`) exists, and it is not part of this repository yet.

3. **Verify your toolchain**:
   ```bash
   just doctor
   ```
   This checks the Rust, Python, and TypeScript toolchains and audits the dialect TCK suites.

4. **Build the project**:
   ```bash
   just build
   ```
   This compiles the Rust core engine and builds the local Python extension via `maturin develop`.

---

## 6. Running Tests

### Standard Unit & Conformance Tests
Before submitting changes, make sure all standard tests pass:

```bash
# Run all test suites (Rust unit/integration + doctests, Python, and TypeScript)
just test

# Or run specific test suites:
just test-rust       # Rust unit and integration tests (via cargo-nextest)
just test-doc        # Rust doctests, including the crate README examples (cargo test --doc)
just test-python     # Python SDK tests (via pytest)
just test-snapshot   # Snapshot tests (via cargo-insta)
```

### Live Database Tests with Containers
Voyager integrates with real graph databases (Neo4j, Memgraph, Apache AGE, and FalkorDB). A pre-configured multi-database container stack is provided in `containers/compose.yaml`:

```bash
# 1. Start local database containers
just up                # uses `podman compose`
# Docker users: docker compose -f containers/compose.yaml up -d

# 2. Run live database integration tests against real engines
just test-live

# 3. Stop containers when finished
just down              # uses `podman compose`
```

**Neo4j edition and licensing.** `just up` starts **Neo4j Community** (open source), and every live test passes on it; tests that need Enterprise-only features (property-existence constraints) adapt automatically. Neo4j Enterprise is opt-in, and you must accept Neo4j's licence terms yourself. Nothing in this repository accepts them for you:

```bash
# "eval" = Neo4j's 30-day evaluation agreement (internal evaluation only).
# Use "yes" only if you hold a Neo4j commercial licence. See https://neo4j.com/terms/licensing/
NEO4J_EDITION=enterprise NEO4J_ACCEPT_LICENSE_AGREEMENT=eval just up neo4j
```

Memgraph (BSL 1.1) and FalkorDB (SSPL v1) are pulled and run locally only; this project never redistributes them.

### Git Hooks

The repository ships hooks in `.githooks/`. Enable them once per clone:

```bash
git config core.hooksPath .githooks
```

`just setup` does not set this for you. The hooks delegate to `just`:

- `pre-commit` runs `just pre-commit` (format check, lint, and Python type checks) — fast enough for every commit.
- `pre-push` currently runs `just pre-push`, which is the full `just test` suite plus a benchmark capture (`just bench-save`). That is slow; use `git push --no-verify` when you only need to push, until the pre-push hook is made fast (tracked separately).

---

## 7. Code Formatting and Linting

The codebase maintains clean formatting and strict linting standards:

```bash
# Format code automatically (cargo fmt + ruff format)
just fmt

# Run all linters (cargo clippy + ruff check)
just lint

# Run static type checking (ty)
just typecheck

# Full CI check (format/lint, supply-chain gates, and all tests)
just ci
```

`just ci` runs `fmt-check`, `lint`, `deny`, `machete`, `audit`, and the complete `test` suite (Rust unit/integration tests, doctests, Python, and TypeScript when present).

---

## 8. Project Structure

Voyager is structured as a monorepo:

* `crates/voyager-ogm`: Core query compiler — fluent builder, AST arena, optimizer, multi-dialect emitters (openCypher, SQL:2023 PGQ, ISO GQL, Apache AGE), schema registry, Arrow bridge.
* `crates/voyager-drivers`: Native async driver layer — Bolt (Neo4j, Memgraph), PostgreSQL wire (Apache AGE, PostgreSQL 19), Redis RESP (FalkorDB), connection pooling, native transactions.
* `crates/voyager-cli`: The `voyager` binary — diagnostics (`doctor`), plus early `compile`/`bench` placeholders (planned short alias: `voy`).
* `crates/voyager-pyo3`: PyO3 native extension for the Python package (not published to crates.io; built into the wheel by maturin).
* `packages/python/voyager_ogm`: Python SDK — sessions, models, bridges, Polars streaming, graph viewer.
* `examples/`: Progressive Python learning path (01-31) plus a README cross-indexing the Rust examples in `crates/*/examples/`.
* `benchmarks/`: Benchmark scripts, `benchmark_summary.json`, the published SVGs; see `benchmarks/README.md`.
* `scripts/`: Release, benchmark, smoke-test, and conformance-fetch automation.
* `spec/`: The query-spec JSON schema (`query_spec.v1.schema.json`).
* `test_data/`: TCK/conformance corpora and seed data.
* `fuzz/`: cargo-fuzz targets (excluded from the Cargo workspace).
* `containers/compose.yaml`: Multi-engine Compose stack (Neo4j, Memgraph, Apache AGE, FalkorDB, PostgreSQL) for live tests.
* `.github/workflows/` + `justfile`: CI (`ci.yml`), releases (`release.yml`), PR tooling, and every dev recipe.

A TypeScript SDK (`packages/typescript`) is planned but is not part of this repository yet.

---

## 9. API Design & Alias Policy

Following the cleanup in `#131`, Voyager maintains a strict policy regarding API aliases to prevent API surface sprawl and maintain documentation clarity:

1. **Aliases Are Name Bindings Only**: Aliases must never duplicate underlying implementation logic or maintain parallel execution branches. An alias must be an exact one-line reference or binding to the canonical function, class, or method (e.g. `text_search = fulltext_search` or `TextIndex = FullTextIndex`).
2. **Canonical Form Documented**: Documentation, guides, examples, and error diagnostics must always reference and teach the canonical name. The alias exists strictly for developer ergonomics across diverse graph communities (e.g. Memgraph's `text_search` alongside openCypher's `fulltext_search`).
3. **Every Alias Must Be Tested**: Any introduced alias must have an explicit unit test asserting its identity or exact parity with the canonical implementation.

---

## 10. Submitting a Pull Request

1. Create a branch from `main` named `<type>/<issue>-<short-desc>`, using the same type as your commit (real examples from this repository: `fix/221-age-uri-graph-param`, `feat/192-create-relationship-matched`, `refactor/217-rename-voyager-drivers`, `ci/242-trusted-publishing`):
   ```bash
   git switch -c fix/221-age-uri-graph-param
   ```
2. Make your changes and add tests where appropriate.
3. Update `CHANGELOG.md`:
   - Every user-facing pull request must add a bullet under `## [Unreleased]` in `CHANGELOG.md` following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). To check it locally, run `python3 scripts/check_changelog.py --format` and `python3 scripts/check_changelog.py --require-unreleased-change origin/main`.
   - Internal-only pull requests (CI, internal refactors) are exempt if tagged with the `skip-changelog` label.
4. Run `just ci` to ensure formatting, linting, supply-chain checks, and all tests pass.
5. Commit with a subject in the form `<type>(<scope>): <summary> (#issue)`. Omit the scope only for repo-wide changes (e.g. `refactor: rename voyager-net crate to voyager-drivers (#217)`). Real examples:
   - `fix(bridge): accept ?graph= / ?graph_name= on PostgreSQL and AGE URIs (#221)`
   - `feat(query): create and merge relationships between matched nodes (#192)`
   - `fix(net): bind PostgreSQL and AGE parameters instead of interpolating them (#222)`
   - `fix(ddl): emit uniqueness constraints for unique relationship fields (#211)`
   - `refactor(core): resolve primary key and MERGE key from the native schema registry (#200)`
   - `feat(drivers): Cargo features per protocol, slimmer Arrow, README example links (#218)`
6. Push your branch and open a pull request. Fill in the repository PR template (`.github/pull_request_template.md`): a summary, the type checkbox, related issues, and the verification checklist. The workflow `.github/workflows/pr-description.yml` keeps the commit list in the description up to date.
7. Add the labels that change CI:
   - `full-ci` expands the Rust and Python matrices to macOS and Windows (and Python 3.11-3.14).
   - `skip-changelog` exempts the PR from the `[Unreleased]` changelog gate.
   Labels are read when a CI run starts, not when the PR is opened. Add them when you open the PR (or before a manual re-run); a label added later needs a CI re-run to take effect.
8. CI runs once per opened or reopened pull request and once per push; label-only changes do not start a new run.

---

## 11. Releasing

Voyager OGM follows SemVer with pre-1.0 rules and a tag-driven, trusted-publishing release process. In short (see [RELEASING.md](RELEASING.md) for the full standard):

1. From an up-to-date `main`, create a release branch (`chore/release-<version>`) and run `just release-prepare <version>` — it runs full CI, bumps the workspace version, refreshes lockfiles, cuts the changelog section, and creates the `chore(release): v<version>` commit.
2. Open the release PR to `main` with the labels `full-ci` and `skip-changelog`, and merge it once green.
3. On `main`, run `just release-tag <version>` and push the annotated tag. `.github/workflows/release.yml` then builds the wheels, creates the GitHub Release, and publishes via OIDC trusted publishing — TestPyPI for pre-release tags; PyPI and crates.io for stable tags. `voyager-pyo3` is never published as a crate (it ships as the Python wheel).

---

## 12. Contribution Licensing

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Voyager OGM by you, as defined in the Apache-2.0 license, shall be dual licensed under the MIT License and the Apache License, Version 2.0, without any additional terms or conditions.

---

## 13. Need Help?

* **Questions or Ideas**: Start a thread in [GitHub Discussions](https://github.com/tanishq-kumar/voyager/discussions).
* **Bugs & Requests**: Open an issue using our [Bug Report](https://github.com/tanishq-kumar/voyager/issues/new/choose) or [Feature Request](https://github.com/tanishq-kumar/voyager/issues/new/choose) templates.
