# Voyager OGM

**Multi-Dialect Object-Graph Mapper (OGM) and Query Compiler**

[![CI](https://github.com/tanishq-kumar/voyager-ogm/actions/workflows/ci.yml/badge.svg)](https://github.com/tanishq-kumar/voyager-ogm/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/tanishq-kumar/voyager-ogm?color=orange&label=Release)](https://github.com/tanishq-kumar/voyager-ogm/releases)
[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Standards](https://img.shields.io/badge/Standards-openCypher%20%7C%20SQL%3A2023%20PGQ%20%7C%20ISO%20GQL-green.svg)](https://www.iso.org/standard/76120.html)
[![Arrow](https://img.shields.io/badge/Zero--Copy-Apache%20Arrow%20PyCapsule-orange.svg)](https://arrow.apache.org/)

---

## Why I Built Voyager

I started Voyager after facing real vendor lock-in and performance bottlenecks migrating between graph databases like Neo4j, Memgraph, and PostgreSQL Apache AGE.

In the relational world, tools like SQLAlchemy make switching databases relatively straightforward. In the graph ecosystem, every database uses a slightly different dialect, custom driver protocols, and slow object serialization.

I wanted a single tool that lets me:

1. **Write one graph query and compile it to any dialect** (openCypher, SQL:2023 PGQ, ISO GQL).
2. **Eliminate Python object hydration bottlenecks** by streaming data directly into Polars via Apache Arrow.
3. **Optimize queries automatically** with a compiler pass before sending them to the database.

I am developing Voyager in safe Rust with native Python (`PyO3`) bindings, with planned bindings for TypeScript (`NAPI-RS`).

---

## Key Features

### 1. Multi-Dialect Query Compilation

Define models with Python type annotations and build queries once. Voyager compiles the abstract syntax tree (AST) into the exact dialect required by your database:

- **openCypher**: Neo4j, Memgraph, FalkorDB, AWS Neptune.
- **SQL:2023 PGQ (`GRAPH_TABLE`)**: DuckDB DuckPGQ, PostgreSQL.
- **ISO GQL (ISO/IEC 39075:2024)**: The official international graph query standard.

```python
from voyager_ogm import Node, Relationship, Query, node, relationship


@node(label="Person")
class Person:
    name: str
    age: int
    city: str = "London"


@relationship(type_name="WORKS_AT")
class WorksAt:
    since: int = 2024


@node(label="Company")
class Company:
    name: str


p = Person()
c = Company()
w = WorksAt()

query = (
    Query.match(p)
    .to(w)
    .node(c)
    .where(p.age >= 21, c.name == "Acme Corp")
    .return_(p.name, p.age, company_name=c.name)
    .order_by(p.name)
    .limit(10)
)

# 1. Compile to openCypher (Neo4j, Memgraph, FalkorDB)
cypher = query.compile("cypher")
print(cypher.statement)
# MATCH (_person_0:Person)-[_worksat_0:WORKS_AT]->(_company_0:Company)
# WHERE (_person_0.age >= $p0) AND (_company_0.name = $p1)
# RETURN _person_0.name, _person_0.age, _company_0.name AS company_name
# ORDER BY _person_0.name ASC LIMIT 10

# 2. Compile to SQL:2023 PGQ (DuckDB, PostgreSQL)
pgq = query.compile("sql_pgq", graph_name="corp_graph")
print(pgq.statement)

# 3. Compile to ISO GQL
gql = query.compile("iso_gql")
print(gql.statement)
```

### 2. Zero-Copy Apache Arrow & Polars Streaming

Traditional Python OGMs instantiate thousands of Python objects when fetching query results, creating a major serialization bottleneck. Voyager uses the **Apache Arrow C Data Interface** (`__arrow_c_stream__`) to convert graph database records directly into **Polars DataFrames** at memory speed without Python serialization overhead.

```python
import polars as pl
from voyager_ogm import generate_synthetic_stream, to_polars

# Stream query records directly into Polars in milliseconds
stream = generate_synthetic_stream(100_000)
df = to_polars(stream)

# Run analytical queries with zero object materialization overhead
stats = (
    df.lazy()
    .filter(pl.col("active"))
    .group_by("label")
    .agg(
        pl.len().alias("total_nodes"),
        pl.col("age").mean().alias("avg_age"),
    )
    .collect()
)
print(stats)
```

### 3. Rule-Based Query Optimizer

Before queries are emitted to the database, Voyager runs optimization passes over the AST:

- **Predicate Pushdown**: Automatically inlines filter conditions directly into the pattern match, allowing database engines to leverage node/relationship indexes instead of scanning full tables.
- **Constant Folding**: Pre-computes arithmetic and static sub-expressions at compile time in Rust, even when combined with dynamic variables. The database engine never re-evaluates static formulas across millions of graph records.
- **Dead Clause Elimination**: Prunes unused query fragments and redundant traversals.

#### Optimization Example:

```python
# Query with a dynamic database property and a time formula:
# e.g., finding events exceeding a user's base quota plus a 7-day retention window
query = (
    Query.match(user)
    .to(action)
    .node(event)
    .where(
        user.city == "London",
        event.duration_sec >= user.base_quota + (7 * 24 * 60 * 60),  # Constant folding
    )
    .return_(user.name, event.type)
)

# Optimized query emitted to the database:
# MATCH (_user_0:Person {city: $p0})-[_action_0:PERFORMED]->(_event_0:Event)
# WHERE (_event_0.duration_sec >= _user_0.base_quota + 604800)  <-- Folded at compile time
# RETURN _user_0.name, _event_0.type
```

### 4. Complex Expressions & Graph Functions

Instead of limiting queries to flat equality checks (`p.age == 25`), Voyager's expression AST supports nested computations, arithmetic trees, string operations, and graph built-in functions:

```python
# Complex expressions in projections and filters:
query = (
    Query.match(p)
    .where(
        p.name.to_lower().starts_with("al"),
        p.salary * 1.15 + p.bonus > 100_000,
        p.tags.size() > 2,
    )
    .return_(
        full_name=p.first_name + " " + p.last_name,
        display_name=p.nickname.coalesce(p.first_name),
    )
)
```

### 5. In-Memory Identity Map & Unit of Work

Voyager combines **Active Record** ergonomics with **Data Mapper** architecture:

- **Active Record Ergonomics**: Work with graph nodes and relationships as natural Python objects—read, update, and modify fields directly in your application code.
- **Data Mapper Cleanliness**: Instead of firing an immediate database query on every property modification, the transactional Unit of Work tracks dirty fields in memory and flushes them in a single, minimal batched update.
- **Identity Map Consistency**: If the same graph node is loaded across multiple queries or traversals within a transaction, Voyager returns the exact same in-memory object. Changes made in one place are immediately visible everywhere without stale reads or duplicate database lookups.

#### Example:

```python
with session.transaction():
    # 1. Fetch a person from the database
    alice = session.get(Person, id="alice_42")

    # 2. Modify properties directly (Active Record style)
    alice.city = "Cambridge"
    alice.role = "Lead Architect"

    # 3. Another query in the same transaction traverses a team that includes Alice:
    # Voyager reuses the existing in-memory object instead of re-fetching
    lead = query.match(team).to(has_lead).node(person).first()
    print(lead.city)  # "Cambridge" (changes are immediately visible, zero stale reads)

    # 4. Flush changes: Unit of Work automatically emits a single minimal UPDATE
    session.flush()
```

### 6. Interactive Graph Viewer

Voyager includes a built-in interactive graph viewer with force-directed physics, Cartesian dot grid, schema legend chips, and an inspector drawer.

```python
from voyager_ogm import view_graph

# In a Marimo notebook cell or VS Code interactive notebook (.ipynb):
view_graph(nodes, relationships)
```

Explore the runnable interactive demos in the repository:

- [**Marimo GraphRAG Demo**](examples/python/07_marimo_graphrag_demo.py) — Interactive dashboard with schema filters, neighborhood spotlights, and inspector drawer (`marimo run examples/python/07_marimo_graphrag_demo.py`).
- [**Jupyter & VS Code Notebook**](examples/python/08_jupyter_vscode_graph_demo.ipynb) — Interactive `.ipynb` notebook demonstrating graph exploration directly within cell outputs.
- [**Query Path Visualizer**](examples/python/09_query_path_visualization.py) — Visualizes multi-hop path traversals and graph pattern matching.

---

## Database Compatibility

Voyager is verified against 6 database backends:

| Database             | Dialect / Standard           | Connection / Transport   |
| :------------------- | :--------------------------- | :----------------------- |
| **Neo4j**            | openCypher / Cypher 25       | Bolt Protocol            |
| **Memgraph**         | openCypher / ISO GQL         | Bolt Protocol            |
| **Apache AGE**       | Cypher-in-SQL                | PostgreSQL Wire Protocol |
| **DuckDB & DuckPGQ** | SQL:2023 PGQ (`GRAPH_TABLE`) | In-Memory / Arrow FFI    |
| **FalkorDB**         | openCypher                   | Native Client / Bolt     |
| **PostgreSQL**       | Relational / Recursive SQL   | PostgreSQL Wire Protocol |

---

## Roadmap

Voyager's development is organized into focused milestone phases. You can track active progress, issues, and completion percentages directly on [**GitHub Milestones**](https://github.com/tanishq-kumar/voyager-ogm/milestones):

- **[Milestone 1: v0.1.0-alpha](https://github.com/tanishq-kumar/voyager-ogm/milestone/1)** — Core Read AST Engine _(Completed)_
- **[Milestone 2: v0.2.0-alpha](https://github.com/tanishq-kumar/voyager-ogm/milestone/2)** — Mutations, Ingestion & Arrow Streaming _(Completed)_
- **[Milestone 3: v0.3.0-alpha](https://github.com/tanishq-kumar/voyager-ogm/milestone/3)** — Multi-Dialect Syntax Conformance _(Completed)_
- **[Milestone 4: v0.4.0-alpha](https://github.com/tanishq-kumar/voyager-ogm/milestone/4)** — Python Integrations, Rich Expressions & Query Optimizer _(Active)_
- **[Milestone 5: v0.5.0-alpha](https://github.com/tanishq-kumar/voyager-ogm/milestone/5)** — TypeScript SDK _(Planned)_
- **[Milestone 6: v0.6.0-alpha](https://github.com/tanishq-kumar/voyager-ogm/milestone/6)** — Voyager CLI (`voy`) _(Planned)_

---

## Contributing & Local Development

Voyager is a solo project and contributions, feedback, and constructive criticism are warmly welcome!

For local build setup instructions, testing with database containers, and contribution guidelines, please see [**CONTRIBUTING.md**](CONTRIBUTING.md).

---

## License

Dual-licensed under either:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))
