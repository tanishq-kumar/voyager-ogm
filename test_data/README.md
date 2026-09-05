# Voyager OGM Test Data and Conformance Fixtures

This directory contains test datasets, graph schemas, syntax conformance fixtures, and benchmark dataset generators for Voyager OGM.

---

## Directory Structure

```
test_data/
├── README.md                      # Test data documentation and index
├── tck/                           # Specification conformance test fixtures
│   ├── TCK_HARNESS.md             # Conformance verification architecture
│   ├── verify_tck.py              # Test fixture auditor script
│   ├── iso_gql/                   # ISO/IEC 39075:2024 GQL conformance scenarios
│   │   └── gql_standard_conformance.feature
│   ├── sql_pgq/                   # ISO SQL:2023 PGQ & DuckPGQ sqllogictest (.test) suites
│   │   ├── graph_table_matching.test
│   │   ├── variable_length_paths.test
│   │   └── aggregations_grouping.test
│   └── vendor_extensions/         # Database-specific vendor procedures (APOC, MAGE, DuckDB)
│       └── vendor_functions.json
├── schemas/                       # Graph Schema definitions (JSON schemas)
│   ├── movies_schema.json         # Movies & Cast graph schema
│   ├── social_schema.json         # Social network benchmark schema
│   └── fraud_schema.json          # Fraud detection graph schema
├── movies/                        # Movie graph dataset
│   ├── seed.cypher                # openCypher seeding script
│   ├── seed_pgq.sql               # ISO SQL:2023 PGQ DDL & DML translation
│   ├── nodes_persons.csv          # Person nodes table
│   ├── nodes_movies.csv           # Movie nodes table
│   ├── edges_acted_in.csv         # ACTED_IN edges table
│   └── edges_directed.csv         # DIRECTED edges table
├── social/                        # Social network dataset
│   ├── seed.cypher                # openCypher seeding script
│   ├── seed_pgq.sql               # SQL:2023 PGQ DDL & DML script
│   ├── nodes_persons.jsonl        # Person node records
│   ├── nodes_posts.jsonl          # Post node records
│   └── edges_knows.jsonl          # KNOWS edge records
├── queries/                       # Multi-dialect golden AST test cases
│   ├── simple_filter.json         # 1-hop property filter golden queries
│   ├── multi_hop_traversal.json   # 2-hop & 3-hop traversal golden queries
│   ├── aggregation_grouping.json  # Aggregations (COUNT, AVG, SUM, MIN, MAX)
│   ├── variable_length_path.json  # Variable hop paths (1..3, 1..5, *)
│   └── complex_predicates.json    # Nested AND/OR/NOT, IN, CONTAINS, Regex
└── generator/                     # Benchmark synthetic data generator
    └── generate_bench_dataset.py  # Generates 1K to 1M+ nodes/edges for Arrow benchmarks
```

---

## Dataset Standards and Sources

| Dataset | Standard and Source | Use Case |
| :--- | :--- | :--- |
| **`test_data/movies/`** | Standard movie dataset modeled on canonical graph examples. | Basic CRUD, single-hop matching, and multi-hop relationship traversals. |
| **`test_data/social/`** | Social network dataset modeled on the LDBC SNB schema. | Complex multi-hop traversals, cyclic paths, and path filtering. |
| **`test_data/tck/`** | Test fixtures based on openCypher, SQL:2023 PGQ, and ISO GQL specifications. | Validates query compiler syntax correctness across all supported dialects. |
| **`test_data/generator/`** | Synthetic graph generator for 1K to 1M+ nodes and edges. | Evaluates zero-copy Arrow streaming and Polars DataFrame hydration. |

---

## Conformance Test Suites

### 1. openCypher Conformance (`crates/voyager-core/tests/tck_conformance_tests.rs`)
- **Format:** Table-driven Rust and Python test suites.
- **Features Tested:**
  - `MATCH`, `WHERE`, `RETURN`, `ORDER BY`, `LIMIT`, `DISTINCT`.
  - Aggregations (`count`, `avg`, `sum`, `min`, `max`, `collect`).
  - Variable-length path traversals (`[:KNOWS*1..3]`).
  - Label expressions (`MATCH (p:Person & (Developer | Manager))`).
  - DML mutations (`CREATE`, `MERGE`, `SET`, `DELETE`).

### 2. SQL:2023 PGQ & DuckPGQ (`test_data/tck/sql_pgq/`)
- **Format:** DuckDB `sqllogictest` (`.test` files).
- **Features Tested:**
  - `CREATE PROPERTY GRAPH` vertex and edge table DDL.
  - `FROM GRAPH_TABLE (...)` property graph matching.
  - Path repetition syntax (`-[k:knows]->{1,3}`).
  - Aggregations and column projections.

### 3. ISO GQL Conformance (`test_data/tck/iso_gql/`)
- **Format:** GQL Gherkin scenarios.
- **Features Tested:**
  - Standard GQL `MATCH` statements with property filters.
  - Path patterns with edge labels.
  - Parenthesized path pattern quantification (`->{1,3}`).

### 4. Database Vendor Extensions (`test_data/tck/vendor_extensions/`)
- **Neo4j APOC:** Procedure calls (`apoc.path.subgraphNodes`).
- **Neo4j Vector Search:** Approximate Nearest Neighbor search (`db.index.vector.queryNodes`).
- **Memgraph MAGE:** Graph algorithm procedures (`pagerank.get()`).
- **DuckDB DuckPGQ:** Parquet scanning via `read_parquet()`.

---

## Auditing the Test Fixtures

Run the test fixture audit script to inspect all registered scenarios:

```bash
uv run python test_data/tck/verify_tck.py
```

---

## Generating Benchmark Datasets

To generate dataset files for benchmark tests:

```bash
uv run python test_data/generator/generate_bench_dataset.py --nodes 100000 --edges 500000 --output-dir test_data/bench_100k
```
