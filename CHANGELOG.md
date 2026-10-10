# Changelog

## [0.4.0-alpha.6] - 2026-10-02

### Added
- **Vector Index DDL, VectorProperty & Vector Search Engine (#112)**:
  - Added declarative `VectorProperty(dimensions, similarity, index_name)` descriptor to `voyager_ogm.models` and exported in `voyager_ogm`.
  - Implemented multi-dialect vector index DDL compilation in `voyager-core`:
    - **Neo4j 5+**: `CREATE VECTOR INDEX {index_name} IF NOT EXISTS FOR (n:{label}) ON (n.{prop}) OPTIONS {indexConfig: {`vector.dimensions`: {dims}, `vector.similarity_function`: '{sim}'}}` and `DROP INDEX {index_name} IF EXISTS`.
    - **FalkorDB**: `CREATE VECTOR INDEX FOR (n:{label}) ON (n.{prop}) OPTIONS {dimension: {dims}, similarityFunction: '{sim}'}` and `DROP VECTOR INDEX FOR (n:{label}) ON (n.{prop})`.
    - **Apache AGE / PostgreSQL (pgvector)**: `CREATE INDEX IF NOT EXISTS {index_name} ON ag_catalog."{label}" USING hnsw ({prop} {opclass});` and `DROP INDEX IF EXISTS {index_name};`.
  - Added full schema lifecycle support via `Session.apply_schema()`, `AsyncSession.apply_schema()`, and `SchemaManager.apply_schema()` with automatic model discovery from `SchemaRegistry.global_registry()` when models are omitted.
  - Added vector search query builder `Query.vector_search(index_or_model, query_vector, k=10, yield_node="node", yield_score="score")` and convenience execution wrappers `Session.vector_search()` and `AsyncSession.vector_search()`.
- **Temporal Types Normalization, Timezone Semantics & Dialect Roundtrips (#115)**:
  - Added Bolt wire protocol PackStream typed structures for all 7 temporal specifications (`BoltDate`, `BoltTime`, `BoltLocalTime`, `BoltDateTime`, `BoltLocalDateTime`, `BoltDateTimeZoneId`, `BoltDuration`) in `voyager-net`.
  - Implemented Howard Hinnant Gregorian calendar algorithms without external dependencies for $O(1)$ day-to-YMD and YMD-to-day calculations.
  - Implemented bidirectional mapping in `voyager_ogm.types` with sub-millisecond precision, RFC 9557 timezone handling, instant preservation, and ISO-8601 duration parser/formatter (`parse_iso_duration`, `to_iso_duration`).
  - Added dialect parameter adaptation for FalkorDB and Apache AGE.
- **Geospatial Point Types & Spatial Distance Engine (#114)**:
  - Added Bolt wire protocol PackStream encoding and decoding for `Point2D` (`0x58`) and `Point3D` (`0x59`) with standard SRIDs (`4326` WGS-84 2D, `4979` WGS-84 3D, `7203` Cartesian 2D, `9157` Cartesian 3D).
  - Added `Point` class in `voyager_ogm.types` supporting geographic and Cartesian coordinates, in-memory spherical Haversine and Euclidean distance calculations, PyArrow/Polars conversion, and `to_dict()`/`from_dict()`.
  - Added spatial query expressions `Point.distance_to()`, `BoundField.distance_to()`, `Field.distance_to()`, `fn.point()`, `fn.point.distance()`, and `fn.distance()`.
  - Added cross-dialect spatial function compilation: normalized to `point.distance()` for openCypher and ISO GQL, and `ST_Distance()` for SQL/PGQ.
  - Added automatic model hydration for `Point` fields on `Node` and `Relationship` entities.

### Removed (Breaking Changes)
- **Legacy Query Builder & Comprehension Aliases (#131)**:
  - Removed redundant `Query.add_*` aliases (`add_match`, `add_optional_match`, `add_create`, `add_merge`, `add_unwind`, `add_load_csv`) in favor of direct fluent methods (`match`, `optional_match`, `create`, `merge`, `unwind`, `load_csv`).
  - Removed `Query.filter` alias on `Query` class in favor of `Query.where`.
  - Updated comprehension parameter `where_filter` to `where` across `list_comprehension()` and `pattern_comprehension()`.
  - Removed module-level free functions `unwind()` and `load_csv()` from `voyager_ogm` and `voyager_ogm.query`. Use `Query.unwind()` and `Query.load_csv()`.

---

## [0.4.0-alpha.5] - 2026-09-07

### Added
- **Rich Expression, Arithmetic & Function AST Engine (`voyager-core`)**:
  - Added support for string functions (`toLower`, `toUpper`, `trim`, `split`), scalar functions (`coalesce`, `size`), arithmetic operators (`+`, `-`, `*`, `/`, `%`), and conditional `CASE WHEN` constructs.
  - Added temporal function expressions (`datetime()`) and list/pattern comprehensions.
- **Branching Topologies & Subqueries**:
  - Added support for branching graph diamond patterns (`MATCH p1, p2`), existential subqueries (`WHERE EXISTS (...)`), and scalar aggregations (`COUNT(...)`).
- **Batched FFI Serialization Layer (`voyager-pyo3`)**:
  - Added `compile_query_from_spec` single-trip AST handoff across the PyO3 boundary, yielding a 3.0x speedup in cross-language query compilation.
- **AST Rule-Based Query Optimizer**:
  - Implemented predicate pushdown pass into inline pattern constraints (`(p:Person {city: $p0})`), dead variable pruning, and constant folding.
- **Multi-Database Batch Identity Map & Active Record Data Mapper**:
  - Added unit of work identity map with weakref lifecycle management (`Session.flush()`, `Node.save()`).

---

## [0.3.0-alpha.1] - 2026-09-03

### Added
- **7-Engine Multi-Database Live Integration Matrix (Task 3.4)**:
  - Created automated live multi-engine integration test harness (`packages/python/tests/test_live_matrix.py`) accessible via `just test-matrix`.
  - **Neo4j 5.26 (Bolt 7687):** Schema constraint DDL, UNWIND bulk batch ingestion, fluent traversal queries, zero-copy Polars DataFrame extraction, SET mutations.
  - **Memgraph (Bolt 7688):** Live graph seeding, variable-length path traversals (1..2 hops), zero-copy Polars ingestion.
  - **Apache AGE (PostgreSQL 5455):** Dynamic graph catalog creation, `AgeEmitter` Cypher-in-SQL table execution with JSON parameter maps (`%s`), `agtype` composite projection.
  - **DuckDB (In-Memory Relational):** Relational graph property tables, multi-table joins, and direct `.pl()` Polars streaming. *(Note: Live DuckPGQ extension `GRAPH_TABLE` execution was skipped in CI due to unsigned extension loading; true live execution and scalar function alignment is tracked in #58).*
  - **PostgreSQL 19 Beta 3 (Port 5456):** Relational graph schema, multi-hop recursive graph path traversals (`WITH RECURSIVE`), zero-copy Polars ingestion.
  - **FalkorDB (Port 6379):** Native `falkordb` client execution, low-latency Cypher traversals, parameter mapping, Polars extraction.
- **Apache AGE & PostgreSQL Embedded Cypher Conformance (Task 3.3)**:
  - Implemented `AgeEmitter` wrapping Cypher AST in PostgreSQL `SELECT * FROM cypher('graph', $$ ... $$, %s) AS (...)`.
  - Added `agtype` composite type projection mappings for entities, properties, and expressions.
  - Implemented `SchemaManager` for automated openCypher constraints, B-Tree indexes, and experimental Cypher 25 / ISO GQL Graph Types (`ALTER CURRENT GRAPH TYPE`).
  - Added modern openCypher operator chaining (`!=`, `.in_()`, `.not_in()`, `.startswith()`, `.endswith()`, `.contains()`).
- **SQL:2023 PGQ & DuckPGQ Conformance Suite (Task 3.2)**:
  - Validated ISO/IEC 9075-16:2023 Part 16 standard `GRAPH_TABLE` nested projections, `IS Label`, `{min,max}` quantifier brackets, and `COLUMNS(...)` syntax via compiler AST string emission and snapshot tests.
  - Verified `GRAPH_TABLE` query structure composability inside CTEs (`WITH ... AS (...)`) and direct subquery `JOIN`s with relational SQL tables.
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
  - Multi-dialect mutation emission across openCypher (`CREATE`, `MERGE ON CREATE/MATCH SET`, `SET`, `DETACH DELETE`) and ISO GQL (`INSERT`, `UPSERT`, `SET`, `DELETE`); SQL:2023 PGQ read-only compliance.
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
