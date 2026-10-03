"""Example 16: Cross-Language Spec Interchange & Compilation ABI.

Demonstrates Voyager's serialized Query Spec interchange contract:
1. Construct a canonical AST Query Spec representing a graph traversal and predicates.
2. Use expression specs (`expr.to_spec()`) for fine-grained predicate representation.
3. Compile directly from the spec dictionary using the native Rust engine (`compile_query_from_spec()`).
4. Prove deterministic parity across graph dialects (Cypher, ISO GQL) matching the fluent Query builder.
5. Mutate the spec dynamically and compile without re-instantiating Python model objects.

Run with: `uv run python examples/python/16_spec_interchange.py`
"""

from __future__ import annotations

import pprint

from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    compile_query_from_spec,
    node,
    relationship,
)


@node(labels=["Person"])
class Person(Node):
    name: str = Field(primary_key=True)
    age: int = Field(default=30)


@node(labels=["Movie"])
class Movie(Node):
    title: str = Field(primary_key=True)


@relationship(type_name="DIRECTED")
class Directed(Relationship):
    pass


def main() -> None:
    print("=" * 70)
    print("[Example 16] Query Spec Interchange & Cross-Language ABI")
    print("=" * 70)

    p = Person(alias="d")
    m = Movie(alias="m")

    # 1. Individual expression specs can be derived via expr.to_spec()
    pred_spec = (p.age >= 40).to_spec()
    print("1. Predicate Expression Spec (from p.age >= 40):")
    print(f"   {pred_spec}\n")

    # 2. Canonical Query Spec interchange format
    # This JSON/dict schema represents the cross-language ABI consumed by TypeScript SDK,
    # MCP tools, and external services.
    spec = {
        "matches": [
            {
                "optional": False,
                "paths": [
                    [
                        ("node", "d", ["Person"]),
                        ("edge", "outgoing", ["DIRECTED"], "r", None, None),
                        ("node", "m", ["Movie"]),
                    ]
                ],
                "where": [pred_spec],
            }
        ],
    }

    print("2. Canonical Query Spec Dictionary:")
    pprint.pprint(spec, indent=3)
    print()

    # 3. Compile directly from the spec dictionary across multiple dialects
    res_cypher = compile_query_from_spec(spec, dialect="cypher")
    res_gql = compile_query_from_spec(spec, dialect="iso_gql")

    # 4. Compare with the equivalent fluent Query builder output
    q = Query.match(p).to(Directed, "r").node(m).where(p.age >= 40)
    direct_cypher = q.compile("cypher")
    direct_gql = q.compile("iso_gql")

    assert res_cypher["statement"] == direct_cypher.statement, "Cypher parity mismatch!"
    assert res_gql["statement"] == direct_gql.statement, "ISO GQL parity mismatch!"

    print("3. Parity Verification:")
    print("   [PASS] Cypher Spec Output == Fluent Builder Output")
    print(f"          Statement: {res_cypher['statement']}")
    print(f"          Params:    {res_cypher['parameters']}")
    print("   [PASS] ISO GQL Spec Output == Fluent Builder Output")
    print(f"          Statement: {res_gql['statement']}")
    print(f"          Params:    {res_gql['parameters']}\n")

    # 5. Mutate spec dictionary directly (e.g. as a TypeScript SDK or MCP tool would)
    print("4. Spec Mutation across ABI Boundary:")
    mutated_spec = {
        "matches": [
            {
                "optional": False,
                "paths": [
                    [
                        ("node", "d", ["Person"]),
                        ("edge", "outgoing", ["DIRECTED"], "r", None, None),
                        ("node", "m", ["Movie"]),
                    ]
                ],
                "where": [
                    (
                        "bin",
                        "and",
                        pred_spec,
                        ("bin", "eq", ("prop", "m", "title"), ("lit", "Matrix")),
                    )
                ],
            }
        ],
    }
    res_mutated = compile_query_from_spec(mutated_spec, dialect="cypher")
    print(f"   Mutated Cypher Statement: {res_mutated['statement']}")
    print(f"   Mutated Parameters:       {res_mutated['parameters']}")
    print("=" * 70)


if __name__ == "__main__":
    main()
