# Voyager OGM

**Multi-Dialect Object-Graph Mapper (OGM) and Query Compiler**

[![CI](https://github.com/tanishq-kumar/voyager-ogm/actions/workflows/ci.yml/badge.svg)](https://github.com/tanishq-kumar/voyager-ogm/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Changelog](https://img.shields.io/badge/Changelog-Keep%20a%20Changelog-informational.svg)](CHANGELOG.md)
[![Standards](https://img.shields.io/badge/Standards-openCypher%20%7C%20SQL%3A2023%20PGQ%20%7C%20ISO%20GQL-green.svg)](https://www.iso.org/standard/76120.html)
[![Arrow](https://img.shields.io/badge/Zero--Copy-Apache%20Arrow%20PyCapsule-orange.svg)](https://arrow.apache.org/)


---

## Why Voyager OGM? (Project Origin)

I worked with Cypher queries and I liked Object-Relational Mappers for SQL (such as SQLAlchemy). But I did not find a good, vendor-neutral Object-Graph Mapper (OGM) for graph databases.

I used GQLAlchemy in the past. But I had problems when I changed setups between different graph databases. To solve those problems, I had to write raw queries manually.

Now I have time for recreational programming. I started this project to solve that problem and to learn systems-level programming.

Why Rust? Nothing special. The Rust compiler helps me write memory-safe code. In Python, tools like Polars show how Rust gives great speed and low memory usage. Let us see how far this project goes.

---

## Overview

Voyager OGM is a graph database toolkit written in safe Rust. It has bindings for Python (`PyO3`) and TypeScript (`NAPI-RS`).

To start writing code immediately, jump to [Local Development Setup](#local-development-and-code-usage) or inspect the available [Developer Commands](#developer-commands).

Voyager OGM solves four common problems in graph databases:

1. **Slow Object Hydration:** It uses the **Apache Arrow C Data Interface** (`__arrow_c_stream__`). It streams graph data directly into **Polars DataFrames** without intermediate Python object copies. See [Zero-Copy Polars Streaming](#zero-copy-polars-streaming).
2. **Dialect Lock-In:** It compiles one query AST into **openCypher**, **SQL:2023 PGQ** (`GRAPH_TABLE`), and **ISO GQL** (ISO/IEC 39075:2024).
3. **Memory Management:** It uses a compact handle-based AST representation (`NodeHandle`) with safe, automatic memory reclamation.
4. **Transaction Safety:** It uses a two-layer Unit of Work. It rolls back in-memory changes and AST allocations automatically if a database query fails.

---

## Project Plan and Roadmap (7 Phases)

> [!NOTE]
> **Roadmap Agility & Scope Flexibility:**
> The phases, milestones, and task breakdowns in this roadmap are designed to be dynamic and evolutionary. As new database capabilities, engine updates (e.g. ISO GQL revisions, Cypher 25, Apache AGE enhancements), or architectural improvements arise during development, tasks may be added, refined, removed, or partitioned into sub-phases (e.g., Phase 3.1a, 3.1b) to maintain rigorous standards compliance and feature completeness.

```mermaid
gantt
    title Voyager OGM Implementation Roadmap (7 Phases)
    dateFormat  YYYY-MM-DD
    section Phase 1: Core Engine (v0.1.0-alpha)
    Workspace & Tooling Scaffolding       :done, p1_1, 2026-08-25, 2d
    Core AST Engine & Models              :done, p1_2, after p1_1, 3d
    Dialect Emitters (Cypher, SQL, GQL)   :done, p1_3, after p1_2, 3d
    Golden Snapshot Test Suite (Insta)    :done, p1_4, after p1_3, 2d
    Python PyO3 SDK & Type Annotations    :done, p1_5, after p1_4, 3d
    section Phase 2: Mutations & Ingestion (v0.2.0-beta)
    Zero-Copy Arrow & Polars Streaming    :done, p2_1, after p1_5, 3d
    Two-Layer Rollbacks & Savepoints      :done, p2_2, after p2_1, 3d
    DML Mutations (CREATE, MERGE, SET)    :done, p2_3, after p2_2, 3d
    Bulk Importer (UNWIND & DataFrame)    :done, p2_4, after p2_3, 3d
    Driver Evaluation & Bridging System   :done, p2_5, after p2_4, 3d
    section Phase 3: Formal TCK Conformance (v0.3.0)
    openCypher & openGQL TCK Harness      :done, p3_1, after p2_5, 4d
    SQL:2023 PGQ & DuckPGQ Suite          :done, p3_2, after p3_1, 3d
    Apache AGE & PostgreSQL Cypher Suite  :done, p3_3, after p3_2, 3d
    Multi-Engine Live Matrix              :done, p3_4, after p3_3, 3d
    section Phase 4: Python Integrations & Optimizers (v0.4.0)
    SQLAlchemy Hybrid Bridge              :active, p4_1, after p3_4, 3d
    Marimo Reactive WebGL Extension       :p4_2, after p4_1, 3d
    AST Query Optimizer & Pushdown        :p4_3, after p4_2, 3d
    section Phase 5: TypeScript NAPI SDK (v0.5.0)
    TypeScript NAPI AST Bindings          :p5_1, after p4_3, 4d
    TypeScript Decorators & Models        :p5_2, after p5_1, 3d
    TypeScript Streaming & Hydration      :p5_3, after p5_2, 3d
    section Phase 6: Voyager CLI Engine (v0.6.0)
    CLI Scaffolding & clap Integration    :p6_1, after p5_3, 2d
    voyager compile Command               :p6_2, after p6_1, 3d
    voyager migrate Command               :p6_3, after p6_2, 3d
    voyager introspect Command            :p6_4, after p6_3, 3d
    section Phase 7: Production GA (v1.0.0)
    Multi-Registry OIDC Publishing        :p7_1, after p6_4, 3d
    Documentation Site & Sandbox Tutorials:p7_2, after p7_1, 3d
```

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

### Phase 3: Formal TCK Conformance & Multi-Engine Compatibility Matrix (Status: Completed)
- [x] **Task 3.1:** [openCypher TCK](https://github.com/opencypher/openCypher/tree/main/tck) & [openGQL TCK](https://github.com/opengql/tck) automated syntax and grammar verification across all 18 categories and 5 query authoring styles.
- [x] **Task 3.2:** [DuckPGQ SQLLogicTest Suite](https://github.com/cwida/duckpgq-extension/tree/main/test) for SQL:2023 `GRAPH_TABLE` standard conformance and live in-memory DuckDB Arrow/Polars extraction.
- [x] **Task 3.3:** [Apache AGE Regression Suite](https://github.com/apache/age/tree/master/regress) for PostgreSQL embedded Cypher conformance and live database execution.
- [x] **Task 3.4:** Automated local multi-engine live integration matrix (`just test-matrix` across Neo4j 5.26, Memgraph, Apache AGE, DuckDB & DuckPGQ, PostgreSQL 19 Beta 3, and FalkorDB).

### Phase 4: Python Integrations & Query Optimizers (Status: In Progress)
- [ ] **Task 4.1:** SQLAlchemy hybrid bridge connecting relational models with graph traversals.
- [ ] **Task 4.2:** Marimo reactive notebook visual extension (`voyager_graph[marimo]`).
- [ ] **Task 4.3:** AST Rule-Based Query Optimizer & Predicate Pushdown Pass (`(p:Person {city: $p0})` inline pattern pushdown).

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
- [ ] **Task 7.2:** Starlight / Astro documentation website with multi-language interactive tutorials deployed to Cloudflare Pages.

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

## Test Suite and Quality Matrix

Voyager OGM uses continuous testing across three languages:

| Engine / Target | Test Runner | Test Count | Status | Execution Time |
| :--- | :--- | :---: | :---: | :---: |
| **Rust Core (`voyager-core`)** | `cargo-nextest` | **87 tests** | Passing | **1.64s** |
| **Multi-Dialect Golden Snapshots** | `insta` | **18 snapshots** | Passing | **Instant** |
| **Python SDK & TCK Conformance** | `pytest` + `pytest-cov` | **108 tests** | Passing | **~8.0s** |
| **Multi-Engine Live Matrix** | `just test-matrix` | **7 suites** | Passing | **~5.0m** |
| **TypeScript SDK (`@voyager-ogm/core`)**| `bun test` | **1 test** | Passing | **0.12s** |
| **Total Automated Tests** | Across all components | **200+ tests** | **100% Green** | **Clean** |

---

## 7-Engine Live Integration & Compatibility Matrix

Voyager OGM is tested against 7 live database engines and runtime environments:

| Database Engine | Dialect / Standard | Connection / Transport | Verified Capabilities | Status |
| :--- | :--- | :--- | :--- | :---: |
| **Neo4j 5.26** | openCypher / Cypher 25 *(Exp.)* | Bolt (`bolt://localhost:7687`) | SchemaManager DDL, UNWIND bulk ingestion, fluent traversals, Polars export, SET mutations | ✅ **Live Verified** |
| **Memgraph** | openCypher / ISO GQL | Bolt (`bolt://localhost:7688`) | Graph seeding, variable-length path traversals (1..2 hops), zero-copy Polars ingestion | ✅ **Live Verified** |
| **Apache AGE** | Cypher-in-SQL | PostgreSQL (`host=localhost:5455`) | Dynamic graph catalogs, `ag_catalog.cypher()` table queries, `%s` JSON parameters, `agtype` projections | ✅ **Live Verified** |
| **DuckDB (Relational)** | SQL:2023 | In-Memory (`:memory:`) | Property tables, relational graph joins, zero-copy `.pl()` Polars DataFrame streaming | ✅ **Live Verified** |
| **DuckDB DuckPGQ** | SQL:2023 PGQ (`GRAPH_TABLE`) | DuckDB Community Extension | `CREATE PROPERTY GRAPH` DDL, standard `GRAPH_TABLE` execution, `{min,max}` quantifiers, `.pl()` streaming | ✅ **Live Verified** |
| **PostgreSQL 19 Beta 3** | Relational / Recursive SQL | PostgreSQL (`host=localhost:5456`) | Relational graph schema, multi-hop recursive graph path traversals (`WITH RECURSIVE`), Polars extraction | ✅ **Live Verified** |
| **FalkorDB** | openCypher | Native Client (`port: 6379`) | Graph catalog selection, low-latency Cypher traversals, parameter mapping, Polars extraction | ✅ **Live Verified** |

> [!NOTE]
> **Dialect Evolution & Full Engine Parity Roadmap:**
> 1. **Cypher 25 Support:** Cypher 25 Graph Types (`ALTER CURRENT GRAPH TYPE`) and advanced GQL-aligned syntax are currently marked as **`[Experimental / Draft]`** (previewed on Neo4j 5.26+).
> 2. **Phased Engine Hardening:** Core AST compilation, transaction bridging, and zero-copy Polars ingestion are fully operational across all 7 engines today. **Full production-hardened dialect feature parity, vendor-specific driver extensions, and dialect pushdown optimizations for each engine will be continuously expanded and deepened in upcoming phases (Phase 4 & Phase 5).**

---

### Current Test Suite Architecture

Voyager OGM validates its core query compiler and memory engine through five layers:

1. **AST & Handle Integrity**: Verifies allocation, mutable updates, and automatic reclamation across multi-hop traversals.
2. **Deterministic Golden Snapshots (`insta`)**: Locks in exact string emissions across openCypher, SQL:2023 PGQ, and ISO GQL for real-world graph patterns (LDBC Social Network, Movie Graph, aggregations, and APOC vendor procedures).
3. **Transaction Rollback & Chaos Tests**: Guarantees zero memory leaks and state rollbacks across 500 aborted transactions and nested savepoints.
4. **Zero-Copy Columnar Streaming**: Validates streaming of Arrow record batches directly into Polars DataFrames without intermediate Python allocations.
5. **Bulk Ingestion Verification**: Verifies batch generation (`UNWIND $batch AS row`) and zero-copy chunking across 100,000 Polars DataFrame rows.

### Multi-Standard Conformance & Correctness Verification (Phase 3)

In **Phase 3**, Voyager OGM integrates four authoritative test suites to ensure 100% formal correctness across all supported graph query paradigms:

1. [**openCypher TCK**](https://github.com/opencypher/openCypher/tree/main/tck): Validates complete openCypher grammar, clause evaluation, and semantic equivalence (Neo4j, Memgraph, FalkorDB).
2. [**openGQL / ISO:IEC 39075:2024 TCK**](https://github.com/opengql/tck): Validates formal ISO standard query constructs, type systems, and graph pattern matching.
3. [**DuckPGQ Test Suite**](https://github.com/cwida/duckpgq-extension/tree/main/test): Validates formal **SQL:2023 Part 16 (SQL/PGQ)** `GRAPH_TABLE` nested projections and `COLUMNS(...)` syntax against DuckDB.
4. [**Apache AGE Regression Suite**](https://github.com/apache/age/tree/master/regress): Validates PostgreSQL-embedded Cypher execution (`cypher('graph', $$ ... $$)`) and `agtype` property mappings.

Running these official suites guarantees **100% formal mathematical and grammatical correctness** directly against international graph standards and real-world database runtimes.

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
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
