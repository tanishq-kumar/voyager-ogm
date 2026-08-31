# Changelog

## [0.2.0-beta.1] - 2026-08-31

### Added
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
  - Added cross-platform GitHub Actions CI workflow with matrix testing (Ubuntu, macOS, Windows).

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
  - PyO3 C-extensions bridging Rust core with Python 3.10+.
  - Typed entity models: `@node`, `@relationship`, `Node`, `Relationship`, and `Field[T]` descriptors.
  - Constructor auto-aliasing (`p = Person()` -> `_person_0`) and fluent query compiler.
