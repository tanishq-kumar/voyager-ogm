# Voyager OGM: Multi-Dialect Conformance Architecture

This document describes how Voyager OGM validates query compilation across **openCypher**, **SQL:2023 PGQ**, **ISO GQL**, and **Apache AGE** using test suites based on standard specifications.

---

## 3-Layer Verification Pipeline

### Layer 1: Table-Driven Conformance Tests
- Implemented in Rust (`crates/voyager-core/tests/tck_conformance_tests.rs`, `gql_conformance_tests.rs`, `pgq_conformance_tests.rs`, `age_conformance_tests.rs`) and Python (`packages/python/tests/`).
- Validates query structure and parameter binding across operators, traversal hops, mutations, and bulk operations.

### Layer 2: Golden Snapshot Regression (`cargo insta`)
- Compares emitted query strings and parameter maps against verified baseline files in `crates/voyager-core/tests/snapshots/`.
- Ensures zero syntax regressions across AST changes.

### Layer 3: Live 6-Engine Integration Matrix
- Runs compiled queries and parameters against live database containers via `just test-matrix`:
  - **Neo4j 5.26** (Bolt protocol)
  - **Memgraph** (Bolt protocol)
  - **Apache AGE** (PostgreSQL extension with `agtype`)
  - **DuckDB & DuckPGQ** (`GRAPH_TABLE` with zero-copy Polars export)
  - **PostgreSQL 19 Beta 3** (Recursive SQL queries)
  - **FalkorDB** (Low-latency Cypher queries)

---

## Dialect Support Matrix

| Clause / Feature | openCypher | SQL:2023 PGQ & DuckPGQ | ISO GQL (2024) | Apache AGE |
| :--- | :--- | :--- | :--- | :--- |
| **Node Filtering (`WHERE`)** | `(p:Person WHERE p.age > 21)` | `(p:Person WHERE p.age > 21)` | `(p:Person WHERE p.age > 21)` | `(p:Person) WHERE p.age > 21` |
| **Edge Traversal** | `(a)-[:KNOWS]->(b)` | `(a) -[:knows]-> (b)` | `(a) -[:KNOWS]-> (b)` | `(a)-[:KNOWS]->(b)` |
| **Variable Hops** | `[:KNOWS*1..3]` | `-[k:knows]->{1,3}` | `-[k:KNOWS]->{1,3}` | `[:KNOWS*1..3]` |
| **Undirected Edges** | `(a)-[:KNOWS]-(b)` | `(a) -[:knows]- (b)` | `(a) ~[:KNOWS]~ (b)` | `(a)-[:KNOWS]-(b)` |
| **Aggregations** | `count(p), avg(p.age)` | `COUNT(p.id), AVG(p.age)` | `count(p), avg(p.age)` | `count(p), avg(p.age)` |
| **Distinct Projections** | `RETURN DISTINCT p.name` | `SELECT DISTINCT gt.name` | `RETURN DISTINCT p.name` | `RETURN DISTINCT p.name` |
| **Pagination** | `SKIP 10 LIMIT 5` | `LIMIT 5 OFFSET 10` | `OFFSET 10 LIMIT 5` | `SKIP 10 LIMIT 5` |
