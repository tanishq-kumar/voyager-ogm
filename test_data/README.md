# 󰓾 Voyager OGM: Test Datasets, Official TCKs & Golden Fixtures

This directory contains standardized test datasets, graph schemas, **official Technology Compatibility Kits (TCKs)**, multi-dialect golden queries, and benchmark dataset generators used across Voyager OGM's test suite (`voyager-core`, `voyager-pyo3`, `voyager-napi`, `voyager-cli`, and visual studio).

---

## 📁 Directory Structure

```
test_data/
├── README.md                      # Test data documentation and index
├── tck/                           # Official Technology Compatibility Kits
│   ├── TCK_HARNESS.md             # Conformance verification architecture
│   ├── verify_tck.py              # Automated TCK test suite auditor
│   ├── opencypher/                # Official openCypher 9 & Cypher 25 Gherkin (.feature) test suites
│   │   ├── MatchAcceptanceTest.feature
│   │   ├── WhereAcceptanceTest.feature
│   │   ├── ReturnAcceptanceTest.feature
│   │   ├── PathAcceptanceTest.feature
│   │   └── cypher25_modern.feature
│   ├── sql_pgq/                   # Official ISO SQL:2023 PGQ & DuckPGQ sqllogictest (.test) suites
│   │   ├── graph_table_matching.test
│   │   ├── variable_length_paths.test
│   │   └── aggregations_grouping.test
│   ├── iso_gql/                   # Official ISO/IEC 39075:2024 GQL Conformance Suite
│   │   └── gql_standard_conformance.feature
│   └── vendor_extensions/         # Database-specific vendor procedures (Neo4j APOC, Memgraph MAGE, DuckDB)
│       └── vendor_functions.json
├── schemas/                       # Graph Schema definitions (JSON schemas)
│   ├── movies_schema.json         # Canonical Movies & Cast graph schema
│   ├── social_schema.json         # LDBC-style Social Network schema
│   └── fraud_schema.json          # Financial Fraud graph schema
├── movies/                        # Official Canonical Movie Graph dataset
│   ├── seed.cypher                # Official Neo4j :play movies seeding script
│   ├── seed_pgq.sql               # ISO SQL:2023 PGQ DDL & DML translation
│   ├── nodes_persons.csv          # Person nodes table
│   ├── nodes_movies.csv           # Movie nodes table
│   ├── edges_acted_in.csv         # ACTED_IN edges table
│   └── edges_directed.csv         # DIRECTED edges table
├── social/                        # LDBC Social Network Benchmark dataset
│   ├── seed.cypher                # openCypher seeding script
│   ├── seed_pgq.sql               # SQL:2023 PGQ DDL & DML script
│   ├── nodes_persons.jsonl        # Person node records
│   ├── nodes_posts.jsonl          # Post node records
│   └── edges_knows.jsonl          # KNOWS edge records
├── queries/                       # Multi-dialect golden AST test cases
│   ├── simple_filter.json         # 1-hop property filter golden queries
│   ├── multi_hop_traversal.json   # 2-hop & 3-hop traversal golden queries
│   ├── aggregation_grouping.json  # Aggregation (COUNT, AVG, SUM, MIN, MAX)
│   ├── variable_length_path.json  # Variable hop paths (1..3, 1..5, *)
│   └── complex_predicates.json    # Nested AND/OR/NOT, IN, CONTAINS, Regex
└── generator/                     # Benchmark synthetic data generator
    └── generate_bench_dataset.py  # Generates 1K to 1M+ nodes/edges for Arrow benchmarks
```

---

## 🏛️ Dataset Standard Status & Provenance

