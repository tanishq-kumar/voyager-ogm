# Voyager OGM Query Spec Interchange Protocol (v1)

## Overview

The **Query Spec Interchange Protocol** is the canonical, language-agnostic Abstract Syntax Tree (AST) exchange contract for Voyager OGM. It defines how graph queries, traversal chains, filter predicates, DML mutations, aggregations, and pagination are serialized across FFI boundaries between:

- Python SDK (`voyager-ogm`)
- TypeScript / Node.js SDK (`@voyager-ogm/client`)
- Native Rust Core Engine (`voyager-ogm` / `voyager-pyo3`)
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

## Errors

Every structural failure in the Query Spec v1 consumer raises `VoyagerError`
(a `ValueError` subclass) with:

- `code == "INVALID_SPEC"`.
- `path` set to a JSON Pointer to the offending element.
- the same pointer in the message, prefixed with `Invalid query spec at`.

The document root is the empty pointer `""`. Object members append `/name` and
array items append `/index`, so `/matches/0/paths/1/2` is the third step of the
second path of the first match. JSON Pointer escaping applies (`~` becomes
`~0`, `/` becomes `~1`).

Messages name JSON types (`string`, `number`, `boolean`, `null`, `array`,
`object`), not Python class names, because spec authors write JSON. An
expression-position rejection also names the explicit spelling to use:
`["lit", value]` for a scalar and `["lit", [...]]` / `["list", [...]]` for a
list.

Examples:

| Spec | `path` |
| :--- | :--- |
| `{"spec_version": 1, "bogus": 1}` | `/bogus` |
| `{"spec_version": 1}` (empty spec) | `""` |
| `{"matches": []}` (missing `spec_version`) | `/spec_version` |
| `{"spec_version": 1, "matches": 42}` | `/matches` |
| `{"spec_version": 1, "matches": [{"paths": [[["node","n",["N"]], ["edge","out",["R"]]]]}]}` | `/matches/0/paths/0/1` |
| `{"spec_version": 1, "skip": -1, ...}` | `/skip` |

The following are rejected:

- Unknown object keys at the root, in a match clause, in a with clause, and in
  an object-wrapped expression. The key sets are closed.
- Wrong container types for `load_csv`, `unwinds`, `matches`, `paths`,
  `mutations`, `with_clauses`, `projections`, `order_by`, `linear_clauses`, and
  merge set lists.
- Unknown enum values: `path_mode`, `execution_mode`, `linear_clauses` tags,
  mutation tags, path step tags, projection tags, and aggregation functions.
- Negative or boolean `skip` / `offset` / `limit` values.
- A bare or untagged array in an expression position: an empty array, a non-string
  first element (for example `[1, 2, 3]`) or an unknown first string (for
  example `["propp", ...]` or `["red", "green"]`). Every expression array must
  be tagged (see [Expression arrays are always tagged](#expression-arrays-are-always-tagged)).
- A `where` array whose first element is a scalar other than a string (for
  example `[1, 2, 3]`), which is neither one expression nor an array of
  expressions (see [`where`: one expression or a list](#where-one-expression-or-a-list-of-expressions)).
- An **empty spec**: a document that has none of `load_csv`, `unwinds`,
  `matches`, `mutations`, `with_clauses`, `projections`, or `linear_clauses`
  with content. `{"spec_version": 1}` and `{"spec_version": 1, "matches": []}`
  are both empty. Pagination and `execution_mode` alone do not make a spec
  non-empty.

`INVALID_CLAUSE` remains the code for non-spec callers, such as the fluent
Python/Rust builder API.

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

### ORDER BY entries

`order_by` appears at the root and inside each `with_clauses[i]`. Every entry is one of two shapes:

| Shape | Description |
| :--- | :--- |
| `[variable, property, direction]` | Sort by a property of a bound variable, e.g. `["p", "name", true]`. |
| `[expression, direction]` | Sort by an expression descriptor, e.g. `[["fn", "size", [["prop", "p", "name"]]], "DESC"]`. |

`direction` accepts both a boolean and a string: `true` means ascending, `false` means descending, and the strings `"ASC"` / `"DESC"` are exact, uppercase spellings of the same two directions. A boolean and its string counterpart compile to the same statement, so `["p", "name", true]` and `["p", "name", "ASC"]` are equivalent. Any other value (for example `"asc"`, `"up"` or `1`) is rejected with an `INVALID_SPEC` error at the direction element (for example `/order_by/0/2`).

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

### Expression arrays are always tagged

An expression position never accepts an untagged JSON array. Every array there
must be a tagged expression descriptor: its first element is a string that is a
known expression tag. The array spelling may be a Python `list` (the JSON form)
or a `tuple` (the form the Python SDK emits), and the tag rule is the same for
both.

Valid (a literal list, and a list whose items are expressions):

```json
["lit", [1, 2, 3]]
["list", [["lit", 1], ["lit", 2], ["lit", 3]]]
```

Invalid (`INVALID_SPEC` at the path of the array itself):

```json
[1, 2, 3]
```

An empty array, a non-string first element and an unknown first string (for
example `["propp", "p", "age"]` or `["red", "green"]`) are all rejected. Write a
literal list as `["lit", [...]]`; write a list whose items are expressions as
`["list", [...]]`.

### `where`: one expression or a list of expressions

The schema allows `where` to be either one `Expression` or an array of
`Expression`s. A single expression is itself a JSON array starting with its tag
string, so the form is chosen deterministically from the first element:

| `where` value | Meaning |
| :--- | :--- |
| `[]` | An empty predicate list; no predicate is emitted. |
| `["bin", ...]` (first element is a string) | One expression. |
| `[["bin", ...], ["bin", ...]]` (first element is an array or object) | A list of expressions. |
| anything else, such as `[1, 2, 3]` | `INVALID_SPEC` at the `where` path. |

These two forms compile to identical Cypher:

```json
"where": ["bin", "gt", ["prop", "f", "age"], ["lit", 21]]
"where": [["bin", "gt", ["prop", "f", "age"], ["lit", 21]]]
```

The rule is the same for `matches[i].where` and `with_clauses[i].where`, and for
the Python tuple spelling of either form. An empty array remains an empty
predicate list in both spellings.

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
