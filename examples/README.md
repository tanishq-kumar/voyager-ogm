# Voyager OGM Code Examples

Comprehensive, end-to-end code examples demonstrating query building, entity modeling, multi-dialect compilation, streaming, ingestion, transactions, and live database execution across Python and Rust.

---

## Containers & Database Setup

Local database engines for live execution are defined in [`containers/compose.yaml`](../containers/compose.yaml) (a standard, runtime-neutral Compose specification compatible with Docker, Podman, or any Compose runner).

Start the required engine(s) using your preferred runner:
```bash
# Docker Compose
docker compose -f containers/compose.yaml up -d <service>   # e.g. neo4j, memgraph, falkordb, apache-age

# Podman Compose
podman compose -f containers/compose.yaml up -d <service>

# Automation recipe via just
just up <service>
```

Offline examples require no running database and execute either compile-only or using in-memory mock and embedded engines.

---

## Python & Rust Language Feature Counterparts

Voyager OGM provides idiomatic interfaces in both Python and Rust. Where counterparts exist across languages, they are indexed below:

| Feature / Domain Area | Python Example (`examples/python/`) | Rust Counterpart (`crates/*/examples/`) | What They Teach |
| :--- | :--- | :--- | :--- |
| **Basic Entity Modeling** | [`01_hello_graph.py`](python/01_hello_graph.py) | [`03_single_call_pattern.rs`](../crates/voyager-ogm/examples/03_single_call_pattern.rs) | Minimal entity modeling, 1-hop pattern traversal, and openCypher/SQL:2023 PGQ emission. |
| **Semantic Traversal Shortcuts** | [`02_minimal_type_annotations.py`](python/02_minimal_type_annotations.py) | [`02_semantic_shortcuts.rs`](../crates/voyager-ogm/examples/02_semantic_shortcuts.rs) | Concise entity declarations, directional shortcuts, string filters, and sorting. |
| **Multi-Hop Path Traversal** | [`04_relationships_and_traversals.py`](python/04_relationships_and_traversals.py) | [`01_step_by_step_chaining.rs`](../crates/voyager-ogm/examples/01_step_by_step_chaining.rs) | Multi-hop directional path chaining: `(Node)-[REL]->(Node)<-[REL]-(Node)`. |
| **Expression Trees & Logic** | [`06_multihop_aggregations.py`](python/06_multihop_aggregations.py) | [`04_expression_tree_builder.rs`](../crates/voyager-ogm/examples/04_expression_tree_builder.rs) | Nested boolean expression trees (`AND`, `OR`, `NOT`), parenthesized predicates, parameter maps. |
| **Native Arena Allocation** | [`29_native_rust_builder.py`](python/29_native_rust_builder.py) | [`05_direct_arena_allocation.rs`](../crates/voyager-ogm/examples/05_direct_arena_allocation.rs) | Direct low-level AST arena allocation via `QueryAstArena` for extreme compilation throughput. |
| **Query Optimizer & Pushdown** | [`10_query_optimizer_constant_folding.py`](python/10_query_optimizer_constant_folding.py) | [`06_ast_optimizer_constant_folding.rs`](../crates/voyager-ogm/examples/06_ast_optimizer_constant_folding.rs) | Compile-time arithmetic reduction, associative folding, and index seek pushdown hoisting. |
| **Plan Explain & Profiling** | [`11_query_profiling_and_explain.py`](python/11_query_profiling_and_explain.py) | [`07_explain_and_profile_queries.rs`](../crates/voyager-ogm/examples/07_explain_and_profile_queries.rs) | ExecutionMode configuration (`EXPLAIN` and `PROFILE`) with multi-dialect keyword mappings. |
| **Schema DDL & Indexes** | [`13_schema_ddl_and_indexes.py`](python/13_schema_ddl_and_indexes.py) | [`08_schema_ddl_and_indexes.rs`](../crates/voyager-ogm/examples/08_schema_ddl_and_indexes.rs) | Declarative entity schemas, constraints, composite indexes, Cypher 25, GQL, and PGQ DDL. |
| **Graph Mutation DSL** | [`07_mutation_dsl.py`](python/07_mutation_dsl.py) | [`09_mutation_builder.rs`](../crates/voyager-ogm/examples/09_mutation_builder.rs) | Declarative DML mutations: `CREATE`, `MERGE` (`ON CREATE`/`ON MATCH`), `SET`, and `DELETE`. |
| **Bulk Ingestion Compilation** | [`19_bulk_ingestion.py`](python/19_bulk_ingestion.py) | [`10_bulk_ingest_compilation.rs`](../crates/voyager-ogm/examples/10_bulk_ingest_compilation.rs) | Vectorized `UNWIND $batch AS row` node/edge bulk templates for high-throughput loads. |
| **Transactions & Savepoints** | [`18_transactions_and_identity_map.py`](python/18_transactions_and_identity_map.py) | [`11_transactions_and_unit_of_work.rs`](../crates/voyager-ogm/examples/11_transactions_and_unit_of_work.rs) | Unit of Work mutation tracking, AST checkpoints, named savepoints, and transaction rollbacks. |
| **Vector & Full-Text Search** | [`12_vector_and_fulltext_search.py`](python/12_vector_and_fulltext_search.py) | [`12_vector_and_fulltext_search.rs`](../crates/voyager-ogm/examples/12_vector_and_fulltext_search.rs) | Vector embeddings, full-text indexes, DDL generation, and similarity/procedure queries. |
| **Live Engine Connection** | [`30_live_neo4j_execution.py`](python/30_live_neo4j_execution.py) | [`live_connect_and_query.rs`](../crates/voyager-net/examples/live_connect_and_query.rs) | Direct wire connection over Bolt, connection pooling, and clean unreachable handling. |
| **Live Engine Profiling** | [`11_query_profiling_and_explain.py`](python/11_query_profiling_and_explain.py) | [`live_explain_and_profile.rs`](../crates/voyager-net/examples/live_explain_and_profile.rs) | Live query planning and runtime performance metrics against running database instances. |

