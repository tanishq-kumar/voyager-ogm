# Changelog

## [0.3.0-alpha.1] - 2026-09-03

### Added
- **7-Engine Multi-Database Live Integration Matrix (Task 3.4)**:
  - Created automated live multi-engine integration test harness (`packages/python/tests/test_live_matrix.py`) accessible via `just test-matrix`.
  - **Neo4j 5.26 (Bolt 7687):** Schema constraint DDL, UNWIND bulk batch ingestion, fluent traversal queries, zero-copy Polars DataFrame extraction, SET mutations.
  - **Memgraph (Bolt 7688):** Live graph seeding, variable-length path traversals (1..2 hops), zero-copy Polars ingestion.
  - **Apache AGE (PostgreSQL 5455):** Dynamic graph catalog creation, `AgeEmitter` Cypher-in-SQL table execution with JSON parameter maps (`%s`), `agtype` composite projection.
  - **DuckDB (In-Memory Relational):** Relational graph property tables, multi-table joins, direct `.pl()` Polars streaming.
  - **DuckDB DuckPGQ Extension:** Live `CREATE PROPERTY GRAPH` DDL, formal SQL:2023 `GRAPH_TABLE` execution with quantifier repetition, direct Polars extraction.
  - **PostgreSQL 19 Beta 3 (Port 5456):** Relational graph schema, multi-hop recursive graph path traversals (`WITH RECURSIVE`), zero-copy Polars ingestion.
  - **FalkorDB (Port 6379):** Native `falkordb` client execution, low-latency Cypher traversals, parameter mapping, Polars extraction.
- **Apache AGE & PostgreSQL Embedded Cypher Conformance (Task 3.3)**:
  - Implemented `AgeEmitter` wrapping Cypher AST in PostgreSQL `SELECT * FROM cypher('graph', $$ ... $$, %s) AS (...)`.
  - Added `agtype` composite type projection mappings for entities, properties, and expressions.
  - Implemented `SchemaManager` for automated openCypher constraints, B-Tree indexes, and experimental Cypher 25 / ISO GQL Graph Types (`ALTER CURRENT GRAPH TYPE`).
  - Added modern openCypher operator chaining (`!=`, `.in_()`, `.not_in()`, `.startswith()`, `.endswith()`, `.contains()`).
- **SQL:2023 PGQ & DuckPGQ Conformance Suite (Task 3.2)**:
  - Validated ISO/IEC 9075-16:2023 Part 16 standard `GRAPH_TABLE` nested projections, `IS Label`, `{min,max}` quantifier brackets, and `COLUMNS(...)` syntax.
  - Verified `GRAPH_TABLE` composability inside CTEs (`WITH ... AS (...)`) and direct subquery `JOIN`s with relational SQL tables.
- **openCypher & openGQL Standard TCK Harness (Task 3.1)**:
  - Integrated official openCypher and openGQL TCK specifications across all 18 standard query categories and 5 query authoring styles.
  - 100% test pass rate across Rust `cargo-nextest` and Python `pytest` suites.

---

## [0.2.0-alpha.1] - 2026-09-01

### Added
- **Database Bridging System & Driver Adapters (Task 2.5)**:
  - Core Rust `DatabaseBridge` trait, `QueryResult`, `QuerySummary`, and `MockDatabaseBridge` in `voyager-core`.
  - Python `DatabaseBridge` and `AsyncDatabaseBridge` runtime protocols.
  - Built-in adapters: `Neo4jBoltBridge`, `AsyncNeo4jBoltBridge` (Bolt protocol for Neo4j, Memgraph, FalkorDB), `DuckDbBridge`, `AsyncDuckDbBridge` (zero-copy Polars integration), `MockBridge`, and `AsyncMockBridge`.
  - Dynamic `register_bridge()` and `create_bridge()` factory for third-party driver auto-detection.
  - `Session` and `AsyncSession` execution (`execute()`, `execute_to_polars()`, `run_bulk()`).
  - Live integration test suite verifying real Neo4j, DuckDB, official LDBC Social Network, and Canonical Movie Graph datasets.
  - Vendor-neutral container stack (`containers/compose.yaml`) supporting Neo4j, Memgraph, Apache AGE, and FalkorDB.
  - Enforced strict Google-style docstrings across all Python modules via Ruff rule `D` (`pydocstyle`).
