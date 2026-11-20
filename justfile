# ============================================================
# Voyager OGM: Automation Justfile
# ============================================================

set windows-shell := ["powershell", "-NoProfile", "-Command"]

export UV_PYTHON_INSTALL_DIR := env_var_or_default("UV_PYTHON_INSTALL_DIR", home_directory() / ".cargo/uv-python")
# RUSTC_WRAPPER is deliberately not set here, so a compile cache configured in the
# environment or in ~/.cargo/config.toml (e.g. `rustc-wrapper = "sccache"`) applies to
# every recipe. If sccache reports "Connection refused" / "Connection to server timed
# out", restart it (`sccache --stop-server`) or bypass it for one command:
# `RUSTC_WRAPPER= CARGO_BUILD_RUSTC_WRAPPER= just <recipe>`.

# Default: List available recipes
default:
    @just --list

# System diagnostic check and toolchain verification
[unix]
doctor:
    @echo "=== Voyager OGM Environment & Toolchain Diagnostics ==="
    @echo "  [OK] Rust Compiler      : $(cargo --version)"
    @echo "  [OK] Nextest Runner     : $(cargo nextest --version 2>/dev/null || echo 'not installed')"
    @echo "  [OK] sccache            : $(sccache --version 2>/dev/null || echo 'not installed')"
    @echo "  [OK] mold linker        : $(mold --version 2>/dev/null | cut -d' ' -f1-2 || echo 'not installed')"
    @echo "  [OK] Python Toolchain   : $(uv --version)"
    @echo "  [OK] TypeScript Runtime : Bun v$(bun --version)"
    @echo ""
    @echo "=== Auditing Dialect & Vendor TCK Suites ==="
    @uv run python test_data/tck/verify_tck.py
    @echo "\n[PASS] All environment checks completed successfully!"

[windows]
doctor:
    @Write-Host "=== Voyager OGM Environment & Toolchain Diagnostics ===" -ForegroundColor Cyan
    @Write-Host "  [OK] Rust Compiler      : $(cargo --version)"
    @Write-Host "  [OK] Nextest Runner     : cargo-nextest $( (cargo nextest --version)[0].Split(' ')[1] )"
    @Write-Host "  [OK] sccache            : $( (sccache --version).Trim() )"
    @Write-Host "  [OK] Python Toolchain   : $(uv --version)"
    @Write-Host "  [OK] TypeScript Runtime : Bun v$(bun --version)"
    @Write-Host ""
    @Write-Host "=== Auditing Dialect & Vendor TCK Suites ===" -ForegroundColor Cyan
    @uv run python test_data/tck/verify_tck.py
    @Write-Host "`n[PASS] All environment checks completed successfully!" -ForegroundColor Green

# Setup development dependencies across Rust, Python, and TypeScript
setup:
    @echo "=== Setting up Python virtual environment ==="
    @uv sync --all-extras
    @echo "=== Setting up TypeScript workspace ==="
    @bun install
    @echo "=== Fetching official conformance repositories ==="
    @uv run python scripts/fetch_conformance.py
    @echo "[PASS] Environment setup complete!"

# Fetch or update official vendor conformance repositories (Apache AGE, FalkorDB)
fetch-conformance:
    @uv run python scripts/fetch_conformance.py

# Run official FalkorDB conformance test suite
test-falkordb-official:
    uv run cargo test -p voyager-drivers --test official_falkordb_tests -- --nocapture

# Fast type/borrow check of the whole workspace (no codegen, no linking)
check:
    cargo check --workspace --all-targets