---

## Rust Examples

Run all offline Rust examples with:
```bash
just examples-rust
# or run an individual example:
cargo run -p voyager-ogm --example <example_name>
```

Run live Rust engine examples (requires running containers) with:
```bash
just examples-live
# or run individual live example:
cargo run -p voyager-net --example <example_name>
```

### Core Engine & AST Compilation (`crates/voyager-ogm/examples/`)

| File | What It Teaches | Prerequisites | Run Command |
| :--- | :--- | :--- | :--- |
| [`01_step_by_step_chaining.rs`](../crates/voyager-ogm/examples/01_step_by_step_chaining.rs) | Multi-hop path chaining (`.r#match()`, `.node()`, `.to()`), parameterized filtering, multi-dialect emission. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 01_step_by_step_chaining` |
| [`02_semantic_shortcuts.rs`](../crates/voyager-ogm/examples/02_semantic_shortcuts.rs) | Directional shortcuts (`.node_label()`, `.out_edge()`, `.field()`), string filters, descending ordering. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 02_semantic_shortcuts` |
| [`03_single_call_pattern.rs`](../crates/voyager-ogm/examples/03_single_call_pattern.rs) | Combined 1-hop pattern definitions (`.node().to_edge()`), explicit `Direction` control, SQL:2023 PGQ output. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 03_single_call_pattern` |
| [`04_expression_tree_builder.rs`](../crates/voyager-ogm/examples/04_expression_tree_builder.rs) | Nested boolean expression trees (`AND`, `OR`, `XOR`), parenthesized Cypher WHERE clauses, parameter maps. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 04_expression_tree_builder` |
| [`05_direct_arena_allocation.rs`](../crates/voyager-ogm/examples/05_direct_arena_allocation.rs) | Direct AST allocation with `QueryAstArena`, contiguous arena indexing, ISO GQL statement emission. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 05_direct_arena_allocation` |
| [`06_ast_optimizer_constant_folding.rs`](../crates/voyager-ogm/examples/06_ast_optimizer_constant_folding.rs) | Compile-time arithmetic constant reduction, associative reassociation, index seek pushdown hoisting. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 06_ast_optimizer_constant_folding` |
| [`07_explain_and_profile_queries.rs`](../crates/voyager-ogm/examples/07_explain_and_profile_queries.rs) | ExecutionMode configuration (`EXPLAIN` / `PROFILE`), multi-dialect keyword mappings across 4 engines. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 07_explain_and_profile_queries` |
| [`08_schema_ddl_and_indexes.rs`](../crates/voyager-ogm/examples/08_schema_ddl_and_indexes.rs) | Declarative schema DDL, constraints, composite indexes, Cypher 25, GQL, and SQL:2023 PGQ catalog emission. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 08_schema_ddl_and_indexes` |
| [`09_mutation_builder.rs`](../crates/voyager-ogm/examples/09_mutation_builder.rs) | Graph mutations: `create()`, path creation, `merge()` with `on_create_set()` / `on_match_set()`, `set_property()`, `delete()`, `detach_delete()`. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 09_mutation_builder` |
| [`10_bulk_ingest_compilation.rs`](../crates/voyager-ogm/examples/10_bulk_ingest_compilation.rs) | High-throughput `UNWIND $batch AS row` template compilation for bulk node creation, upserts, and relationships. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 10_bulk_ingest_compilation` |
| [`11_transactions_and_unit_of_work.rs`](../crates/voyager-ogm/examples/11_transactions_and_unit_of_work.rs) | Unit of Work mutation tracking, AST checkpoints, named savepoints, and two-layer transaction rollbacks. | None (offline / in-memory UoW) | `cargo run -p voyager-ogm --example 11_transactions_and_unit_of_work` |
| [`12_vector_and_fulltext_search.rs`](../crates/voyager-ogm/examples/12_vector_and_fulltext_search.rs) | Vector embeddings, full-text indexes, multi-dialect DDL, and vector similarity / full-text procedure queries. | None (offline / compile-only) | `cargo run -p voyager-ogm --example 12_vector_and_fulltext_search` |

