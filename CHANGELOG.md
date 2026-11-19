# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Description Metadata for Fields, Nodes, and Relationships (#210)**:
  - Added optional `description: str | None = None` parameter to `Field()` descriptor and `description` attribute to `Field` instances.
  - Added support for model-level description metadata via `__description__` class attribute on `Node` and `Relationship` models.
  - Carried descriptions through schema registration into native `FieldDescriptor`, `NodeSchema`, and `RelationshipSchema` structs in Rust and exposed them via PyO3 dictionary bindings.
  - Updated schema snapshot serialization to include `"description"` fields in JSON snapshots when present, omitting them when `None` via serde `skip_serializing_if` so existing snapshot JSON remains byte-identical.
  - Ensured backward compatibility: older snapshots without description keys deserialize cleanly via serde default with `description: None`.
- **Bridge Authentication Credentials Parameter (#193)**:
  - Added keyword-only `auth: tuple[str, str] | Any | None = None` parameter to `create_bridge()` across all overloads.
  - Bolt schemes accept `(username, password)` tuples and custom auth tokens (e.g. `bearer_auth`) passed directly to `GraphDatabase.driver` / `AsyncGraphDatabase.driver`.
  - PostgreSQL and Apache AGE schemes accept `(username, password)` tuples passed as `user=` / `password=` connect keyword arguments; invalid types raise `TypeError`.
  - FalkorDB and Redis schemes accept `(username, password)` tuples passed as `username=` / `password=` keyword arguments to `FalkorDB.from_url`.
  - Explicit `auth=` takes precedence over credentials in URI strings.
- **Unified Examples Learning Path, OGM-First Modernization, and Rust Parity (#172)**:
  - Streamlined Python examples into a single progressive beginner-to-advanced learning path (01 to 31) with OGM-first declarative modeling, mutation DSL, and consistent setup and offline skip handling.
  - Added idiomatic Rust examples covering schema DDL and index emission (`08`), mutation builder (`09`), bulk ingestion compilation (`10`), transactions and unit of work (`11`), vector/full-text search query building (`12`), and live engine execution (`live_connect_and_query`).
  - Added new Python examples covering real engine bridges via `create_bridge()` and mock fallbacks, embedded Relate (`relate://`), bulk ingestion, transactions with savepoints and identity map, multi-engine live execution (Memgraph, FalkorDB, Apache AGE), vector and full-text search, mutation DSL, and standalone `AsyncSession` / `HybridSession`.
  - Added `just examples-live` recipe and updated `examples-python` and `examples-rust` to run all offline examples.
- **Embedded Relate Database Bridge (#157)**:
  - Added `RelateBridge` and `AsyncRelateBridge` in `voyager_ogm.bridge`, exported from `voyager_ogm`, for in-process, zero-network execution against embedded Relate databases (`relatedb.ai`).
  - Added auto-instantiation support in `create_bridge()` for `relate://` and `relatedb://` URI schemes and connection instance auto-detection.
  - Added the `relate` optional dependency extra (`pip install "voyager-ogm[relate]"`, `relate-db>=1.0.0`).
  - Added Relate-compatible parameter and result adaptation: spatial `Point` and duration conversion, nested dict/list parameter mapping, and node/relationship record normalization.
  - Added sync and async bulk batch ingestion plus `ping()` liveness and explicit connection close on both bridges.
  - Added a Relate compatibility test suite covering schema creation, CRUD, traversal, aggregation, and parameter round-trips.
- **NetworkX Graph Analytics Export Bridge (#117)**:
  - Added direct, high-performance `.to_networkx()` graph export on `ExecutionResult`, `QueryResult`, and top-level module `voyager_ogm.to_networkx`.
  - Added fluent `session.query(...)` and `async_session.query(...)` builders supporting direct `.where(...).to_networkx()` chaining.
  - Supported directed (`DiGraph`, `MultiDiGraph`) and undirected (`Graph`, `MultiGraph`) graph generation with node/edge attribute filtering and relationship type preservation.
  - Added explicit `node_only=True` escape hatch and `source_col=""` support to disable automatic tabular edge heuristic detection.
  - Added `analytics` optional dependency extra (`pip install "voyager-ogm[analytics]"`).
- **File, S3, and Azure Cloud Ingestion Loaders (#110)**:
  - Added zero-copy, lazy-scanned cloud and local file loaders directly on `Session`, `AsyncSession`, and top-level module APIs (`ingest_parquet`, `ingest_csv`, `ingest_parquet_relationships`, `ingest_csv_relationships`).
  - Added `create_parquet_ingest_plan`, `create_csv_ingest_plan`, `create_parquet_rel_ingest_plan`, and `create_csv_rel_ingest_plan` with support for `"create"` / `"upsert"` modes, field remapping, and multi-cloud authentication via Polars `storage_options`.
- **Session Liveness Probes and Health Monitoring (#118)**:
  - Added `Session.ping()`, `AsyncSession.ping()`, `Session.health()`, and `AsyncSession.health()` for fast backend liveness verification and diagnostics.
  - Added `SessionHealth` diagnostic dataclass capturing roundtrip latency, backend mode, and circuit breaker status.
- **Full-Text Index DDL Generation and Query Integration (#113)**:
  - Added declarative `FullTextIndex(properties, name=None, analyzer="standard")` configuration to `voyager_ogm.schema` and re-exported in `voyager_ogm` (with `TextIndex` and `fulltext_index` developer ergonomics aliases).
  - Added model metadata reflection on `Node` and `Relationship` subclasses via `__fulltext_indexes__`.
  - Implemented multi-dialect full-text index DDL generation and drop statements in `voyager-ogm`:
    - **Neo4j 5+ / openCypher**: `CREATE FULLTEXT INDEX {index_name} IF NOT EXISTS FOR (n:{label}) ON EACH [n.{p1}, ...] OPTIONS {indexConfig: {`fulltext.analyzer`: '{analyzer}'}}` (and `FOR ()-[r:{type}]-() ON EACH [r.{p1}, ...]`) with `DROP INDEX {index_name} IF EXISTS`.
    - **Memgraph (MAGE Tantivy)**: `CREATE TEXT INDEX {index_name} ON :{label_or_type}({props});` for nodes and `CREATE TEXT EDGE INDEX ...` for relationships, with `DROP TEXT INDEX {index_name};`.
    - **DuckDB (fts)**: `PRAGMA create_fts_index('{table}', '{pk}', '{props}');` and `PRAGMA drop_fts_index('{table}');`.
    - **FalkorDB (RediSearch)**: `CREATE FULLTEXT INDEX FOR (n:{label}) ON (n.{p1}, ...)` (and `FOR ()-[r:{type}]-() ON (r.{p1}, ...)`) with matching `DROP FULLTEXT INDEX` statements.
    - **Apache AGE / PostgreSQL (GIN)**: GIN index creation on `to_tsvector` over concatenated properties and `DROP INDEX IF EXISTS {index_name};` (verified via compiled DDL snapshot tests).
  - Centralized canonical full-text index naming authority (`canonical_fulltext_index_name`) in `voyager-ogm`, exposed via PyO3 to Python to prevent dual bookkeeping.
  - Added full-text search query builder `Query.fulltext_search(index_or_model, query_string, yield_node="node", yield_score="score", dialect="cypher")` (with `Query.text_search` alias):
    - openCypher: routes to `db.index.fulltext.queryNodes` and `db.index.fulltext.queryRelationships`.
    - Memgraph: routes to `text_search.search_all` (or `text_search.search` for field queries with automatic Tantivy `data.<field>` property mapping) and `text_search.search_all_edges` (or `text_search.search_edges`).
    - FalkorDB: routes to `db.idx.fulltext.queryNodes` and `db.idx.fulltext.queryRelationships`.
  - Added convenience session facades on `Session.fulltext_search()` / `Session.text_search()` and `AsyncSession.fulltext_search()` / `AsyncSession.text_search()`.
  - Added model search entrypoints `Node.text_search()` / `fulltext_search()` and `Relationship.text_search()` / `fulltext_search()`.
- **Vector Index DDL, VectorProperty & Vector Search Engine (#112)**:
  - Added declarative `VectorProperty(dimensions, similarity, index_name)` descriptor to `voyager_ogm.models` and exported in `voyager_ogm`.
  - Implemented multi-dialect vector index DDL compilation in `voyager-ogm`:
    - **Neo4j 5+**: `CREATE VECTOR INDEX {index_name} IF NOT EXISTS FOR (n:{label}) ON (n.{prop}) OPTIONS {indexConfig: {`vector.dimensions`: {dims}, `vector.similarity_function`: '{sim}'}}` and `DROP INDEX {index_name} IF EXISTS` (with deliberate degradation of `dot` to `cosine`).
    - **Memgraph (v3.2+)**: `CREATE VECTOR INDEX {index_name} ON :{label}({prop}) WITH CONFIG {"dimension": {dims}, "capacity": 10000, "metric": "{metric}"};` and `DROP VECTOR INDEX {index_name};`.
    - **DuckDB (vss)**: `CREATE INDEX IF NOT EXISTS {index_name} ON "{table}" USING HNSW ("{prop}") WITH (metric = '{metric}');` and `DROP INDEX IF EXISTS {index_name};`.
    - **FalkorDB**: `CREATE VECTOR INDEX FOR (n:{label}) ON (n.{prop}) OPTIONS {dimension: {dims}, similarityFunction: '{sim}'}` (with native `ip` inner-product support) and `DROP VECTOR INDEX FOR (n:{label}) ON (n.{prop})`.
    - **Apache AGE / PostgreSQL (pgvector)**: `CREATE INDEX IF NOT EXISTS {index_name} ON ag_catalog."{label}" USING hnsw ({prop} {opclass});` and `DROP INDEX IF EXISTS {index_name};`.
  - Centralized canonical vector index naming authority (`canonical_vector_index_name`) shared between Rust emitters and Python `Query.vector_search` to guarantee exact identifier alignment.
  - Centralized dialect metric resolution on `VectorSimilarity::metric_for(DdlDialect)` in `voyager-ogm`.
  - Added strict dimensions validation in PyO3 raising loud `ValueError` on malformed or non-positive values.
  - Added full schema lifecycle support via `Session.apply_schema()`, `AsyncSession.apply_schema()`, and `SchemaManager.apply_schema()` / `apply_schema_async()` with automatic model discovery from `SchemaRegistry.global_registry()` when models are omitted.
  - Added vector search query builder `Query.vector_search(index_or_model, query_vector, k=10, yield_node="node", yield_score="score", dialect="cypher")` supporting Neo4j (`db.index.vector.queryNodes`) and live-verified 3-argument Memgraph (`vector_search.search`), plus convenience execution wrappers `Session.vector_search()` and `AsyncSession.vector_search()`.
  - Added `parse_field_descriptor` BTree default inference when `index=True` is specified without explicit `index_type`.
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
- **Query Spec Interchange Protocol & Structured Error Taxonomy (#143)**:
  - Added `spec_version` field and JSON Schema specification (`spec/query_spec.v1.schema.json`) for cross-language query interchange validation.
  - Added structured `ErrorCode` enum and unified error taxonomy across Rust core and Python bindings.
  - Added explicit, loud errors on mock bridge fallback in production configurations.

### Changed
- **Native Schema Identity and Key Policy (#200)**: the schema registry now keys entries by module-qualified model identity, so same-named models in different modules get separate schema entries while label and relationship-type lookups keep working. Primary-key and MERGE-key resolution now come from the Rust schema from the registered field descriptors; the Python helpers delegate to the native registry instead of scanning `_schema_fields`; bare-class-name and label/type lookups resolve to the most recently registered model when several match.
- **Breaking:** `session.transaction()` is now a real database transaction, and the in-memory API is `UnitOfWork` / `unit_of_work()` (#177):
  - `session.transaction()` / `async with session.transaction()` begins a real `BEGIN`/`COMMIT`/`ROLLBACK` transaction on Neo4j, Memgraph, and Apache AGE/PostgreSQL through both the Python driver bridges and the native Rust client, and on the embedded DuckDB/DuckPGQ and Relate bridges. Every session execution path (`execute`, `execute_to_polars`, `execute_to_arrow`, `run_bulk`, `flush()`) is routed through the active transaction; a clean exit commits, an exception rolls back and re-raises, and nested transactions raise.
  - Real savepoints (`tx.savepoint(name)`) are supported on Apache AGE/PostgreSQL and raise on Bolt engines (Neo4j/Memgraph have no savepoints), DuckDB and Relate. FalkorDB raises a clear error because Redis `MULTI`/`EXEC` cannot provide rollback or read-your-writes. The real-transaction context manager is exported as `TransactionSavepoint`; the top-level `SavepointContext` remains the in-memory unit-of-work savepoint.
  - A failed statement marks the transaction `FAILED`: further statements and `commit()` raise a clear "transaction is aborted; roll back" error, `rollback()` cleans up without masking the original error, and the context manager re-raises the original exception. This fail-closed behaviour applies to native Bolt/PostgreSQL and the Neo4j/Memgraph/AGE, DuckDB/DuckPGQ and Relate bridges.
  - The in-memory class is renamed `Transaction` -> `UnitOfWork` (module `voyager_ogm.unit_of_work`), exposed via `Session.unit_of_work()` / `AsyncSession.unit_of_work()`; the old deprecated aliases are removed. It now snapshots identity-mapped entities so rollback restores entity values, and warns when the identity map is disabled.
  - Renamed the PyO3 scope class `NativeTransaction` to `NativeUnitOfWorkScope`, and added a native `NativeTransaction` that leases one pooled connection for a real database transaction.
  - With the identity map enabled, a real transaction also opens a `UnitOfWork`, so a database rollback restores tracked entities as well.
- **Examples use the fixed OGM write path (#174, #175)**: removed the `.set(instance)` workarounds after `Query.create(...)` in examples 07, 11, 17, 20 and 31, and changed the MERGE demo in example 07 to a keyed `Query.merge(instance)` upsert.
- Renamed the Rust core crate `voyager-core` to `voyager-ogm` (`voyager_core::` -> `voyager_ogm::`; `RUST_LOG` targets change accordingly) (#171).
- Repository renamed to `tanishq-kumar/voyager`; package names unchanged (#168).
- **Runtime Panic Handling, PyO3 and Toolchain Updates (#158)**:
  - Configured the release profile to unwind instead of abort, allowing Rust panics to surface as catchable Python `PanicException` instead of killing the interpreter.
  - Upgraded PyO3 and `pyo3-async-runtimes` to 0.29.
  - Raised Minimum Supported Rust Version (MSRV) to 1.87.

### Removed
- **Unused ConstraintType Enum (#209)**:
  - Removed unused `ConstraintType` enum (`Unique`, `NotNull`, `NodeKey`) from `voyager_ogm` public API and crate root as constraints are declared via boolean flags on `FieldDescriptor` and `NodeKey` is unsupported on Neo4j Community Edition.
- **Legacy Query Builder & Comprehension Aliases (#131)**:
  - **Breaking:** Removed redundant `Query.add_*` aliases (`add_match`, `add_optional_match`, `add_create`, `add_merge`, `add_unwind`, `add_load_csv`) in favor of direct fluent methods (`match`, `optional_match`, `create`, `merge`, `unwind`, `load_csv`).
  - **Breaking:** Removed `Query.filter` alias on `Query` class in favor of `Query.where`.
  - **Breaking:** Updated comprehension parameter `where_filter` to `where` across `list_comprehension()` and `pattern_comprehension()`.
  - **Breaking:** Removed module-level free functions `unwind()` and `load_csv()` from `voyager_ogm` and `voyager_ogm.query`. Use `Query.unwind()` and `Query.load_csv()`.

### Fixed
- **Native PostgreSQL and Apache AGE Parameter Binding (#222)**:
  - The native PostgreSQL client now binds parameters through the extended query protocol (`Parse` / `Bind` / `Describe` / `Execute` / `Sync`) instead of substituting values into SQL text; parameter values are never interpolated for any dialect.
  - Apache AGE statements bind the `%s` agtype parameter map as positional parameter `$1`, while `$name` references inside the `$$ ... $$` Cypher body are left for AGE to resolve from that map.
  - SQL and SQL/PGQ `$name` placeholders are rewritten to positional `$1`, `$2`, ... in first-appearance order, with repeated names reusing one bound value.
  - Error responses during the extended flow are drained through `ReadyForQuery`, so the connection stays usable and a failed statement inside `BEGIN` reports `FailedTransaction`.
  - Apache AGE bulk `UNWIND $batch` statements now include the third `cypher()` parameter argument even when the batch map is a runtime parameter, so bulk create, bulk upsert, and identity-map flushes work on AGE.
- **CI runs once per opened PR and once per push (CI)**: label and other PR metadata changes no longer trigger CI. The `full-ci` and `skip-changelog` labels are read from the PR at run time instead of from the event payload, so labels attached right after opening are honoured and a manual re-run picks up a label added later. Opening a PR with several labels previously started one run per label; cancelled runs posted a failing `CI Gate` that could leave the PR blocked.
- **PostgreSQL and AGE URI Graph Parameters (#221)**:
  - `create_bridge()` now removes `?graph=` and `?graph_name=` from the connection string before calling `psycopg.connect()` on both the sync and async paths, so real PostgreSQL and `age://` URIs no longer fail with `psycopg.ProgrammingError: invalid URI query parameter`. Every other query parameter is kept unchanged and in order.
  - Graph selection precedence is unchanged: an explicit `graph_name` argument wins over the URI parameter, and `?graph=` wins over `?graph_name=`.
  - The native URI parser no longer treats `?graph=` as the PostgreSQL database name in the startup message; Apache AGE graphs live inside the connected catalog, and `?graph_name=` now also triggers AGE initialization. Redis/FalkorDB `?graph=` continues to select the graph database.
  - `VoyagerError.skipped` is now a declared `list[str]` attribute that exists (empty by default) on every `VoyagerError`, so callers no longer need `getattr(error, "skipped", ...)`.
- **Relationship Unique and Primary-Key Constraint DDL (#211)**:
  - On the Cypher dialect, relationship fields marked `unique=True` or `primary_key=True` now emit a `REQUIRE r.<prop> IS UNIQUE` constraint instead of only an existence constraint, so duplicate values are rejected.
  - Relationship existence (`REQUIRE r.<prop> IS NOT NULL`) constraints are now emitted only when the field is non-nullable, so a nullable unique relationship field is no longer made mandatory.
  - Every emitted relationship constraint now has a matching `DROP CONSTRAINT ... IF EXISTS` statement, using the `unique` and `not_null` kinds of `cypher_rel_constraint_name`.
  - Relationship secondary indexes are still skipped for unique and primary-key fields, since the uniqueness constraint provides its own index.
  - Constraint DDL now emits a field's `UNIQUE` statement before its `NOT NULL` (existence) statement; when Neo4j Community rejects Enterprise-only constraints, the remaining constraints still run and one `VoyagerError` (`SCHEMA_ERROR`) reports every skipped statement after confirming the others were applied.
- **FalkorDB Full-Text Index DDL Syntax and Idempotency (#198)**:
  - Updated FalkorDB full-text index creation and drop DDL emitters to use native declarative Cypher syntax accepted by FalkorDB 6.0.4+: `CREATE FULLTEXT INDEX FOR (n:<label>) ON (n.<prop1>, n.<prop2>)` and `DROP FULLTEXT INDEX FOR (n:<label>) ON (n.<prop1>, n.<prop2>)` for nodes, and `CREATE FULLTEXT INDEX FOR ()-[r:<type>]-() ON (r.<prop1>, ...)` and `DROP FULLTEXT INDEX FOR ()-[r:<type>]-() ON (r.<prop1>, ...)` for relationships.
  - Replaced obsolete procedure calls `CALL db.idx.fulltext.createNodeIndex(...)` and `CALL db.idx.fulltext.drop(...)` which were rejected by FalkorDB 6.0.4.
  - Extended FalkorDB bridge and connection error handling to treat "already indexed" and "no such index" response errors as idempotent successes on `CREATE/DROP FULLTEXT INDEX` and `CREATE/DROP VECTOR INDEX` statements, ensuring `session.create_indexes()` and `session.drop_indexes()` can be called repeatedly without raising.
- **Configurable Apache AGE Graph Name (#179)**:
  - Made the Apache AGE graph name configurable on `Session` and `AsyncSession` via `graph_name="custom_graph"`.
  - Added support for `graph_name` on `PostgresBridge`, `AsyncPostgresBridge`, and URI query strings (`?graph=...` / `?graph_name=...`) parsed by `create_bridge()`.
  - Established a single source of truth per session: explicit `Session.graph_name` takes precedence over bridge or URI graph names, falling back to bridge graph name, defaulting to `"age_graph"` when the session dialect is Apache AGE (`"age"`, `"apache_age"`, `"postgres_age"`).
  - Forwarded the session graph name through all query compilation paths (`Session.execute`, `AsyncSession.execute`), bulk ingestion operations (`bulk_create`, `bulk_upsert`, `bulk_create_relationships`, `ingest_parquet`, `ingest_csv`, `ingest_parquet_relationships`, `ingest_csv_relationships`), transactions (`session.transaction()`), identity map flush batches, and Active Record `.save()` / `.async_save()`.
  - Updated Apache AGE index and drop DDL emitters (`create_indexes`, `drop_indexes`, `apply_schema`) to target the session's resolved graph schema (`"<graph>"."<Label>"`) instead of `ag_catalog`, with explicit `::text` property casting for GIN full-text expressions.
  - Added strict identifier validation via `validate_graph_name()` in Python and `is_valid_age_graph_name` in Rust, rejecting unsafe characters and injection attempts with a `ValueError`.
  - Unified default Apache AGE graph name into a single Rust constant `DEFAULT_AGE_GRAPH` in `voyager-ogm`.
- **Field Defaults Applied to Instances and Writes (#197)**:
  - `Field(default=...)` and `Field(default_factory=...)` now populate newly constructed `Node` and `Relationship` instances when no value is supplied; explicit values, including `None`, still win.
  - Default factories are evaluated per instance instead of once at class definition, so mutable defaults such as lists are never shared between instances.
  - Declaring both `default` and `default_factory` now raises `TypeError` when the field is created.
  - Defaults flow through `Query.create`, bulk create plans, identity-map flushes, and `Node.save()`. In `Query.merge` and bulk merge plans they are written only in `ON CREATE SET` for the records that defaulted them, never in the MERGE key or `ON MATCH SET`, so an existing node keeps its stored value. Read applied values with `Node.get()` / `Relationship.get()`; instance attributes remain bound fields for the query DSL.
- **Partial Bulk MERGE and Dirty-Field Flush Writes (#205)**:
  - Bulk MERGE plans now write only the properties each row actually supplies on `ON CREATE SET` and `ON MATCH SET`; an omitted property keeps its stored value, while a property supplied as `null` explicitly clears it.
  - Records are grouped by both the properties they supply and any fields filled from a declared default. Each group gets its own compiled statement (`BulkIngestionPlan.batch_statements` / `BulkIngestionBatch.statement`), which every bridge, transaction, and session bulk executor now runs per batch. Default-filled fields stay create-only, and every group compiles with the session's configured graph name.
  - `Session.flush()` / `AsyncSession.flush()` and `Node.save()` / `Node.async_save()` now update only dirty fields instead of nulling every untouched property, while new records still send their default-filled fields as create-only.
  - For Polars, PyArrow, and pandas inputs, every column present in the frame counts as supplied, so a null cell clears that property; columns absent from the frame are not written.
- **Bolt URI Embedded Credentials Stripping and Decoding (#181)**:
  - `create_bridge()` now extracts and URL-decodes user and password credentials from Bolt connection URIs, forwarding them as `auth=(user, password)` to `GraphDatabase.driver` / `AsyncGraphDatabase.driver`.
  - Strips `userinfo` from the netloc so the Bolt driver receives a clean `scheme://host:port` string; query parameters are parsed into driver options and removed from the URI, because direct `bolt://` drivers reject them.
  - Added password masking (`user:***@host`) across all connection exception and log formatting paths to guarantee secrets are never leaked.
- **Apache AGE Connection Setup and Scheme Rewriting (#191)**:
  - `create_bridge()` now rewrites `age://` and `age+s://` URI schemes to `postgresql://` prior to connecting, mapping `+s` and `+ssc` to `sslmode=require`.
  - Added unified `setup_postgres_age()` connection initializer that runs `LOAD 'age'; SET search_path = ag_catalog, "$user", public;` unconditionally for `age://` schemes and automatically when the `age` extension is detected in `pg_extension` for `postgresql://` connections.
  - `PostgresBridge` and `AsyncPostgresBridge` now auto-detect AGE on user-supplied connections, allowing `Session(PostgresBridge(conn), dialect="age")` to execute AGE Cypher queries out of the box.
- **Node `__label__` Single-Label Alias (#180)**:
  - `Node` subclasses now accept `__label__ = "X"` as the single-label form of `__labels__ = ["X"]`, and the label is used consistently by query compilation, cached labels, schema registration, and bulk ingestion.
  - Declaring both `__label__` and `__labels__` with a different primary label now raises `TypeError` at class creation.
  - An explicit `@node("X")` decorator argument takes precedence over class-body label declarations; `Relationship` classes now honour a class-body `__type__` in the same way.
- **Live-Engine Python Tests Missing the `live` Marker (#199)**:
  - Marked the remaining Neo4j/Memgraph/FalkorDB and openCypher/openGQL Tier 3 integration tests with `@pytest.mark.live` so `pytest -m "not live"` no longer runs them when containers are up.
  - Existing `skipif` port probes are kept, so `pytest -m live` still skips cleanly when a container is unavailable.
  - `create_bridge()` auth tests that need `psycopg` or `falkordb` are skipped when those optional drivers are not installed (they are not installed on Python 3.14), fixing the Python 3.14 jobs on `main`.
- **Identity Map Uses the Declared Primary Key (#178)**:
  - `Session.register()` / `AsyncSession.register()`, `flush()`, `Node.save()`, and `Node.async_save()` now default `key_field` to the model's declared `Field(primary_key=True)` property instead of assuming `id`, falling back to `id` only when the model declares no primary key.
  - Explicit `key_field=` arguments continue to override the detected primary key unchanged.
- **OGM Write Path and Class-Level Field Binding (#174, #175, #176)**:
  - `Query.create(instance)` now inlines every set field as parameterised pattern properties (nodes and relationships); previously all property values were silently dropped.
  - `Query.merge(instance)` keys the MERGE pattern on the primary key (else unique fields, else all set fields) and writes the remaining fields to both `ON CREATE SET` and `ON MATCH SET`.
  - `Query.merge(...).where(...)` folds equality predicates into the MERGE pattern and raises `ValueError` for any other predicate instead of dropping it, which produced a keyless MERGE.
  - Class-level fields such as `Person.name` resolve to the alias of the single pattern created from that class in `return_`, `where`, `order_by`, `with_`, `set` and aggregations, instead of emitting `RETURN .name` or `WHERE $p0`; an ambiguous or missing pattern raises `ValueError`.
  - Apache AGE: `cypher()` query parameters are passed as a single JSON argument.
- **Stack Overflow in Recursion Depth Guards on Windows and macOS (#166)**:
  - Shrank the per-level stack frames of the RESP parser (`parse_resp_slice`) and the Python AST converter (`py_to_node_handle_depth`) by moving match arms into small helpers, so depth guards trigger before the stack is exhausted on 1 MB (Windows) and 2 MiB (macOS) stacks.
  - Lowered `MAX_RESP_DEPTH` from 128 to 64, matching `MAX_PACKSTREAM_DEPTH`.
  - Enabled `opt-level = 1` in the dev profile for `voyager-net` and `voyager-pyo3`.
  - Depth-guard tests now run on an explicit 1 MB stack on every platform.
- **Panic Safety on Malformed and Non-ASCII Inputs (#158)**:
  - Eliminated panics on malformed query topologies (such as incomplete `MATCH (`, `MATCH (n)-[`, and `MATCH (n)-[r]->(`) by parsing in byte space with bounds-checked Unicode helpers.
  - Hardened `agtype` string cleaning in `voyager-net` to be UTF-8 safe across multi-byte inputs.
  - Replaced reachable `unreachable!()` arms in connection pool drop and handover routines with recovery and warnings.
  - Replaced `unreachable!()` arms in DDL emitters with explicit `UnsupportedDialect` errors.

### Security
- **Dependency Advisory Patches and Supply-Chain Audits (#158)**:
  - Upgraded `arrow`, `arrow-schema`, and `arrow-array` to 57.3, resolving advisory RUSTSEC-2026-0442.
  - Patched `rustls` (0.23.46), `rustls-webpki` (0.103.17), `tokio` (1.54.1), and `webpki-roots` (1.0.12).
  - Added supply-chain vulnerability and dependency hygiene audits (`cargo-deny`, `cargo-machete`, `osv-scanner`) to CI.

## [0.4.0-alpha.6] - 2026-10-02

### Added
- **AST Topology Extraction & TCK Conformance Engine (#101)**:
  - Added AST topology parsing, pattern shape extraction, and AST analysis in `voyager-core`.
  - Added openCypher, openGQL, and SQL/PGQ TCK conformance test suites with Scenario Outline evaluation.
- **Native Wire Protocols & Connection Pool (`voyager-net`)**:
  - Added native asynchronous Bolt wire protocol implementation with PackStream serialization and zero-allocation frame encoding.
  - Added native PostgreSQL Frontend/Backend Protocol v3.0 codec with MD5 and SCRAM-SHA-256 authentication and Apache AGE `agtype` composite projection.
  - Added native Redis RESP2/RESP3 pipeline client for FalkorDB with automated protocol negotiation.
- **Multi-Dialect DDL Migration Emitters & Schema Registry (#92, #93, #94, #95)**:
  - Added multi-dialect schema DDL emission across openCypher, ISO GQL, and SQL:2023 PGQ.
  - Added native schema registry and model metadata storage with `@node`, `@relationship`, and `Field[T]` descriptors.
- **ISO GQL Fluent Query Builder & Query Profiling (#54, #56)**:
  - Extended fluent query builder to support ISO/IEC 39075:2024 GQL syntax and clause compositions.
  - Added execution plan inspection and profiling via `.explain()` and `.profile()`.

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

[Unreleased]: https://github.com/tanishq-kumar/voyager/compare/v0.4.0-alpha.6...HEAD
[0.4.0-alpha.6]: https://github.com/tanishq-kumar/voyager/compare/v0.4.0-alpha.5...v0.4.0-alpha.6
[0.4.0-alpha.5]: https://github.com/tanishq-kumar/voyager/compare/v0.3.0-alpha.1...v0.4.0-alpha.5
[0.3.0-alpha.1]: https://github.com/tanishq-kumar/voyager/compare/v0.2.0-alpha.1...v0.3.0-alpha.1
[0.2.0-alpha.1]: https://github.com/tanishq-kumar/voyager/compare/v0.1.0-alpha.1...v0.2.0-alpha.1
[0.1.0-alpha.1]: https://github.com/tanishq-kumar/voyager/releases/tag/v0.1.0-alpha.1