# Check every supported Cargo feature combination with clippy (warnings are errors)
check-features:
    cargo clippy -p voyager-ogm --no-default-features -- -D warnings
    cargo clippy -p voyager-ogm --no-default-features --features serde -- -D warnings
    cargo clippy -p voyager-ogm --no-default-features --features arrow -- -D warnings
    cargo clippy -p voyager-ogm -- -D warnings
    cargo clippy -p voyager-drivers --no-default-features --features bolt -- -D warnings
    cargo clippy -p voyager-drivers --no-default-features --features postgres -- -D warnings
    cargo clippy -p voyager-drivers --no-default-features --features redis -- -D warnings
    cargo clippy -p voyager-drivers --no-default-features --features arrow -- -D warnings
    cargo clippy -p voyager-drivers -- -D warnings
    cargo clippy -p voyager-drivers --all-features -- -D warnings

# Rebuild only the Python native extension into the venv (inner dev loop)
dev:
    uv run maturin develop

# Build all Rust workspace members and Python native extension
build:
    uv run cargo build --workspace --all-targets
    uv run maturin develop

# Build release binaries
build-release:
    uv run cargo build --workspace --release

# Run all test suites across Rust (via cargo-nextest and doctests), Python, and TypeScript
test: test-rust test-doc test-python test-ts

# Run Rust unit and integration tests with cargo-nextest
test-rust:
    uv run cargo nextest run --workspace --all-targets

# Run Rust doctests, including the crate README examples
test-doc:
    uv run cargo test --doc --workspace

# Run snapshot tests with cargo-insta
test-snapshot:
    uv run cargo insta test --workspace

# Run Python SDK tests with pytest (skipping slow live container integration tests)
test-python:
    uv run --no-sync pytest -m "not live"

# Run Relate embedded database integration tests
test-relate:
    uv run --no-sync pytest packages/python/tests/test_relate_compatibility.py -v

# Run TypeScript SDK tests with bun test (skipped when the TypeScript package is not present)
[unix]
test-ts:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ ! -d packages/typescript ]]; then
        echo "[SKIP] packages/typescript not present; no TypeScript tests to run."
        exit 0
    fi
    bun test packages/typescript

# Run TypeScript SDK tests with bun test (skipped when the TypeScript package is not present)
[windows]
test-ts:
    if (Test-Path packages/typescript) { bun test packages/typescript } else { Write-Host "[SKIP] packages/typescript not present; no TypeScript tests to run." }

# Format code across Rust and Python
fmt:
    cargo fmt --all
    uv run --no-sync ruff format .

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check
    uv run --no-sync ruff format --check .

# Validate CHANGELOG.md conformance with Keep a Changelog 1.1.0
changelog-check:
    uv run python scripts/check_changelog.py --format

# Lint code across Rust and Python
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    uv run --no-sync ruff check .

# Report panic-prone code (unwrap/expect/panic/indexing) outside tests. Informational, not a gate.
lint-strict:
    cargo clippy --workspace --all-targets -- -W clippy::unwrap_used -W clippy::expect_used -W clippy::panic -W clippy::indexing_slicing

# Supply-chain checks: security advisories, licenses, banned/duplicate crates, sources (cargo install --locked cargo-deny)
deny:
    cargo deny check

# Detect unused dependencies in Cargo.toml files (cargo install --locked cargo-machete)
machete:
    cargo machete

# Vulnerability scan of every lockfile: Rust (Cargo.lock), Python (uv.lock), TypeScript (bun.lock).
# Lockfiles are listed explicitly so the scan never depends on .gitignore; bun.lock is scanned only when present.
# Accepted findings live in osv-scanner.toml.
audit:
    osv-scanner scan source -L Cargo.lock -L uv.lock {{ if path_exists("bun.lock") == "true" { "-L bun.lock" } else { "" } }}

# Live clippy while YOU edit (bacon). Uses its own target dir so it never fights agents or `just` for the cargo build lock.
watch job="clippy":
    CARGO_TARGET_DIR=target/bacon bacon {{job}}

# Static type checking across Python using Astral ty and Mypy (PEP 561 / PEP 681)
typecheck:
    uv run --no-sync ty check packages/python
    uv run --no-sync mypy packages/python/voyager_ogm