### Live Engine Execution (`crates/voyager-net/examples/`)

| File | What It Teaches | Prerequisites | Run Command |
| :--- | :--- | :--- | :--- |
| [`live_explain_and_profile.rs`](../crates/voyager-net/examples/live_explain_and_profile.rs) | Live Bolt execution against Neo4j (7687) and Memgraph (7688), graph cleanup/seeding, profiling metrics. | Neo4j / Memgraph container | `cargo run -p voyager-net --example live_explain_and_profile` |
| [`live_connect_and_query.rs`](../crates/voyager-net/examples/live_connect_and_query.rs) | Direct wire connection over Bolt, connection pooling via `NativeClient`, Arrow result batches, clean skip handling. | Neo4j / Memgraph container | `cargo run -p voyager-net --example live_connect_and_query` |

---

## Python Learning Path (`examples/python/`)

The Python examples form a progressive **beginner → advanced learning path** (numbered `01` through `31`).

Run all offline Python examples with:
```bash
just examples-python
# or individual example:
uv run python examples/python/<filename>.py
```

Run live database examples (requires running database containers) with:
```bash
just examples-live
```

### Phase 1: Entity Modeling & Basic Querying

| # | File | What It Teaches | Prerequisites | Run Command |
| :-: | :--- | :--- | :--- | :--- |
| 01 | [`01_hello_graph.py`](python/01_hello_graph.py) | First graph model with `@node`, automatic alias generation (`_person_0`), basic comparison operators, openCypher compilation. | None (offline / compile-only) | `uv run python examples/python/01_hello_graph.py` |
| 02 | [`02_minimal_type_annotations.py`](python/02_minimal_type_annotations.py) | Defining nodes and relationships using standard Python type annotations without `Field()`, pattern traversals, sorting, limit. | None (offline / compile-only) | `uv run python examples/python/02_minimal_type_annotations.py` |
| 03 | [`03_custom_labels_and_constraints.py`](python/03_custom_labels_and_constraints.py) | Explicit labels (`@node(label="Customer")`), uniqueness and secondary indexes (`Field(unique=True, index=True)`), SQL:2023 PGQ compilation. | None (offline / compile-only) | `uv run python examples/python/03_custom_labels_and_constraints.py` |
| 04 | [`04_relationships_and_traversals.py`](python/04_relationships_and_traversals.py) | Multi-hop directional traversals: `(Actor)-[ACTED_IN]->(Movie)<-[DIRECTED]-(Director)`, relationship property filters. | None (offline / compile-only) | `uv run python examples/python/04_relationships_and_traversals.py` |
| 05 | [`05_oop_class_inheritance.py`](python/05_oop_class_inheritance.py) | Pure OOP subclassing (`class Person(Node):`, `class ActedIn(Relationship):`), when to prefer inheritance vs decorators, ISO GQL compilation. | None (offline / compile-only) | `uv run python examples/python/05_oop_class_inheritance.py` |

