# Contributing to Voyager

Welcome! Thank you for taking the time to contribute to Voyager.

---

## 1. Project Context & Philosophy

* **Solo Project**: Voyager is currently developed and maintained as a solo project by [@tanishq-kumar](https://github.com/tanishq-kumar).
* **Open to Constructive Criticism**: I am genuinely open to feedback, constructive critique, and alternative architectural ideas. If you see a cleaner way to design an API, optimize an algorithm, or structure a feature, your input is very welcome!
* **Maintainer Capacity**: Because I maintain this in my own personal capacity and available time, response and review times may vary. Your patience and kindness are deeply appreciated.

---

## 2. Finding Ways to Help

* **Good First Issues**: Beginner-friendly tasks are labeled as [`good first issue`](https://github.com/tanishq-kumar/voyager-ogm/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22). These are great starting points if you are new to the codebase.
* **Help Wanted**: Issues labeled [`help wanted`](https://github.com/tanishq-kumar/voyager-ogm/issues?q=is%3Aopen+is%3Aissue+label%3A%22help+wanted%22) are community tasks open for contribution.
* **Bug Fixes**: Issues labeled [`bug`](https://github.com/tanishq-kumar/voyager-ogm/issues?q=is%3Aopen+is%3Aissue+label%3A%22bug%22) are always great candidates for contribution.
* **Discuss Features First**: Before implementing large new features or major breaking changes, please open an issue or start a [GitHub Discussion](https://github.com/tanishq-kumar/voyager-ogm/discussions) first. This avoids wasted effort and ensures alignment on the design before code is written.
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
   git clone https://github.com/tanishq-kumar/voyager-ogm.git
   cd voyager-ogm
   ```

2. **Set up dependencies**:
   ```bash
   just setup
   ```
   This automatically installs all Python dependencies into a local virtual environment.

3. **Verify your toolchain**:
   ```bash
   just doctor
   ```
   This confirms that your Rust compiler, Python runner, and test suites are working properly.

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
# Run all test suites (Rust and Python)
just test

# Or run specific test suites:
just test-rust       # Rust core unit and integration tests (via cargo-nextest)
just test-python     # Python SDK tests (via pytest)
just test-snapshot   # Snapshot tests (via cargo-insta)
```

### Live Database Tests with Containers
Voyager integrates with real graph databases (Neo4j, Memgraph, Apache AGE, and FalkorDB). A pre-configured multi-database container stack is provided in `containers/compose.yaml`:

```bash
# 1. Start local database containers (Docker or Podman)
just up

# 2. Run live database integration tests against real engines
just test-live

# 3. Stop containers when finished
just down
```

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

# Full CI check (runs formatting check, linters, and all tests)
just ci
```

---

## 8. Project Structure

Voyager is structured as a monorepo:

* `crates/voyager-core`: Rust core database engine, multi-dialect AST parser, query optimizer, and Arrow bridge.
* `crates/voyager-pyo3`: PyO3 Rust-to-Python native extension bindings.
* `crates/voyager-cli`: Command-line interface tool (`voy`).
* `packages/python/voyager_ogm`: Python SDK, Polars streaming integration, and interactive graph viewer.
* `containers/compose.yaml`: Multi-engine Docker/Podman stack for local live testing.

---

## 9. Submitting a Pull Request

1. Create a feature branch from `main`:
   ```bash
   git checkout -b feat/my-new-feature
   ```
2. Make your changes and add tests where appropriate.
3. Run `just ci` to ensure all formatting, linting, and tests pass.
4. Commit your changes using conventional commit messages in simple English:
   - `feat: add math functions to expression compiler`
   - `fix: handle null property in record deserializer`
   - `docs: update setup instructions in README`
5. Push your branch to GitHub and open a Pull Request.
6. In your PR description, explain what changes you made and link any related issues (e.g. `Closes #20`).

---

## 10. Need Help?

* **Questions or Ideas**: Start a thread in [GitHub Discussions](https://github.com/tanishq-kumar/voyager-ogm/discussions).
* **Bugs & Requests**: Open an issue using our [Bug Report](https://github.com/tanishq-kumar/voyager-ogm/issues/new/choose) or [Feature Request](https://github.com/tanishq-kumar/voyager-ogm/issues/new/choose) templates.