# Full Continuous Integration (CI) verification suite
ci: fmt-check lint deny machete audit test
    @echo "[PASS] Full CI verification passed with 0 errors!"

# Release step 1 (on a release branch): run CI, bump the version, update lockfiles, cut the changelog, commit
[unix]
release-prepare version:
    #!/usr/bin/env bash
    set -euo pipefail
    branch=$(git rev-parse --abbrev-ref HEAD)
    if [[ "$branch" == "main" ]]; then
        echo "Error: main is protected; run release-prepare on a release branch (e.g. chore/release-{{version}})." >&2
        exit 1
    fi
    if [[ -n "$(git status --porcelain)" ]]; then
        echo "Error: Working directory is not clean. Commit or stash changes before releasing." >&2
        exit 1
    fi
    if git rev-parse -q --verify "refs/tags/v{{version}}" >/dev/null; then
        echo "Error: tag v{{version}} already exists." >&2
        exit 1
    fi
    echo "=== Running Full CI Suite ==="
    just ci
    echo "=== Updating Version to {{version}} in Cargo.toml ==="
    python3 -c "import pathlib, tomllib; p = pathlib.Path('Cargo.toml'); text = p.read_text(encoding='utf-8'); old = tomllib.loads(text)['workspace']['package']['version']; needle = 'version = \"' + old + '\"'; n = text.count(needle); assert n >= 1, 'workspace version not found'; p.write_text(text.replace(needle, 'version = \"{{version}}\"'), encoding='utf-8'); print(f'Bumped {old} -> {{version}} in {n} places (workspace package + internal path deps)')"
    echo "=== Updating Lockfiles ==="
    cargo update -w
    uv lock
    echo "=== Cutting CHANGELOG.md Section ==="
    python3 scripts/check_changelog.py --release "{{version}}" "$(date +%Y-%m-%d)"
    echo "=== Creating Release Commit ==="
    git add Cargo.toml Cargo.lock uv.lock CHANGELOG.md
    git commit -m "chore(release): v{{version}}"
    echo ""
    echo "Release commit for v{{version}} created on '$branch'."
    echo "Next: push the branch, open a PR to main with labels full-ci and skip-changelog, merge it, then run 'just release-tag {{version}}' on main."

# Release step 2 (on main, after the release PR is merged): verify the release and create the annotated tag
[unix]
release-tag version:
    #!/usr/bin/env bash
    set -euo pipefail
    branch=$(git rev-parse --abbrev-ref HEAD)
    if [[ "$branch" != "main" ]]; then
        echo "Error: tag releases from 'main' (currently on '$branch')." >&2
        exit 1
    fi
    if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
        echo "Error: tracked files have local changes." >&2
        exit 1
    fi
    git fetch -q origin main
    if [[ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]]; then
        echo "Error: local main is not at origin/main; pull first." >&2
        exit 1
    fi
    cargo_version=$(python3 -c "import pathlib, tomllib; print(tomllib.loads(pathlib.Path('Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version'])")
    if [[ "$cargo_version" != "{{version}}" ]]; then
        echo "Error: Cargo.toml version is '$cargo_version', expected '{{version}}' (is the release PR merged?)." >&2
        exit 1
    fi
    if [[ -z "$(python3 scripts/check_changelog.py --extract '{{version}}')" ]]; then
        echo "Error: CHANGELOG.md has no [{{version}}] section." >&2
        exit 1
    fi
    if git rev-parse -q --verify "refs/tags/v{{version}}" >/dev/null; then
        echo "Error: tag v{{version}} already exists." >&2
        exit 1
    fi
    git tag -a "v{{version}}" -m "Release v{{version}}"
    echo "Created annotated tag v{{version}} on $(git rev-parse --short HEAD)."
    echo "To publish, run:"
    echo "    git push origin v{{version}}"

# Pre-commit hook recipe (formatting, linting, and type checking)
pre-commit: fmt-check lint typecheck
    @echo "[PASS] Pre-commit checks passed cleanly!"

