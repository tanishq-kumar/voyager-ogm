# Voyager OGM Query Spec Interchange Protocol (v1)

## Overview

The **Query Spec Interchange Protocol** is the canonical, language-agnostic Abstract Syntax Tree (AST) exchange contract for Voyager OGM. It defines how graph queries, traversal chains, filter predicates, DML mutations, aggregations, and pagination are serialized across FFI boundaries between:

- Python SDK (`voyager-ogm`)
- TypeScript / Node.js SDK (`@voyager-ogm/client`)
- Native Rust Core Engine (`voyager-core` / `voyager-pyo3`)
- AI Model Context Protocol (MCP) servers and language agents

This formal ABI contract guarantees cross-language stability and deterministic translation into target dialects (**openCypher**, **ISO/IEC 39075:2024 GQL**, **SQL:2023 PGQ**, and **Apache AGE**).

---

## 1. Specification Versioning (`spec_version: 1`)

Every query spec dictionary **must** carry a top-level integer attribute:
```json
{
  "spec_version": 1
}
```

### Compatibility Rules
1. **Major Version Invariance**: The native Rust compiler (`compile_query_from_spec`) and internal builders strictly reject unknown major versions:
   - Any payload missing `"spec_version"` is rejected immediately with:
     `Missing required 'spec_version' in query spec dictionary. Expected 'spec_version': 1.`
   - Any payload with an unsupported version (`spec_version: 2`) is rejected with:
     `Unsupported query spec_version: 2. Supported major versions: 1.`
2. **Clause Versioning**: Individual clause dictionaries (`matches[i]`, `with_clauses[i]`) may optionally declare `"spec_version": 1`. If present, unknown values are rejected.

---

## 2. Root Query Spec Structure

```json
{
  "spec_version": 1,
  "load_csv": ["https://data.example.com/nodes.csv", true, "row"],
  "unwinds": [["$batch", "item"]],
  "matches": [
    {
      "spec_version": 1,
      "optional": false,
      "path_mode": "trail",
      "path_variable": "p",
      "paths": [
        [
          ["node", "u", ["User"]],
          ["edge", "outgoing", ["KNOWS"], "r", 1, 2],
          ["node", "f", ["User"]]
        ]
      ],
      "where": [
        ["bin", "gt", ["prop", "f", "age"], ["lit", 21]]
      ]
    }
  ],
  "with_clauses": [
    {
      "spec_version": 1,
      "distinct": false,
      "projections": [
        ["field", "u", "name", "user_name"],
        ["agg", "f", "id", "count", "friend_count"]
      ],
      "where": [
        ["bin", "gte", ["ident", "friend_count"], ["lit", 5]]
      ],
      "order_by": [
        ["friend_count", "friend_count", false]
      ],
      "limit": 10
    }
  ],
  "projections": [
    ["field", "u", "name", "user_name"],
    ["expr", ["ident", "friend_count"], "total"]
  ],
  "distinct": false,
  "order_by": [
    ["user_name", "user_name", true]
  ],
  "skip": 0,
  "limit": 50,
  "execution_mode": "normal"
}
```

---

## 3. Path Step Tuples

Paths within `matches[i].paths` or `create`/`merge` mutations are sequences of steps:

| Step Type | Shape | Description |
| :--- | :--- | :--- |
| **Node** | `["node", variable, labels]` | Node pattern, e.g. `["node", "u", ["User", "Admin"]]`. `variable` can be `null` for anonymous nodes. |
| **Edge** | `["edge", dir, types, variable, min_hops, max_hops]` | Directed or undirected relationship traversal. `dir` is `"outgoing"`, `"incoming"`, or `"undirected"`. `min_hops` and `max_hops` specify variable-length paths (`null` for 1 hop). |

---

## 4. Expression AST Descriptors

Expression trees used in `where`, `set`, and projections support fine-grained operators:

| Operator | Tuple / Array Shape | Description |
| :--- | :--- | :--- |
| **Property Access** | `["prop", var, property]` | Access entity property: `u.age` |
| **Identifier** | `["ident", name]` | Variable or alias identifier: `friend_count` |
| **Parameter** | `["param", name]` | Query parameter reference: `$param` |
| **Literal** | `["lit", value]` | Scalar literal (`42`, `"Alice"`, `true`, `null`) |
| **Binary Op** | `["bin", op, left, right]` | Binary operators: `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, `and`, `or`, `xor`, `add`, `sub`, `mul`, `div`, `mod`, `contains`, `starts_with`, `ends_with`, `regex`, `in` |
| **Unary Op** | `["unary", op, operand]` | `not`, `negate`, `is_null`, `is_not_null` |
| **Function** | `["fn", name, [args...]]` | Function call: `["fn", "toUpper", [["prop", "u", "name"]]]` |
| **CASE WHEN** | `["case", operand, [[when, then]...], else]` | Conditional branching |
| **List Literal** | `["list", [items...]]` | Literal list of expressions |
| **Subquery Exists**| `["exists", subquery_spec]` | Existential graph subquery |
| **Subquery Count** | `["count", subquery_spec]` | Scalar subquery count |
| **Alias** | `["alias", inner, alias_name]` | Projection aliasing |

---

## 5. DML Mutations

Mutation tuples within `mutations`:

- **CREATE**: `["create", paths_list]`
- **MERGE**: `["merge", path_steps, on_create_sets, on_match_sets]`
- **SET**: `["set", variable, property, value_expression]`
- **DELETE**: `["delete", detach_bool, [target_vars...]]`
- **REMOVE**: `["remove", variable, property]`

---

## 6. Target Dialect Parity

Any valid `spec` compiled with `compile_query_from_spec(spec, dialect="cypher" | "iso_gql" | "sql_pgq" | "apache_age")` produces the exact same parameter-isolated SQL/Cypher statement as the fluent `Query` builder API.
