# ============================================================
# Voyager OGM: Automation Justfile
# ============================================================

set shell := ["powershell", "-NoProfile", "-Command"]

# Default: List available recipes
default:
    @just --list

# System diagnostic check and toolchain verification
doctor:
    @Write-Host "=== Voyager OGM Environment & Toolchain Diagnostics ===" -ForegroundColor Cyan
    @Write-Host "  [OK] Rust Compiler      : $(cargo --version)"
    @Write-Host "  [OK] Nextest Runner     : cargo-nextest $( (cargo nextest --version)[0].Split(' ')[1] )"
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
    @echo "[PASS] Environment setup complete!"

# Build all Rust workspace members and Python native extension
build:
    uv run cargo build --workspace --all-targets
    uv run maturin develop

# Build release binaries
build-release:
    uv run cargo build --workspace --release

# Run all test suites across Rust (via cargo-nextest), Python, and TypeScript
test: test-rust test-python test-ts

# Run Rust unit and integration tests with cargo-nextest
test-rust:
    uv run cargo nextest run --workspace --all-targets

# Run snapshot tests with cargo-insta
test-snapshot:
    uv run cargo insta test --workspace

# Run Python SDK tests with pytest
test-python:
    uv run pytest

# Run TypeScript SDK tests with bun test
test-ts:
    bun test

# Format code across Rust and Python
fmt:
    cargo fmt --all
    uv run ruff format .

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check
    uv run ruff format --check .

# Lint code across Rust and Python
lint:
    uv run cargo clippy --workspace --all-targets -- -D warnings
    uv run ruff check .

# Static type checking across Python using Astral ty
typecheck:
    uvx ty check packages/python

# Full Continuous Integration (CI) verification suite
ci: fmt-check lint test
    @echo "[PASS] Full CI verification passed with 0 errors!"

# Run benchmarks
bench:
    uv run cargo bench --workspace

# Run all Rust code examples
examples-rust:
    uv run cargo run --example 01_step_by_step_chaining
    uv run cargo run --example 02_semantic_shortcuts
    uv run cargo run --example 03_single_call_pattern
    uv run cargo run --example 04_expression_tree_builder
    uv run cargo run --example 05_direct_arena_allocation

# Run all Python code examples (Beginner to Advanced)
examples-python:
    uv run python examples/python/01_beginner_hello_graph.py
    uv run python examples/python/02_intermediate_relationships.py
    uv run python examples/python/03_advanced_multihop_aggregations.py
    uv run python examples/python/04_data_science_polars_streaming.py
    uv run python examples/python/05_enterprise_multi_dialect.py

# Run all code examples across Rust and Python
examples: examples-rust examples-python

# Generate synthetic scale datasets for hydration benchmarks
generate-bench-data nodes="100000" edges="500000" out="test_data/bench_100k":
    uv run python test_data/generator/generate_bench_dataset.py --nodes {{nodes}} --edges {{edges}} --output-dir {{out}}

# Start local graph database containers via Podman/Docker Compose
up:
    podman compose -f containers/compose.yaml up -d

# Stop and tear down local graph database containers
down:
    podman compose -f containers/compose.yaml down

# Run live database integration tests against real running databases
test-live:
    uv run pytest packages/python/tests/test_live_database_bridge.py packages/python/tests/test_real_world_scenarios.py -v

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
    uv run pytest packages/python/tests/test_live_matrix.py -v
