# Voyager OGM Code Examples

Code examples showing query building and entity modeling patterns in Rust and Python.

---

## Rust Examples (`crates/voyager-core/examples/`)

Run the Rust examples with:
```bash
just examples-rust
# or
cargo run --example <example_name>
```

| Example File | Approach | Description |
| :--- | :--- | :--- |
| [`01_step_by_step_chaining.rs`](../crates/voyager-core/examples/01_step_by_step_chaining.rs) | **Step-by-Step Path Chaining** | Multi-hop traversal chaining with fluent methods. |
| [`02_semantic_shortcuts.rs`](../crates/voyager-core/examples/02_semantic_shortcuts.rs) | **Semantic Shortcuts** | Directional navigation with `.node_label()`, `.out_edge()`, and `.order_by_desc()`. |
| [`03_single_call_pattern.rs`](../crates/voyager-core/examples/03_single_call_pattern.rs) | **Combined 1-Hop Pattern** | 1-hop pattern definitions with `.node().to_edge()`. |
| [`04_expression_tree_builder.rs`](../crates/voyager-core/examples/04_expression_tree_builder.rs) | **Nested Expression Trees** | Boolean predicate trees with `AND`, `OR`, `XOR`, and property comparisons. |
| [`05_direct_arena_allocation.rs`](../crates/voyager-core/examples/05_direct_arena_allocation.rs) | **Direct AST Allocation** | Direct AST allocation using `QueryAstArena`. |
| [`06_ast_optimizer_constant_folding.rs`](../crates/voyager-core/examples/06_ast_optimizer_constant_folding.rs) | **AST Optimizer & Constant Folding** | Compile-time arithmetic folding, boolean simplification, and predicate pushdown. |
| [`07_explain_and_profile_queries.rs`](../crates/voyager-core/examples/07_explain_and_profile_queries.rs) | **Query Profiling & Plan Explanation** | ExecutionMode (`EXPLAIN` / `PROFILE`), fluent modifiers, and multi-dialect emission. |
| [`live_explain_and_profile.rs`](../crates/voyager-net/examples/live_explain_and_profile.rs) | **Live Engine Profiling & Plan Explanation** | Live Bolt execution against Neo4j and Memgraph with QueryBuilder seeding and profiling. |

---

## Python Examples (`examples/python/`)

Run the Python examples with:
```bash
just examples-python
# or
uv run python examples/python/<example_name>.py
```

| Level | File | Concept | Description |
| :---: | :--- | :--- | :--- |
| 1 | [`01_beginner_hello_graph.py`](python/01_beginner_hello_graph.py) | **Single Node Operations** | Model definition with `@node`, auto-aliasing, and filtering. |
| 2 | [`02_intermediate_relationships.py`](python/02_intermediate_relationships.py) | **Relationship Traversals** | Connecting nodes with edge patterns, aliases, and property filters. |
| 3 | [`03_advanced_multihop_aggregations.py`](python/03_advanced_multihop_aggregations.py) | **Variable Paths & Aggregation** | Variable-length hops (`.hops(1, 3)`), `DISTINCT`, sorting, and pagination. |
| 4 | [`04_data_science_polars_streaming.py`](python/04_data_science_polars_streaming.py) | **Columnar Streaming** | Stream node records directly into Polars DataFrames using the Arrow C interface. |
| 5 | [`05_enterprise_multi_dialect.py`](python/05_enterprise_multi_dialect.py) | **Multi-Dialect Compilation** | Compile a single AST into openCypher, SQL:2023 PGQ, and ISO GQL. |
| 6 | [`06_sqlalchemy_hybrid_bridge.py`](python/06_sqlalchemy_hybrid_bridge.py) | **SQLAlchemy Integration** | Relational-to-graph bridging via `PropertyGraph`, `graph_table()`, and CTE transpilation. |
| 7 | [`07_marimo_graphrag_demo.py`](python/07_marimo_graphrag_demo.py) | **Marimo Notebook** | Interactive graph visualization in Marimo notebook cells. |
| 8 | [`08_jupyter_vscode_graph_demo.ipynb`](python/08_jupyter_vscode_graph_demo.ipynb) | **VS Code / Jupyter Notebook** | Interactive graph explorer in VS Code and Jupyter notebooks. |
| 9 | [`09_query_path_visualization.py`](python/09_query_path_visualization.py) | **Interactive Graph Explorer** | Local interactive UI server for exploring multi-hop subgraphs. |
| 10 | [`10_live_neo4j_execution_demo.py`](python/10_live_neo4j_execution_demo.py) | **Live Neo4j Database Execution** | Real Bolt protocol execution, fraud detection graph seed, and path visualization. |
| 11 | [`11_query_optimizer_constant_folding.py`](python/11_query_optimizer_constant_folding.py) | **AST Constant Folding & Optimizer** | Compile-time arithmetic reduction, associative reassociation, and index seek hoisting. |
| 12 | [`12_query_profiling_and_plan_explanation.py`](python/12_query_profiling_and_plan_explanation.py) | **Query Profiling & Plan Explanation** | Fluent `.explain()`/`.profile()`, functional `explain()`/`profile()`, and multi-dialect emission. |