### Phase 2: Querying & Query Expressions

| # | File | What It Teaches | Prerequisites | Run Command |
| :-: | :--- | :--- | :--- | :--- |
| 06 | [`06_multihop_aggregations.py`](python/06_multihop_aggregations.py) | Variable-length path expansion (`.hops(1, 3)`), DISTINCT deduplication, DESC sorting, SKIP / LIMIT pagination. | None (offline / compile-only) | `uv run python examples/python/06_multihop_aggregations.py` |
| 07 | [`07_mutation_dsl.py`](python/07_mutation_dsl.py) | Declarative DML mutations: `create()`, path creation, `merge()` with `on_create_set()` / `on_match_set()`, `set()`, `detach_delete()`, model dirty tracking. | None (offline / compile-only) | `uv run python examples/python/07_mutation_dsl.py` |
| 08 | [`08_multi_dialect_compilation.py`](python/08_multi_dialect_compilation.py) | Cross-compiling a single AST into openCypher 9/25, SQL:2023 PGQ (`GRAPH_TABLE`), and ISO/IEC 39075:2024 GQL. | None (offline / compile-only) | `uv run python examples/python/08_multi_dialect_compilation.py` |
| 09 | [`09_enterprise_dialects.py`](python/09_enterprise_dialects.py) | Multi-label node patterns (`:Person:Engineer`), dialect cheat-sheet (`"cypher"`, `"sql_pgq"`, `"iso_gql"`, `"age"`), schema naming. | None (offline / compile-only) | `uv run python examples/python/09_enterprise_dialects.py` |
| 10 | [`10_query_optimizer_constant_folding.py`](python/10_query_optimizer_constant_folding.py) | Compile-time arithmetic constant reduction, associative reassociation, index seek predicate pushdown hoisting into `{age: $p0}`. | None (offline / compile-only) | `uv run python examples/python/10_query_optimizer_constant_folding.py` |
| 11 | [`11_query_profiling_and_explain.py`](python/11_query_profiling_and_explain.py) | Fluent `.explain()` and `.profile()`, functional style `explain()`/`profile()`, multi-dialect execution keyword translations, live execution with clean offline skips. | None (offline / compile-only) | `uv run python examples/python/11_query_profiling_and_explain.py` |

### Phase 3: Search & Advanced Schema Features

