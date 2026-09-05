# Voyager OGM

**Multi-Dialect Object-Graph Mapper (OGM) and Query Compiler**

[![CI](https://github.com/tanishq-kumar/voyager-ogm/actions/workflows/ci.yml/badge.svg)](https://github.com/tanishq-kumar/voyager-ogm/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Changelog](https://img.shields.io/badge/Changelog-Keep%20a%20Changelog-informational.svg)](CHANGELOG.md)
[![Standards](https://img.shields.io/badge/Standards-openCypher%20%7C%20SQL%3A2023%20PGQ%20%7C%20ISO%20GQL-green.svg)](https://www.iso.org/standard/76120.html)
[![Arrow](https://img.shields.io/badge/Zero--Copy-Apache%20Arrow%20PyCapsule-orange.svg)](https://arrow.apache.org/)

---

## Why Voyager OGM?

I started Voyager after facing real vendor lock-in pain migrating between Neo4j, Memgraph, and PostgreSQL AGE. In the relational world, ORMs like SQLAlchemy allow teams to switch backends with minimal friction. In the graph ecosystem, every vendor has subtle dialect differences, proprietary driver APIs, and hydration bottlenecks.

After building an initial prototype in 2025, I dedicated focused time to turn it into a production-targeting toolkit. The goal is simple: **write one graph query, run it anywhere**.

Voyager is engineered in safe Rust to provide a high-performance, memory-efficient core, exposed to Python (`PyO3`) with zero-copy Apache Arrow streaming, with TypeScript (`NAPI-RS`) support planned.

---

## Overview

Voyager OGM is a graph database toolkit written in safe Rust with bindings for Python (`PyO3`) and planned bindings for TypeScript (`NAPI-RS`).

To start writing code immediately, jump to [Local Development Setup](#local-development-and-code-usage) or inspect the available [Developer Commands](#developer-commands).

Voyager OGM addresses four common challenges in graph database development:

1. **Slow Object Hydration:** Leverages the **Apache Arrow C Data Interface** (`__arrow_c_stream__`) to stream graph query records directly into **Polars DataFrames** without intermediate Python object instantiation overhead. See [Zero-Copy Polars Streaming](#zero-copy-polars-streaming).
2. **Dialect Lock-In:** Compiles a single unified query AST into **openCypher**, **SQL:2023 PGQ** (`GRAPH_TABLE`), and **ISO GQL** (ISO/IEC 39075:2024).
3. **Memory Safety & Efficiency:** Uses a compact handle-based AST arena (`NodeHandle`) with automatic memory reclamation.
4. **Transaction Safety:** Features a two-layer Unit of Work with nested in-memory savepoints that automatically rolls back uncommitted mutations and AST allocations if a query fails.

---

## Project Plan & Roadmap (7 Phases)

| Phase                                         | Scope                                                                                                                                              | Target Version |     Status     |
| :-------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------- | :------------: | :------------: |
| **Phase 1: Core Read AST Engine**             | Handle-based AST (`QueryAstArena`), multi-dialect emitters (openCypher, SQL:2023 PGQ, ISO GQL), PyO3 Python SDK                                    | `v0.1.0-alpha` |  ✅ Completed  |
| **Phase 2: Mutations, Ingestion & Streaming** | DML mutations, 2-layer rollback transaction UoW, Bulk `UNWIND $batch`, Zero-copy Arrow/Polars streaming, Database bridging                         | `v0.2.0-beta`  |  ✅ Completed  |
| **Phase 3: Multi-Dialect Syntax Conformance** | Standard openCypher, ISO GQL, DuckPGQ & Apache AGE syntax test suites, 6-engine live integration matrix | `v0.3.0` | ✅ Completed |
| **Phase 4: Python Integrations & Optimizers** | SQLAlchemy hybrid bridge, Interactive Notebook Graph Viewer (`voyager_ogm.viewer`), Multi-database batch Identity Map, AST predicate pushdown pass | `v0.4.0` | 🔄 In Progress |
| **Phase 5: TypeScript SDK** | NAPI-RS native bindings, `@Node` / `@Relationship` decorators, fluent query builder, Arrow streaming | `v0.5.0` | 🚧 Planned |
| **Phase 6: Voyager CLI Engine** | Standalone CLI (`clap`), `voyager compile`, `voyager migrate`, `voyager introspect` | `v0.6.0` | 🚧 Planned |
| **Phase 7: Production GA Release** | Multi-registry publishing with OIDC provenance (Documentation site to follow post-Phase 7) | `v1.0.0` | 🚧 Planned |

### Phase 1: Core Read AST Engine (Status: Completed)

- [x] **Task 1.1:** Create workspace tooling with `Cargo`, `uv`, `maturin`, and `justfile`.
- [x] **Task 1.2:** Build AST representation and handle-based memory management (`QueryAstArena`).
- [x] **Task 1.3:** Build query emitters for openCypher, SQL:2023 PGQ, and ISO GQL.
- [x] **Task 1.4:** Create 18 golden snapshot tests with `insta`.
- [x] **Task 1.5:** Build Python SDK with model decorators (`@node`, `@relationship`) and type annotations.

### Phase 2: Mutations, Ingestion, Streaming & Bridging (Status: Completed)

- [x] **Task 2.1:** Zero-copy Apache Arrow and Polars stream ingestion.
- [x] **Task 2.2:** Two-layer transaction Unit-of-Work with in-memory savepoint rollbacks.
- [x] **Task 2.3:** DML mutation AST nodes and dialect emitters (`CREATE`, `MERGE`, `SET`, `DELETE`, `DETACH DELETE`).
- [x] **Task 2.4:** Bulk ingestion engine (`UNWIND $batch` and Polars DataFrame importer).
- [x] **Task 2.5:** Database driver evaluation and query dispatch bridging layer (Neo4j, Memgraph, DuckDB, Mock).

### Phase 3: Multi-Dialect Syntax Conformance & Integration Matrix (Status: Completed)

- [x] **Task 3.1:** openCypher & openGQL syntax conformance tests (based on official openCypher and ISO GQL specifications).
- [x] **Task 3.2:** SQL:2023 PGQ & DuckPGQ syntax tests for `GRAPH_TABLE` standard emission and in-memory DuckDB Polars extraction.
- [x] **Task 3.3:** Apache AGE regression tests for PostgreSQL embedded Cypher (`cypher()`) execution and `agtype` mappings.
- [x] **Task 3.4:** Automated local multi-engine live integration matrix (`just test-matrix` across Neo4j 5.26, Memgraph, Apache AGE, DuckDB & DuckPGQ, PostgreSQL 19 Beta 3, and FalkorDB).

### Phase 4: Python Integrations & Query Optimizers (Status: In Progress)

- [x] **Task 4.1:** SQLAlchemy hybrid bridge connecting relational models with graph traversals (`HybridSession`, `as_cte()`, `sync_table_to_graph`).
- [x] **Task 4.2:** Interactive Graph Viewer Widget (`voyager_ogm.viewer` supporting Marimo, VS Code interactive `.ipynb` notebooks, and standalone HTML).
- [x] **Task 4.3:** [Experimental] Multi-Database Batch Identity Map & Active Record Data Mapper Fusion (`Session.flush()`, `Node.save()`, `weakref` memory management).
- [ ] **Task 4.4:** AST Rule-Based Query Optimizer & Predicate Pushdown Pass (`(p:Person {city: $p0})` inline pattern pushdown).

### Phase 5: TypeScript SDK (`@voyager-ogm/core`) (Status: Planned)

- [ ] **Task 5.1:** TypeScript NAPI-RS native bindings for the core AST engine.
- [ ] **Task 5.2:** TypeScript `@Node()` and `@Relationship()` decorators with full type safety.
- [ ] **Task 5.3:** Type-safe fluent query builder and columnar Arrow hydration in Bun, Deno, and Node.js.
- [ ] **Task 5.4:** Automated Bun test conformance suite and benchmarks.

### Phase 6: Voyager CLI Development (Status: Planned)

- [ ] **Task 6.1:** Standalone CLI tool (`voyager-cli`) built with `clap`.
- [ ] **Task 6.2:** `voyager compile`: (sqlc-style) Compiles `.cypher` / `.gql` queries into type-safe models and async functions.
- [ ] **Task 6.3:** `voyager migrate`: Manages in-graph schema migrations (constraints, indexes, labels, Graph Types).
- [ ] **Task 6.4:** `voyager introspect`: Scans live database catalogs to generate model classes and query functions.

### Phase 7: Production GA Release & Multi-Registry Publishing (Status: Planned)

- [ ] **Task 7.1:** Automated multi-registry publishing to PyPI, Crates.io, and npm with trusted OIDC provenance.

> [!NOTE]
> A comprehensive documentation website and multi-language interactive tutorials will be developed after the Phase 7 GA release.

---

## Local Development and Code Usage

To build and run Voyager OGM locally from source:

### Step 1: Build the Local Python Extension

```bash
uv run maturin develop
```

### Step 2: Define Models with Python Type Hints

```python
from voyager_ogm import Node, Relationship, Query, node, relationship


@node(label="Person")
class Person:
    name: str
    age: int
    city: str = "London"


@relationship(type_name="WORKS_AT")
class WorksAt:
    since: int = 2024


@node(label="Company")
class Company:
    name: str
```

### Step 3: Build and Compile a Query

```python
p = Person()
c = Company()
w = WorksAt()

query = (
    Query.match(p)
    .to(w)
    .node(c)
    .where(p.age >= 21, c.name == "Acme Corp")
    .return_(p.name, p.age, company_name=c.name)
    .order_by(p.name)
    .limit(10)
)

# 1. Compile to openCypher (Neo4j, Memgraph, TigerGraph)
cypher = query.compile("cypher")
print(cypher.statement)
# MATCH (_person_0:Person)-[_worksat_0:WORKS_AT]->(_company_0:Company)
# WHERE (_person_0.age >= $p0) AND (_company_0.name = $p1)
# RETURN _person_0.name, _person_0.age, _company_0.name AS company_name
# ORDER BY _person_0.name ASC LIMIT 10

# 2. Compile to SQL:2023 PGQ (DuckDB, PostgreSQL 19)
pgq = query.compile("sql_pgq", graph_name="corp_graph")
print(pgq.statement)

# 3. Compile to ISO/IEC 39075:2024 GQL
gql = query.compile("iso_gql")
print(gql.statement)
```

---

## Zero-Copy Polars Streaming

Voyager OGM streams graph data into Polars without Python object conversion overhead.

```python
import polars as pl
from voyager_ogm import generate_synthetic_stream, to_polars

# 1. Receive native Arrow stream from query engine
stream = generate_synthetic_stream(100_000)

# 2. Load directly into Polars in less than 2 milliseconds
df = to_polars(stream)

# 3. Run analytical queries
stats = (
    df.lazy()
    .filter(pl.col("active"))
    .group_by("label")
    .agg(
        pl.len().alias("total_nodes"),
        pl.col("age").mean().alias("avg_age"),
    )
    .collect()
)
print(stats)
```

---

## Multi-Database Compatibility & Testing

> [!NOTE]
> **Active Development Status:**
> Voyager OGM is actively under development. Query compilation and database bridging are continuously verified against live database instances.

Voyager is verified against 6 database engines:

| Database Engine          | Dialect / Standard              | Connection / Transport             |  Status   |
| :----------------------- | :------------------------------ | :--------------------------------- | :-------: |
| **Neo4j 5.26**           | openCypher / Cypher 25 _(Exp.)_ | Bolt (`bolt://localhost:7687`)     | ✅ Tested |
| **Memgraph**             | openCypher / ISO GQL            | Bolt (`bolt://localhost:7688`)     | ✅ Tested |
| **Apache AGE**           | Cypher-in-SQL                   | PostgreSQL (`host=localhost:5455`) | ✅ Tested |
| **DuckDB & DuckPGQ** | SQL:2023 / SQL:2023 PGQ (`GRAPH_TABLE`) | In-Memory / Extension | ✅ Tested |
| **PostgreSQL 19 Beta 3** | Relational / Recursive SQL      | PostgreSQL (`host=localhost:5456`) | ✅ Tested |
| **FalkorDB**             | openCypher                      | Native Client (`port: 6379`)       | ✅ Tested |

All automated test suites and test cases can be inspected directly in the test directories:

- Python integration & conformance suites: [`packages/python/tests/`](packages/python/tests/)
- Rust core AST & dialect snapshots: [`crates/voyager-core/tests/`](crates/voyager-core/tests/)

---

## Developer Commands

Voyager OGM uses [`just`](https://github.com/casey/just) as a command runner for automated development tasks.

### Install `just`

If you do not have `just` installed, install it for your system from the [official installation page](https://github.com/casey/just#installation):

```bash
# Via Rust Cargo
cargo install just

# Via Windows (Winget or Scoop)
winget install Casey.Just
# or: scoop install just

# Via macOS / Linux (Homebrew)
brew install just
```

### Available Development Recipes

Run tasks with `just`:

```bash
# Setup the local development environment
just setup

# Verify environment and toolchain
just doctor

# Format all code (Rust, Python, TypeScript)
just fmt

# Run all linters
just lint

# Run all test suites
just test

# Run Rust and Python code examples
just examples

# Run full continuous integration check
just ci
```

---

## License

Dual-licensed under either:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