| Dataset | Standard Status & Provenance | Industry Use Case |
| :--- | :--- | :--- |
| **`test_data/movies/`** | **Official Canonical Movie Graph**<br>Direct 1:1 match with the official **Neo4j / openCypher `:play movies`** standard dataset (`movies.cypher`). | The universal "Hello World" of graph databases. Used in virtually all Cypher/GQL documentation, benchmarks, and tutorials (The Matrix, Tom Hanks, Apollo 13, etc.). |
| **`test_data/social/`** | **LDBC SNB (Linked Data Benchmark Council) Standard**<br>Modeled on the official ISO/LDBC Social Network Benchmark schema (`Person`, `Post`, `Tag`, `KNOWS`, `LIKES`, `CREATOR_OF`). | The international standard benchmark (LDBC Interactive Workload) used for evaluating graph query performance, multi-hop traversals, and network algorithms. |
| **`test_data/tck/`** | **Official Upstream TCKs**<br>• openCypher 9 TCK (`opencypher/openCypher`)<br>• DuckPGQ & ISO SQL:2023 `sqllogictest` (`duckpgq/duckpgq`)<br>• ISO/IEC 39075:2024 GQL Conformance Suite | Formal conformance test suites used by graph database engine vendors (Neo4j, DuckDB Labs, AWS Neptune, Memgraph) to certify spec compliance. |
| **`test_data/generator/`** | **Scalable Synthetic Benchmark Generator**<br>Generates 1K to 1,000,000+ entities in Arrow / CSV / JSON-L. | Used to test high-throughput Arrow PyCapsule zero-copy streaming and Polars hydration SLAs (< 20ms for 1,000,000 nodes). |

---

## 🧪 Official TCKs (Technology Compatibility Kits)

### 1. openCypher 9 & Cypher 25 TCK (`test_data/tck/opencypher/`)
- **Standard:** openCypher 9 Specification & Neo4j Cypher 25 / GQL alignment.
- **Format:** Cucumber Gherkin (`.feature` files).
- **Features Tested:**
  - Standard `MATCH`, `WHERE`, `RETURN`, `ORDER BY`, `LIMIT`, `DISTINCT`.
  - Aggregations (`count`, `avg`, `sum`, `min`, `max`, `collect`).
  - Bounded variable-length path traversals (`[:KNOWS*1..3]`).
  - Modern Cypher 25 / GQL Label Expressions (`MATCH (p:Person & (Developer | Manager))`).
  - Quantified Path Patterns (`((sub)-[:REPORTS_TO]->(mgr)){1,2}`).

### 2. SQL:2023 PGQ & DuckPGQ Tests (`test_data/tck/sql_pgq/`)
- **Standard:** ISO/IEC 9075-16:2023 (SQL/PGQ) Part 16.
- **Format:** DuckDB `sqllogictest` (`.test` files).
- **Features Tested:**
  - `CREATE PROPERTY GRAPH` vertex and edge table DDL.
  - `FROM GRAPH_TABLE (...)` property graph matching.
  - ISO quantified path repetition syntax (`-[k:knows]->{1,3}`).
  - Aggregation and `GROUP BY` column projections.

### 3. ISO GQL Conformance Suite (`test_data/tck/iso_gql/`)
- **Standard:** ISO/IEC 39075:2024 (Database languages — GQL).
- **Format:** GQL TCK Gherkin scenarios.
- **Features Tested:**
  - Standard GQL `MATCH` statements with property conditions.
  - Path patterns with edge labels.
  - Parenthesized path pattern quantification (`->{1,3}`).

### 4. Database-Specific Vendor Extensions (`test_data/tck/vendor_extensions/`)
- **Neo4j APOC:** Procedure calls (`apoc.path.subgraphNodes`).
- **Neo4j Vector Search:** Approximate Nearest Neighbor (ANN) search (`db.index.vector.queryNodes`).
- **Memgraph MAGE:** Graph algorithms (`pagerank.get()`).
- **DuckDB DuckPGQ:** Direct querying over Parquet files via `read_parquet()`.

---

## 🔍 Auditing the Test Suite

Run the automated TCK auditor script to inspect all registered scenarios:

```bash
uv run python test_data/tck/verify_tck.py
```

---

## ⚡ Generating Benchmark Datasets

To generate 100,000 or 1,000,000 nodes/edges for zero-copy Arrow / Polars hydration SLA benchmarks:

```bash
uv run python test_data/generator/generate_bench_dataset.py --nodes 100000 --edges 500000 --output-dir test_data/bench_100k
```
