# ISO/IEC 9075-16:2023 SQL:PGQ & DuckPGQ Conformance Specification

This document defines the comprehensive matrix of **SQL:2023 Part 16 (Property Graph Queries - SQL/PGQ)** and **DuckPGQ** standard syntax supported by Voyager OGM, with empirical verification notes against live DuckPGQ execution.

---

## 🏛 The Canonical SQL:2023 PGQ Compilation Matrix

| # | Feature Category | SQL:2023 PGQ / DuckPGQ Transpilation | Voyager Python OGM API | Status | Engine Reality & Verification Notes |
| :- | :--- | :--- | :--- | :---: | :--- |
| **1** | **Single Node Pattern** | `SELECT * FROM GRAPH_TABLE (g MATCH (p IS Person) COLUMNS (p.name AS name, p.age AS age))` | `Query.match(p).return_(name=p.name, age=p.age).compile("sql_pgq", graph_name="g")` | **Verified** | Executes cleanly on live DuckPGQ; returns projected node columns. |
| **2** | **Directed Traversal** | `SELECT * FROM GRAPH_TABLE (g MATCH (a IS Person) -[r IS KNOWS]-> (b IS Person) COLUMNS (a.name AS source, b.name AS target))` | `Query.match(a).to(Knows, "r").node(b).return_(source=a.name, target=b.name).compile("sql_pgq", graph_name="g")` | **Verified** | Executes cleanly on live DuckPGQ; expands 1-hop outgoing edges. |
| **3** | **Incoming Traversal** | `SELECT * FROM GRAPH_TABLE (g MATCH (c IS Company) <-[r IS WORKS_AT]- (a IS Person) COLUMNS (c.name AS company, a.name AS person))` | `Query.match(c).from_(WorksAt, "r").node(a).return_(company=c.name, person=a.name).compile("sql_pgq", graph_name="g")` | **Verified** | Executes cleanly on live DuckPGQ; expands incoming edges. |
| **4** | **Undirected Traversal** | `SELECT * FROM GRAPH_TABLE (g MATCH (a IS Person) -[r IS KNOWS]- (b IS Person) COLUMNS (b.name AS name))` | `Query.match(a).edge(Knows, "r").node(b).return_(b.name).compile("sql_pgq", graph_name="g")` | **Verified** | Executes cleanly on live DuckPGQ; bidirectional edge match. |
| **5** | **Bounded Repetition Hops** | `SELECT * FROM GRAPH_TABLE (g MATCH (a IS Person) -[r IS KNOWS]->{1,3} (b IS Person) COLUMNS (b.name AS name))` | `Query.match(a).to(Knows, "r").hops(1, 3).node(b).return_(b.name).compile("sql_pgq", graph_name="g")` | **Compiler AST Only** | AST currently emits `-[IS KNOWS]{1,3}->`, which fails DuckPGQ parser. Live DuckPGQ requires `->{1,3}` (alignment tracked in #58). |
| **6** | **String Predicate (`CONTAINS`)** | `SELECT * FROM GRAPH_TABLE (g MATCH (p IS Person) WHERE p.city LIKE '%' \|\| $p0 \|\| '%' COLUMNS (p.name AS name))` | `Query.match(p).where(p.city.contains("don")).return_(p.name).compile("sql_pgq", graph_name="g")` | **Verified** | Executes cleanly on live DuckPGQ; parameter binding with LIKE matches. |
| **7** | **String Predicate (`STARTS WITH`)** | `SELECT * FROM GRAPH_TABLE (g MATCH (p IS Person) WHERE p.name LIKE $p0 \|\| '%' COLUMNS (p.name AS name))` | `Query.match(p).where(p.name.startswith("Al")).return_(p.name).compile("sql_pgq", graph_name="g")` | **Verified** | Executes cleanly on live DuckPGQ. |
| **8** | **Comparison & Range Filters** | `SELECT * FROM GRAPH_TABLE (g MATCH (p IS Person) WHERE (p.age > $p0) AND (p.city = $p1) COLUMNS (p.name AS name))` | `Query.match(p).where(p.age > 30, p.city == "London").return_(p.name).compile("sql_pgq", graph_name="g")` | **Verified** | Executes cleanly on live DuckPGQ. |
| **9** | **Projections & Column Aliases** | `SELECT * FROM GRAPH_TABLE (g MATCH (p IS Person) COLUMNS (p.name AS full_name, p.age AS age))` | `Query.match(p).return_(full_name=p.name, age=p.age).compile("sql_pgq", graph_name="g")` | **Verified** | Executes cleanly on live DuckPGQ; column renaming works as expected. |
| **10** | **Aggregations in `COLUMNS`** | `SELECT city, COUNT(name) AS count, AVG(age) AS avg_age FROM GRAPH_TABLE (g MATCH (p IS Person) COLUMNS (p.city AS city, p.name AS name, p.age AS age)) GROUP BY city` | `Query.match(p).return_(city=p.city, count=p.name.count(), avg_age=p.age.avg()).compile("sql_pgq", graph_name="g")` | **Compiler AST Only** | AST currently places `COUNT(...)` inside `COLUMNS`, which fails live SQL binder without `GROUP BY`. Grouping aggregations belong in outer SQL (tracked in #58). |
| **11** | **Sorting & Ordering** | `SELECT * FROM GRAPH_TABLE (g MATCH (p IS Person) COLUMNS (p.name AS name, p.age AS age)) ORDER BY age ASC` | `Query.match(p).return_(p.name, p.age).order_by(p.age).compile("sql_pgq", graph_name="g")` | **Compiler AST Only** | AST outputs `ORDER BY p.age`, which fails because graph alias `p` is out of scope in outer SQL. Must order by projected alias `ORDER BY age` (tracked in #58). |
| **12** | **Offset Pagination** | `SELECT * FROM GRAPH_TABLE (g MATCH (p IS Person) COLUMNS (p.name AS name)) LIMIT 10 OFFSET 5` | `Query.match(p).return_(p.name).skip(5).limit(10).compile("sql_pgq", graph_name="g")` | **Verified (DuckDB)** | Executes cleanly in DuckDB; ANSI SQL standard `OFFSET ... ROWS FETCH NEXT ... ROWS ONLY` tracked in #58. |

---

## 🔬 SQL:2023 PGQ Invariants & Engine Realities

1. **`GRAPH_TABLE` Function:** Pattern matching is encapsulated within `GRAPH_TABLE (<graph_name> MATCH <pattern> [WHERE <predicates>] COLUMNS (<projections>))`.
2. **Label Disjunction / Conjunction:** Node/edge labels use `IS <Label>` syntax (e.g. `(p IS Person)`).
3. **Like Translation:** Cypher `CONTAINS 'val'` transpiles into parameterized SQL `LIKE '%' || $p0 || '%'`.
4. **Quantifier Repetition:** DuckPGQ and SQL:2023 PGQ require repetition quantifiers after the arrow (e.g. `->{1,3}`), rather than bracketed `-[IS KNOWS]{1,3}->`.
5. **Scalar Functions:** Scalar functions must map to SQL standard names (`UPPER()`, `LOWER()`, `LENGTH()`). Raw Cypher function names (`toUpper`) are rejected by SQL engines.
6. **Outer Query Scope:** The outer SQL query enclosing `GRAPH_TABLE` can only reference column aliases explicitly declared in `COLUMNS (...)`. Internal graph aliases (e.g. `p.age`) are out of scope for outer `ORDER BY` and `GROUP BY` clauses.

> [!NOTE]
> **Audit Status (Sub-Issue #59 / Parent #58):**
> Previously, documentation marked all 12 queries as "100% verified" based on compiler AST string emission tests. In live testing with DuckPGQ, queries 1-4, 6-9, and 12 executed successfully, while queries 5, 10, and 11 failed due to parser/binder constraints. Full dialect alignment and live execution tests are tracked in Issue #58.
