# Project Rules & Guidelines for Voyager OGM (agy rules)

## 1. Git & Version Control Restrictions
- **No Automatic Commits or Pushes**: Never run `git commit` or `git push` unless the user explicitly requests it.
- **No Automatic Branch or PR Creation**: Never create branches, GitHub Pull Requests, issues, or repositories without explicit user permission.
- **Branch Naming Convention**:
  - Format: `<type>/<issue_id>-<short-description>`
  - Examples: `feat/117-networkx-export-bridge`, `fix/spec-version-errors-mockbridge`

## 2. Commit Message Format
Follow the project's established Conventional Commits pattern:
- Format: `<type>(<scope>): <imperative summary> (optional #issue_id)` or `<type>: <imperative summary>`
- Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`
- Scopes: `bridge`, `core`, `net`, `session`, `types`, `ddl`, `query`, `ingest`, `analytics`, `examples`, etc.
- Tone/Style: Lowercase, imperative mood (e.g., `feat(bridge): add relate-db embedded adapter`, `test(bridge): add unit tests for relate driver`).

## 3. Automation and Task Execution via `just`
- Always prefer running tasks through `just` recipes instead of manual long commands:
  - `just test-python` - Run Python test suite (`uv run pytest -m "not live"`)
  - `just test-rust` - Run Rust test suite via cargo-nextest
  - `just test` - Run full test suite (Rust, Python, TS)
  - `just lint` - Run Clippy and Ruff lint checks
  - `just fmt` / `just fmt-check` - Format / verify code formatting
  - `just typecheck` - Run `ty` and `mypy` type checks
  - `just build` - Build Rust crates and Maturin Python extension
  - `just ci` - Run full CI verification (fmt-check, lint, test)
- **Extending `justfile`**: If any complex or frequently repeated command is missing, add a dedicated recipe to `justfile`.