# Pre-push hook recipe (runs full test matrix and captures benchmark snapshots)
pre-push: test bench-save
    @echo "[PASS] Pre-push verification and benchmark capture complete!"

# Run Rust micro-benchmarks
bench:
    uv run cargo bench --workspace

# Run Python PyCapsule & hydration benchmarks (collects bench_*.py explicitly)
bench-python:
    uv run --no-sync pytest packages/python/benches/ --benchmark-only -o python_files="bench_*.py"

# Run head-to-head OGM comparison benchmark against Neomodel and Pydantic v2
bench-ogm:
    uv run python packages/python/benches/bench_head_to_head.py

# Re-generate vector SVG benchmark charts for benchmarks/README.md
bench-charts:
    uv run python scripts/generate_benchmark_charts.py

# Run complete benchmark suite, save JSON snapshot, and re-generate charts
bench-all name="":
    uv run cargo bench --workspace
    uv run python scripts/save_benchmarks.py {{name}}
    uv run python scripts/generate_benchmark_charts.py

# Run Python & Rust hydration/compilation benchmarks and dynamically save CSV summary
bench-save name="":
    uv run python scripts/save_benchmarks.py {{name}}

# Full reset of target/ (dependencies are restored cheaply from the sccache cache)
clean:
    cargo clean

# Show sccache hit/miss statistics and cache size
cache-stats:
    sccache --show-stats

# Remove packaging leftovers only; keeps incremental + dependency caches (use `just clean` for a full reset)
[unix]
prune:
    @rm -rf target/wheels target/tmp && echo "[OK] Removed target/wheels and target/tmp"
    @du -sh target 2>/dev/null || true
    @echo "[PASS] Pruned. Incremental cache kept, so the next build stays incremental."

[windows]
prune:
    @if (Test-Path target/wheels) { Remove-Item -Recurse -Force target/wheels }
    @if (Test-Path target/tmp) { Remove-Item -Recurse -Force target/tmp }
    @Write-Host "[PASS] Pruned. Incremental cache kept, so the next build stays incremental." -ForegroundColor Green

# Alias for prune
prune-debug: prune


# Run all Rust code examples (offline / no database needed)
examples-rust:
    uv run cargo run --example 01_step_by_step_chaining
    uv run cargo run --example 02_semantic_shortcuts
    uv run cargo run --example 03_single_call_pattern
    uv run cargo run --example 04_expression_tree_builder
    uv run cargo run --example 05_direct_arena_allocation
    uv run cargo run --example 06_ast_optimizer_constant_folding
    uv run cargo run --example 07_explain_and_profile_queries
    uv run cargo run -p voyager-ogm --example 08_schema_ddl_and_indexes
    uv run cargo run -p voyager-ogm --example 09_mutation_builder
    uv run cargo run -p voyager-ogm --example 10_bulk_ingest_compilation
    uv run cargo run -p voyager-ogm --example 11_transactions_and_unit_of_work
    uv run cargo run -p voyager-ogm --example 12_vector_and_fulltext_search