| # | File | What It Teaches | Prerequisites | Run Command |
| :-: | :--- | :--- | :--- | :--- |
| 12 | [`12_vector_and_fulltext_search.py`](python/12_vector_and_fulltext_search.py) | Vector embedding properties (`VectorProperty`), BM25 full-text indexes (`FullTextIndex`), multi-dialect DDL (Neo4j, Memgraph, DuckDB, Postgres pgvector), `Query.vector_search()`, `Query.fulltext_search()`. | None (offline / compile-only) | `uv run python examples/python/12_vector_and_fulltext_search.py` |
| 13 | [`13_schema_ddl_and_indexes.py`](python/13_schema_ddl_and_indexes.py) | Automated DDL generation for constraints, composite indexes, and secondary indexes across 6 backends via `SchemaManager` and `Session`. | None (offline / compile-only) | `uv run python examples/python/13_schema_ddl_and_indexes.py` |
| 14 | [`14_schema_validation.py`](python/14_schema_validation.py) | Global `SchemaRegistry` registration, ahead-of-time raw query validation (LLM text), fluent `Query.validate()`, Levenshtein suggestions. | None (offline / compile-only) | `uv run python examples/python/14_schema_validation.py` |
| 15 | [`15_topology_extraction.py`](python/15_topology_extraction.py) | Extracting graph pattern topology directly from query ASTs with `query.extract_topology()`, inspecting nodes, edges, labels, and directions. | None (offline / compile-only) | `uv run python examples/python/15_topology_extraction.py` |
| 16 | [`16_spec_interchange.py`](python/16_spec_interchange.py) | Query Spec JSON interchange format, cross-language compilation ABI with `compile_query_from_spec()`, dynamic spec manipulation. | None (offline / compile-only) | `uv run python examples/python/16_spec_interchange.py` |

### Phase 4: Bridges, Engines & Execution Sessions

| # | File | What It Teaches | Prerequisites | Run Command |
| :-: | :--- | :--- | :--- | :--- |
| 17 | [`17_database_bridges.py`](python/17_database_bridges.py) | Unified connectivity with `create_bridge()`, URI schemes (`bolt://`, `duckdb://`, `postgres://`, `falkordb://`, `relate://`), graceful `fallback="mock"`, OGM session dispatch. | None (in-memory DuckDB, Relate, Mock) | `uv run python examples/python/17_database_bridges.py` |
| 18 | [`18_transactions_and_identity_map.py`](python/18_transactions_and_identity_map.py) | Atomic transactions (`session.transaction()`), auto-commit/rollback, savepoints (`tx.savepoint()`), Identity Map caching, and vectorized `session.flush()`. | None (in-memory MockBridge) | `uv run python examples/python/18_transactions_and_identity_map.py` |
| 19 | [`19_bulk_ingestion.py`](python/19_bulk_ingestion.py) | High-throughput UNWIND ingestion: `session.ingest_csv()`, `session.ingest_parquet()`, `session.bulk_create()`, `session.bulk_create_relationships()`. | None (in-memory MockBridge + temp dir) | `uv run python examples/python/19_bulk_ingestion.py` |
| 20 | [`20_embedded_relate_bridge.py`](python/20_embedded_relate_bridge.py) | Zero-network embedded graph database execution via `relate://:memory:`, OGM schema constraints, OGM entity creation, traversal queries, and bulk ingestion. | `relate` extra (`pip install 'voyager-ogm[relate]'`) | `uv run python examples/python/20_embedded_relate_bridge.py` |
| 21 | [`21_data_science_polars_streaming.py`](python/21_data_science_polars_streaming.py) | Zero-copy columnar streaming of 100,000 entities directly into Polars DataFrames using the Arrow PyCapsule protocol (`__arrow_c_stream__`). | None (Polars base dependency) | `uv run python examples/python/21_data_science_polars_streaming.py` |
| 22 | [`22_async_and_hybrid_sessions.py`](python/22_async_and_hybrid_sessions.py) | Non-blocking `AsyncSession` context managers and standalone `HybridSession` / `AsyncHybridSession` coordinating relational and graph queries into Polars. | `sqlalchemy` extra (`pip install 'voyager-ogm[sqlalchemy]'`) | `uv run python examples/python/22_async_and_hybrid_sessions.py` |
| 23 | [`23_sqlalchemy_hybrid_bridge.py`](python/23_sqlalchemy_hybrid_bridge.py) | Relational-to-graph bridging via `PropertyGraph`, `graph_table()`, live recursive CTE graph traversal on SQLite, `graph_relationship()` descriptor. | `sqlalchemy` extra (`pip install 'voyager-ogm[sqlalchemy]'`) | `uv run python examples/python/23_sqlalchemy_hybrid_bridge.py` |
| 24 | [`24_sqlalchemy_graph_table.py`](python/24_sqlalchemy_graph_table.py) | SQLAlchemy Core `graph_table()` FromClause, multi-label pattern derivation from AST arena, composing relational `select()` queries. | `sqlalchemy` extra (`pip install 'voyager-ogm[sqlalchemy]'`) | `uv run python examples/python/24_sqlalchemy_graph_table.py` |
| 25 | [`25_networkx_analytics.py`](python/25_networkx_analytics.py) | Exporting OGM query results to NetworkX DiGraphs via `result.to_networkx()`, running PageRank, centrality, and shortest paths. | `analytics` extra (`pip install 'voyager-ogm[analytics]'`) | `uv run python examples/python/25_networkx_analytics.py` |

