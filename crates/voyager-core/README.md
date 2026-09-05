# voyager-core

Core AST compiler, optimizer, and multi-dialect emitters for Voyager OGM.

---

## Query Authoring Approaches

### 1. Step-by-Step Path Chaining
```rust
let mut builder = QueryBuilder::new();
builder
    .r#match()
    .node(Some("p"), vec!["Person"])
    .to(vec!["ACTED_IN"], Some("r"))
    .hops(1, 2)
    .node(Some("m"), vec!["Movie"])
    .where_gt("p", "age", 21)
    .r#return()
    .field("p", "name", Some("actor"))
    .field("m", "title", Some("movie"));
```

### 2. Directional Traversal Methods
```rust
let mut builder = QueryBuilder::new();
builder
    .node_label("Person")
    .out_edge(vec!["ACTED_IN"], Some("r"))
    .node(Some("m"), vec!["Movie"])
    .where_contains("p", "name", "Keanu")
    .field("m", "title", Some("movie_title"))
    .order_by_desc("m", "released")
    .limit(10);
```

### 3. Combined Pattern Builder
```rust
let mut builder = QueryBuilder::new();
builder
    .node(Some("u"), vec!["User"])
    .to_edge(Direction::Outgoing, vec!["FOLLOWS"], Some("f"), Some("target"), vec!["User"])
    .where_eq("u", "name", "Alice")
    .field("target", "name", Some("followed_user"));
```

### 4. Expression Tree Filtering
```rust
let mut builder = QueryBuilder::new();
let age_gt = builder.binary_expr(builder.prop("p", "age"), BinaryOp::Gt, builder.literal(18));
let city_ny = builder.binary_expr(builder.prop("p", "city"), BinaryOp::Eq, builder.literal("NY"));
let city_ldn = builder.binary_expr(builder.prop("p", "city"), BinaryOp::Eq, builder.literal("London"));
let city_or = builder.binary_expr(city_ny, BinaryOp::Or, city_ldn);
let full_filter = builder.binary_expr(age_gt, BinaryOp::And, city_or);

builder
    .node(Some("p"), vec!["Person"])
    .filter(full_filter)
    .field("p", "name", Some("name"));
```

### 5. Direct AST Allocation (`QueryAstArena`)
```rust
let mut arena = QueryAstArena::new();
let node = arena.alloc(AstNode::NodePattern {
    variable: Some("p".into()),
    labels: vec!["Person".into()],
    predicates: vec![],
});
let match_clause = arena.alloc(AstNode::MatchClause {
    optional: false,
    paths: vec![node],
    where_clause: None,
});
let root = arena.alloc(AstNode::QueryStatement {
    matches: vec![match_clause],
    return_clause: None,
});
```
