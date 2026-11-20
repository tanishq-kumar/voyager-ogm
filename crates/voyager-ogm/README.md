# voyager-ogm

`voyager-ogm` is the core of Voyager OGM: a vendor-neutral Object-Graph Mapper and AST query compiler in safe Rust (`#![forbid(unsafe_code)]`). Build a query once with the fluent `QueryBuilder`, optimize it, and compile it to openCypher 9/25, SQL:2023 PGQ (`GRAPH_TABLE`, DuckDB/DuckPGQ), ISO GQL, or Apache AGE.

> Status: `0.4.0-alpha`, pre-1.0. Breaking changes may land in minor releases.

## Install

```bash
cargo add voyager-ogm
```

| Feature | Default | Enables |
| :--- | :--- | :--- |
| `serde` | yes | `serde` and `serde_json` derives for AST and compiled-query types |
| `arrow` | yes | Apache Arrow bridge (`arrow-array`, `arrow-schema`) for columnar results |

For a slim build without Arrow, use `cargo add voyager-ogm --no-default-features --features serde`.

## Examples

Each snippet below is a doctest and runs as printed.

### Build and emit Cypher

```rust
# use voyager_ogm::builder::QueryBuilder;
# use voyager_ogm::emitters::CypherEmitter;
# use voyager_ogm::visitor::AstVisitor;
let mut builder = QueryBuilder::new();
builder
    .r#match()
    .node(Some("p"), vec!["Person"])
    .to(vec!["ACTED_IN"], Some("r"))
    .node(Some("m"), vec!["Movie"])
    .where_gt("p", "age", 21)
    .r#return()
    .field("p", "name", Some("actor"))
    .field("m", "title", Some("movie"));

let (arena, root) = builder.build();
let mut emitter = CypherEmitter::new();
let compiled = emitter.visit_query(&arena, root).unwrap();
assert!(compiled.statement.starts_with("MATCH"));
```

### Emit the same AST for other dialects

```rust
# use voyager_ogm::builder::QueryBuilder;
# use voyager_ogm::emitters::{IsoGqlEmitter, SqlPgqEmitter};
# use voyager_ogm::visitor::AstVisitor;
let mut builder = QueryBuilder::new();
builder
    .r#match()
    .node(Some("p"), vec!["Person"])
    .to(vec!["ACTED_IN"], Some("r"))
    .node(Some("m"), vec!["Movie"])
    .r#return()
    .field("p", "name", Some("actor"))
    .field("m", "title", Some("movie"));

let (arena, root) = builder.build();

let mut pgq = SqlPgqEmitter::new("cinema_graph");
let sql = pgq.visit_query(&arena, root).unwrap();
assert!(sql.statement.contains("GRAPH_TABLE"));

let mut gql = IsoGqlEmitter::new();
let iso = gql.visit_query(&arena, root).unwrap();
assert!(iso.statement.starts_with("MATCH"));
```

### Semantic shortcuts

```rust
# use voyager_ogm::builder::QueryBuilder;
# use voyager_ogm::emitters::CypherEmitter;
# use voyager_ogm::visitor::AstVisitor;
let mut builder = QueryBuilder::new();
builder
    .node(Some("p"), vec!["Person"]) // bound as `p`
    .out_edge(vec!["KNOWS"], Some("r"))
    .node(Some("friend"), vec!["Person"])
    .where_contains("p", "name", "Keanu")
    .where_gte("friend", "age", 18)
    .field("friend", "name", Some("friend_name"))
    .field("friend", "city", Some("city"))
    .order_by_desc("friend", "age")
    .limit(25);

let (arena, root) = builder.build();
let mut emitter = CypherEmitter::new();
let compiled = emitter.visit_query(&arena, root).unwrap();
assert!(compiled.statement.contains("CONTAINS"));
```

### Direct AST allocation

`QueryAstArena` is the same contiguous arena the builder writes to. Allocate nodes directly when you need full control:

```rust
# use voyager_ogm::ast::*;
# use voyager_ogm::emitters::IsoGqlEmitter;
# use voyager_ogm::visitor::AstVisitor;
let mut arena = QueryAstArena::new();

let node = arena.alloc(AstNode::NodePattern {
    variable: Some("p".into()),
    label_expression: LabelExpression::from_labels(["Person", "Developer"]),
    predicates: vec![],
});
let matched = arena.alloc(AstNode::MatchClause {
    optional: false,
    paths: vec![node],
    where_clause: None,
});
let target = arena.alloc(AstNode::Identifier("p".into()));
let property = arena.alloc(AstNode::PropertyAccess {
    target,
    property: "name".into(),
});
let returned = arena.alloc(AstNode::ReturnClause {
    distinct: false,
    projections: vec![ProjectionItem {
        expression: property,
        alias: Some("dev_name".into()),
        aggregation: None,
    }],
    order_by: vec![],
    skip: None,
    limit: Some(10),
});
let root = arena.alloc(AstNode::QueryStatement {
    execution_mode: ExecutionMode::Normal,
    load_csv: None,
    unwinds: vec![],
    matches: vec![matched],
    linear_clauses: vec![],
    with_clauses: vec![],
    mutations: vec![],
    return_clause: Some(returned),
});

let mut emitter = IsoGqlEmitter::new();
let compiled = emitter.visit_query(&arena, root).unwrap();
assert!(compiled.statement.contains("dev_name"));
```

More runnable examples live in [`crates/voyager-ogm/examples`](https://github.com/tanishq-kumar/voyager/tree/main/crates/voyager-ogm/examples); run one with `cargo run -p voyager-ogm --example 01_step_by_step_chaining`.

## Supported dialects

| Dialect | Emitter | Targets |
| :--- | :--- | :--- |
| openCypher 9 / Cypher 25 | `CypherEmitter::new()` | Neo4j, Memgraph, FalkorDB |
| SQL:2023 PGQ (`GRAPH_TABLE`) | `SqlPgqEmitter::new("graph_name")` | DuckDB/DuckPGQ, PostgreSQL 19+ |
| ISO GQL (ISO/IEC 39075:2024) | `IsoGqlEmitter::new()` | Standard GQL engines |
| Apache AGE | `AgeEmitter::new("age_graph")` | Cypher-in-SQL on PostgreSQL |

DDL helpers for schema, constraints, indexes, vector, and full-text indexes are exported from the crate root.

## Related projects

`voyager-ogm` compiles queries; [`voyager-drivers`](https://crates.io/crates/voyager-drivers) executes them over Bolt, PostgreSQL, and Redis; [`voyager-cli`](https://crates.io/crates/voyager-cli) installs the `voyager` binary. Python users get the same compiler in the [`voyager-ogm`](https://pypi.org/project/voyager-ogm/) package on PyPI.

## MSRV

Rust **1.87.0** (the workspace `rust-version`).

## License

MIT OR Apache-2.0. See [LICENSE-MIT](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE-MIT) and [LICENSE-APACHE](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE-APACHE).

## Links

- Repository: <https://github.com/tanishq-kumar/voyager>
- API docs: <https://docs.rs/voyager-ogm>
- Changelog: <https://github.com/tanishq-kumar/voyager/blob/main/CHANGELOG.md>