# Run all Python code examples (Beginner to Advanced, offline / no database needed)
examples-python:
    uv run python examples/python/01_hello_graph.py
    uv run python examples/python/02_minimal_type_annotations.py
    uv run python examples/python/03_custom_labels_and_constraints.py
    uv run python examples/python/04_relationships_and_traversals.py
    uv run python examples/python/05_oop_class_inheritance.py
    uv run python examples/python/06_multihop_aggregations.py
    uv run python examples/python/07_mutation_dsl.py
    uv run python examples/python/08_multi_dialect_compilation.py
    uv run python examples/python/09_enterprise_dialects.py
    uv run python examples/python/10_query_optimizer_constant_folding.py
    uv run python examples/python/11_query_profiling_and_explain.py
    uv run python examples/python/12_vector_and_fulltext_search.py
    uv run python examples/python/13_schema_ddl_and_indexes.py
    uv run python examples/python/14_schema_validation.py
    uv run python examples/python/15_topology_extraction.py
    uv run python examples/python/16_spec_interchange.py
    uv run python examples/python/17_database_bridges.py
    uv run python examples/python/18_transactions_and_identity_map.py
    uv run python examples/python/19_bulk_ingestion.py
    uv run python examples/python/20_embedded_relate_bridge.py
    uv run python examples/python/21_data_science_polars_streaming.py
    uv run python examples/python/22_async_and_hybrid_sessions.py
    uv run python examples/python/23_sqlalchemy_hybrid_bridge.py
    uv run python examples/python/24_sqlalchemy_graph_table.py
    uv run python examples/python/25_networkx_analytics.py
    uv run python examples/python/26_query_path_visualization.py
    uv run python examples/python/27_marimo_graphrag_demo.py
    uv run python examples/python/29_native_rust_builder.py

# Run live code examples (requires running database containers via `just up` or Compose runner)
examples-live:
    uv run python examples/python/30_live_neo4j_execution.py
    uv run python examples/python/31_live_multi_engine_execution.py
    uv run cargo run -p voyager-drivers --example live_explain_and_profile
    uv run cargo run -p voyager-drivers --example live_connect_and_query

# Run all code examples across Rust and Python (offline)
examples: examples-rust examples-python

# Generate synthetic scale datasets for hydration benchmarks
generate-bench-data nodes="100000" edges="500000" out="test_data/bench_100k":
    uv run python test_data/generator/generate_bench_dataset.py --nodes {{nodes}} --edges {{edges}} --output-dir {{out}}

# Start local graph database containers (all, or named ones: `just up neo4j`). Pinned tags fall back to :latest.
[unix]
up *services:
    #!/usr/bin/env bash
    set -uo pipefail
    pull_or_latest() { # VAR repo pinned
        if podman pull -q "docker.io/$2:$3" >/dev/null 2>&1; then export "$1=$3"
        else echo "[WARN] $2:$3 failed to pull, falling back to :latest"; export "$1=latest"; fi
    }
    want() { [ -z "{{services}}" ] || [[ " {{services}} " == *" $1 "* ]]; }
    want memgraph   && pull_or_latest MEMGRAPH_TAG memgraph/memgraph-mage 3.14.0
    want apache-age && pull_or_latest AGE_TAG apache/age release_PG18_1.8.0
    want falkordb   && pull_or_latest FALKORDB_TAG falkordb/falkordb 6.0.4
    podman compose -f containers/compose.yaml up -d {{services}}

[windows]
up *services:
    podman compose -f containers/compose.yaml up -d {{services}}

# Stop and tear down local graph database containers
down:
    podman compose -f containers/compose.yaml down

# Run live database integration tests against real running databases
test-live:
    uv run pytest -m live packages/python/tests/test_live_database_bridge.py packages/python/tests/test_real_world_scenarios.py -v

# Run openCypher TCK conformance test suite
test-tck:
    uv run pytest packages/python/tests/test_tck_conformance.py -v

# Run ISO GQL (ISO/IEC 39075:2024) conformance test suite
test-gql:
    uv run pytest packages/python/tests/test_gql_conformance.py -v

# Run SQL:2023 PGQ and DuckPGQ conformance test suite
test-pgq:
    uv run pytest packages/python/tests/test_pgq_conformance.py -v

# Run Apache AGE (PostgreSQL Embedded Cypher) conformance test suite
test-age:
    uv run pytest packages/python/tests/test_age_conformance.py -v

# Run Multi-Engine Live Matrix integration tests (Neo4j, Memgraph, Apache AGE, DuckDB)
test-matrix:
    uv run pytest -m live packages/python/tests/test_live_matrix.py -v

# Launch Voyex Manager developer portal & orchestrator
manager:
    cd ../voyex-manager && uv run python launch.py