- **High-Throughput Bulk Ingestion Engine (Task 2.4)**:
  - Added `AstNode::UnwindClause` and `AstNode::Parameter` for `UNWIND $batch AS row` unrolling.
  - Implemented `compile_bulk_create`, `compile_bulk_merge`, and `compile_bulk_create_rel` in `voyager_core::bulk`.
  - Added zero-copy `chunk_dataframe(df, batch_size=50_000)` and `chunk_records()` supporting Polars `pl.DataFrame`, PyArrow Tables, and Pandas.
  - Implemented `Session.bulk_create()`, `Session.bulk_upsert()`, and `Session.bulk_create_relationships()`.
  - Added `Query.unwind(batch_param, alias)` fluent builder method.
- **DML Mutation AST Nodes & Emitters (Task 2.3)**:
  - Added AST mutation variants: `CreateClause`, `MergeClause`, `SetClause`, `SetItem`, `DeleteClause`, `RemoveClause`.
  - Multi-dialect mutation emission across openCypher (`CREATE`, `MERGE ON CREATE/MATCH SET`, `SET`, `DETACH DELETE`), ISO GQL (`INSERT`, `UPSERT`, `SET`, `DELETE`), and SQL:2023 PGQ.
  - In-code dirty property tracking on `Node` models (`.dirty_fields`, `.clear_dirty()`, automatic delta mutation).
- **Two-Layer Rollback Unit-of-Work (Task 2.2)**:
  - Implemented `Transaction` and `UnitOfWork` with automatic rollback of dirty memory arena handles and entity state.
  - Added nested savepoints (`savepoint()`, `rollback_to_savepoint()`, `release_savepoint()`).
  - Added Python context managers: `with session.transaction() as tx:` and `with tx.savepoint("sp"):`.
- **Zero-Copy Arrow & Polars Streaming (Task 2.1)**:
  - Implemented Arrow `RecordBatch` columnar graph batch builder in `crates/voyager-core/src/arrow.rs`.
  - Exported `__arrow_c_stream__` PyCapsule interface via `voyager-pyo3`.
  - Added `.to_polars()` and `.to_arrow()` methods (1,000,000 nodes streamed in 9.08 ms).
- **Dual Licensing & CI Automation**:
  - Added formal dual-license pointer (`LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE`).
  - Pinned GitHub Actions in CI workflow to exact 40-character commit SHAs.
  - Added Python 3.14 to test matrix and automated Dependabot configuration.

---

## [0.1.0-alpha.1] - 2026-08-25

### Added
- **Core AST Engine (`voyager-core`)**:
  - Contiguous 32-bit handle memory arena (`QueryAstArena`) with 4-byte `NodeHandle(u32)` indices.
  - Core AST nodes: `NodePattern`, `EdgePattern`, `PathChain`, `BinaryExpression`, `LiteralValue`, `ReturnClause`, `ProcedureCall`.
  - Fluent builder API (`QueryBuilder`) with type-safe method chaining.
- **Multi-Dialect AST Query Emitters**:
  - `CypherEmitter`: openCypher parameterization (`$p0, $p1`).
  - `SqlPgqEmitter`: SQL:2023 Part 16 `GRAPH_TABLE` nested projection syntax.
  - `IsoGqlEmitter`: ISO/IEC 39075:2024 GQL standard syntax.
- **Golden Snapshot Regression Suite**:
  - Integrated `insta` golden snapshot testing across 18 real-world graph query patterns (LDBC Social Network, Movie Graph, aggregations, APOC procedure calls).
- **Python SDK (`voyager-ogm`)**:
  - PyO3 C-extensions bridging Rust core with Python 3.11+.
  - Typed entity models: `@node`, `@relationship`, `Node`, `Relationship`, and `Field[T]` descriptors.
  - Constructor auto-aliasing (`p = Person()` -> `_person_0`) and fluent query compiler.
