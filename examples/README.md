# 󰓾 Voyager OGM: Code Examples

Runnable, production-grade examples demonstrating the different query building and entity modeling approaches across **Rust** and **Python**.

---

## 🦀 Rust Examples (`crates/voyager-core/examples/`)

Run all Rust examples with:
```bash
just examples-rust
# or
cargo run --example <example_name>
```

| Example File | Approach | Description |
| :--- | :--- | :--- |
| [`01_step_by_step_chaining.rs`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/crates/voyager-core/examples/01_step_by_step_chaining.rs) | **Step-by-Step Path Chaining** | Multi-hop traversal chaining (`.node().to().hops().node().from().node()`) matching Memgraph/openCypher mental model. |
| [`02_semantic_shortcuts.rs`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/crates/voyager-core/examples/02_semantic_shortcuts.rs) | **Semantic Shortcuts** | Concise directional navigation (`.node_label()`, `.out_edge()`, `.where_contains()`, `.order_by_desc()`). |
| [`03_single_call_pattern.rs`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/crates/voyager-core/examples/03_single_call_pattern.rs) | **Combined 1-Hop Pattern** | Compact 1-hop pattern definitions (`.node().to_edge()`). |
| [`04_expression_tree_builder.rs`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/crates/voyager-core/examples/04_expression_tree_builder.rs) | **Nested Expression Trees** | Complex boolean predicate trees (`AND`, `OR`, `XOR`, regex, property comparisons). |
| [`05_direct_arena_allocation.rs`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/crates/voyager-core/examples/05_direct_arena_allocation.rs) | **32-bit Memory Arena** | Direct `QueryAstArena` handle allocation for compiler writers and transpiler authors. |

---

## 🐍 Python Tutorial Series: Beginner to Advanced (`examples/python/`)

Run all Python tutorials with:
```bash
just examples-python
# or
uv run python examples/python/<tutorial_name>.py
```

| Level | Tutorial File | Concepts & Graph Topologies | Description |
| :---: | :--- | :--- | :--- |
| 🟢 **L1** | [`01_beginner_hello_graph.py`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/examples/python/01_beginner_hello_graph.py) | **"Hello Graph!" Single Node CRUD** | Pure Python type annotations (`@node class Person:`), auto-aliasing, basic `.where()`, `.return_()`. |
| 🟡 **L2** | [`02_intermediate_relationships.py`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/examples/python/02_intermediate_relationships.py) | **Nodes, Edges & Multi-Hop Paths** | Connecting nodes `(Actor)-[ACTED_IN]->(Movie)<-[DIRECTED]-(Director)` with edge properties and aliases. |
| 🟠 **L3** | [`03_advanced_multihop_aggregations.py`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/examples/python/03_advanced_multihop_aggregations.py) | **Variable Paths & Graph Metrics** | Variable length bounds (`.hops(1, 3)`), `DISTINCT` deduplication, `order_by_desc()`, `skip()` pagination. |
| 🔴 **L4** | [`04_data_science_polars_streaming.py`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/examples/python/04_data_science_polars_streaming.py) | **Zero-Copy Arrow & Polars Ingestion** | Streaming 100,000 graph nodes directly into Polars DataFrame via `__arrow_c_stream__` in < 2ms. |
| 🟣 **L5** | [`05_enterprise_multi_dialect.py`](file:///C:/Users/supri/Documents/github/voyager/voyager-ogm/examples/python/05_enterprise_multi_dialect.py) | **Universal Multi-Dialect Compilation** | Compiling ONE graph AST simultaneously into **openCypher 9/25**, **SQL:2023 PGQ**, and **ISO GQL 2024**. |