### Phase 5: Visualization, UI & Native Integration

| # | File | What It Teaches | Prerequisites | Run Command |
| :-: | :--- | :--- | :--- | :--- |
| 26 | [`26_query_path_visualization.py`](python/26_query_path_visualization.py) | Result record extraction (`mappings()`, `scalars()`, `to_polars()`), graph topology path reconstruction, `GraphViewer` UI inspection. | None (in-memory MockBridge) | `uv run python examples/python/26_query_path_visualization.py` |
| 27 | [`27_marimo_graphrag_demo.py`](python/27_marimo_graphrag_demo.py) | Interactive reactive notebook dashboard in Marimo, reactive WebGL node click callbacks, filtering subgraphs in real time. | `notebook` extra (`pip install 'voyager-ogm[notebook]'`) | `marimo run examples/python/27_marimo_graphrag_demo.py` |
| 28 | [`28_jupyter_vscode_graph_demo.ipynb`](python/28_jupyter_vscode_graph_demo.ipynb) | Interactive notebook for JupyterLab and VS Code, cell output graph visualizer, inspector drawer, multi-dialect code tabs. | `notebook` extra (`pip install 'voyager-ogm[notebook]'`) | Open in JupyterLab or VS Code |
| 29 | [`29_native_rust_builder.py`](python/29_native_rust_builder.py) | Low-level Rust AST arena manipulation via `NativeQueryBuilder` from `voyager_ogm`, high-throughput escape hatch for automated pipelines. | None (offline / in-memory) | `uv run python examples/python/29_native_rust_builder.py` |

### Phase 6: Live Database Execution (Requires Running Containers)

| # | File | What It Teaches | Prerequisites | Run Command |
| :-: | :--- | :--- | :--- | :--- |
| 30 | [`30_live_neo4j_execution.py`](python/30_live_neo4j_execution.py) | Real Bolt connection to live Neo4j (7687), OGM model definitions, schema constraints, high-throughput OGM bulk seeding, query execution, and path visualization. | Neo4j container (`docker compose -f containers/compose.yaml up -d neo4j`), `neo4j` extra | `uv run python examples/python/30_live_neo4j_execution.py` |
| 31 | [`31_live_multi_engine_execution.py`](python/31_live_multi_engine_execution.py) | Live multi-engine query execution across Memgraph (port 7688), FalkorDB (port 6379), and Apache AGE (port 5455), with OGM cleanup, seeding, and clean offline skips. | Container(s) via Compose, `neo4j`, `falkordb`, `psycopg` extras | `uv run python examples/python/31_live_multi_engine_execution.py` |
