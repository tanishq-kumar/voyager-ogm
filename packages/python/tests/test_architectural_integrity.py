"""Architectural Integrity & CI Anti-Regression Guard for Voyager OGM.

Enforces RFC-0004 Phase 1: Complete elimination of Python-side query state mirroring.
Guarantees that `Query` instances and classes hold zero dual bookkeeping or parallel
heap state beyond `_native` (the Rust `NativeQueryBuilder`) and optimizer flags
(`_optimize`, `_optimization_level`).
"""

from __future__ import annotations

import copy
import inspect
from typing import Any

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    fn,
    node,
    relationship,
    reset_alias_counters,
)
from voyager_ogm._voyager_rs import NativeQueryBuilder

# Whitelist of strictly authorized instance attributes on Query instances
AUTHORIZED_INSTANCE_ATTRIBUTES: frozenset[str] = frozenset(
    {"_native", "_optimize", "_optimization_level"}
)

# Explicit blacklist of historical / forbidden dual-bookkeeping attributes
FORBIDDEN_MIRRORED_ATTRIBUTES: tuple[str, ...] = (
    "_match_clauses",
    "_where_clauses",
    "_return_fields",
    "_mutations",
    "_linear_clauses",
    "_with_clauses",
    "_order_by_fields",
    "_skip_count",
    "_limit_count",
    "_offset_count",
    "_path_mode",
    "_path_variable",
    "_distinct",
    "_current_clause",
    "_patterns",
    "_projections",
    "_parameters",
    "_labels",
    "_variables",
    "_edges",
    "_hops",
    "_clauses",
    "_statements",
    "_ast",
)


@pytest.fixture(autouse=True)
def reset_aliases():
    reset_alias_counters()


@node(label="Person")
class Person(Node):
    name = Field()
    age = Field()
    city = Field()


@node(label="Company")
class Company(Node):
    name = Field()


@relationship(type_name="KNOWS")
class Knows(Relationship):
    since = Field()


@relationship(type_name="WORKS_AT")
class WorksAt(Relationship):
    role = Field()


def assert_architectural_integrity(query: Query) -> None:
    """Strictly asserts that a Query instance adheres to the RFC-0004 Single Source of Truth invariant."""
    actual_attributes = set(query.__dict__.keys())
    unauthorized = actual_attributes - AUTHORIZED_INSTANCE_ATTRIBUTES
    assert not unauthorized, (
        f"RFC-0004 Architectural Violation: Unauthorized attributes {unauthorized} detected on Query instance! "
        f"Allowed attributes: {sorted(AUTHORIZED_INSTANCE_ATTRIBUTES)}. "
        f"Python-side dual bookkeeping must never be reintroduced."
    )

    # Blacklist check
    for forbidden in FORBIDDEN_MIRRORED_ATTRIBUTES:
        assert not hasattr(query, forbidden), (
            f"RFC-0004 Regression: Forbidden attribute '{forbidden}' found on Query instance!"
        )

    # Delegation check
    assert hasattr(query, "_native"), "Query instance is missing '_native' handle!"
    assert isinstance(query._native, NativeQueryBuilder), (
        f"Expected query._native to be NativeQueryBuilder, got {type(query._native)}"
    )


def test_query_init_integrity():
    """Query initialization must create only authorized attributes."""
    q = Query()
    assert_architectural_integrity(q)
    assert q._optimize is None
    assert q._optimization_level is None


def test_fluent_chaining_lifecycle_integrity():
    """Introspects a Query through an exhaustive multi-clause lifecycle.

    Asserts zero unauthorized heap attributes at every step of the chain.
    """
    p = Person("p")
    p2 = Person("p2")
    c = Company("c")

    q = Query()
    assert_architectural_integrity(q)

    # 1. Match node
    q.match(p)
    assert_architectural_integrity(q)

    # 2. Outgoing traversal
    q.to(Knows, "k")
    assert_architectural_integrity(q)

    # 3. Variable hops
    q.hops(1, 3)
    assert_architectural_integrity(q)

    # 4. Target node
    q.node(p2)
    assert_architectural_integrity(q)

    # 5. Incoming traversal
    q.from_(WorksAt, "w")
    assert_architectural_integrity(q)

    # 6. Company node
    q.node(c)
    assert_architectural_integrity(q)

    # 7. Where relational predicates
    q.where(p.age > 21, p2.age <= 65)
    assert_architectural_integrity(q)

    # 8. Where not
    q.where_not(p2.age < 18)
    assert_architectural_integrity(q)

    # 9. Additional pattern branch
    q.pattern()
    assert_architectural_integrity(q)
    q.match(p).edge(Knows).node(p2)
    assert_architectural_integrity(q)

    # 10. WITH clause
    q.with_(p.name, p2_age=p2.age)
    assert_architectural_integrity(q)

    # 11. LET linear statement
    q.let_("senior", p2.age >= 65)
    assert_architectural_integrity(q)

    # 12. Linear filter
    q.linear_filter(p2.age >= 65)
    assert_architectural_integrity(q)

    # 13. RETURN clause
    q.return_(p.name, p2.age, total=fn.count(c))
    assert_architectural_integrity(q)

    # 14. DISTINCT modifier
    q.distinct()
    assert_architectural_integrity(q)

    # 15. ORDER BY
    q.order_by(p.name, ascending=True)
    assert_architectural_integrity(q)
    q.order_by(p2.age, ascending=False)
    assert_architectural_integrity(q)

    # 16. Pagination (SKIP, LIMIT, OFFSET)
    q.skip(10)
    assert_architectural_integrity(q)
    q.limit(25)
    assert_architectural_integrity(q)
    q.offset(10)
    assert_architectural_integrity(q)

    # 17. Execution modes
    q.explain()
    assert_architectural_integrity(q)
    assert q.execution_mode == "explain"

    q.profile()
    assert_architectural_integrity(q)
    assert q.execution_mode == "explain_and_profile"

    # 18. Optimizer flags
    q.optimize()
    assert_architectural_integrity(q)
    assert q._optimize is True

    q.optimize("aggressive")
    assert_architectural_integrity(q)
    assert q._optimization_level == "aggressive"


def test_mutation_methods_integrity():
    """Mutation methods must strictly delegate to native AST without Python heap mirrors."""
    p = Person("p")

    # CREATE
    q_create = Query.create(p)
    assert_architectural_integrity(q_create)
    assert q_create.has_mutations() is True

    # MERGE
    q_merge = Query.merge(p).on_create_set(p.name == "Alice").on_match_set(p.city == "London")
    assert_architectural_integrity(q_merge)
    assert q_merge.has_mutations() is True

    # SET & REMOVE
    q_set = Query.match(p).set(p.name == "Bob").remove(p.city)
    assert_architectural_integrity(q_set)
    assert q_set.has_mutations() is True

    # DELETE & DETACH DELETE
    q_del = Query.match(p).delete(p)
    assert_architectural_integrity(q_del)
    assert q_del.has_mutations() is True

    q_ddel = Query.match(p).detach_delete(p)
    assert_architectural_integrity(q_ddel)
    assert q_ddel.has_mutations() is True


def test_ingestion_and_batch_methods_integrity():
    """UNWIND and LOAD CSV methods must delegate directly to native builder."""
    q_unwind = Query.unwind("$batch", "item")
    assert_architectural_integrity(q_unwind)

    q_csv = Query.load_csv("file:///data.csv", with_headers=True, alias="row")
    assert_architectural_integrity(q_csv)

    q_yield = Query.match(Person("p")).yield_("node", "score")
    assert_architectural_integrity(q_yield)


def test_traversal_modes_and_path_variables_delegation():
    """Path search modes and path variables must not store Python-side mirrors."""
    p = Person("p")

    # trail
    q_trail = Query.match().trail("path").node(p)
    assert_architectural_integrity(q_trail)

    # simple
    q_simple = Query.match().simple("path").node(p)
    assert_architectural_integrity(q_simple)

    # acyclic
    q_acyc = Query.match().acyclic("path").node(p)
    assert_architectural_integrity(q_acyc)

    # walk
    q_walk = Query.match().walk("path").node(p)
    assert_architectural_integrity(q_walk)

    # path_variable and path_mode
    q_custom = Query.match().path_variable("pvar").path_mode("TRAIL").node(p)
    assert_architectural_integrity(q_custom)


def test_cloning_and_copy_isolation():
    """Query cloning (clone, copy, deepcopy) must maintain complete AST and state isolation.

    Modifying a clone must not contaminate the original query, and clones must satisfy
    the authorized attribute whitelist.
    """
    p = Person("p")
    original = Query.match(p).where(p.age > 21)
    assert_architectural_integrity(original)

    # 1. Query.clone()
    clone1 = original.clone()
    assert_architectural_integrity(clone1)
    assert clone1 is not original
    assert clone1._native is not original._native

    # Mutate clone1
    clone1.where(p.city == "Berlin").return_(p.name).limit(5)
    assert_architectural_integrity(clone1)

    # Verify original was NOT mutated
    orig_compiled = original.compile("cypher")
    clone_compiled = clone1.compile("cypher")
    assert orig_compiled.parameters == {"p0": 21}
    assert "LIMIT" not in orig_compiled.statement
    assert clone_compiled.parameters == {"p0": 21, "p1": "Berlin"}
    assert "LIMIT 5" in clone_compiled.statement

    # 2. copy.copy()
    clone2 = copy.copy(original)
    assert_architectural_integrity(clone2)
    assert clone2 is not original
    assert clone2._native is not original._native

    # 3. copy.deepcopy()
    clone3 = copy.deepcopy(original)
    assert_architectural_integrity(clone3)
    assert clone3 is not original
    assert clone3._native is not original._native


def test_class_level_architectural_cleanliness():
    """Query class itself must not contain mutable shared accumulators (dicts/lists/sets)."""
    for attr_name, value in Query.__dict__.items():
        if attr_name.startswith("__") and attr_name.endswith("__"):
            continue
        # Ensure no mutable collection is held at the class level
        assert not isinstance(value, (list, dict, set, bytearray)), (
            f"Query class holds mutable accumulator '{attr_name}' ({type(value)}), "
            "which risks cross-query state leakage!"
        )


def test_all_public_methods_maintain_attribute_whitelist():
    """Introspects all public methods on Query to guarantee none pollute the instance __dict__."""
    public_methods = [
        name
        for name, member in inspect.getmembers(Query)
        if not name.startswith("_") and (inspect.isfunction(member) or inspect.ismethod(member))
    ]

    p = Person("p")
    dummy_args: dict[str, Any] = {
        "match": (p,),
        "match_node": ("p", ["Person"]),
        "node": (p,),
        "to": (),
        "from_": (),
        "edge": (),
        "hops": (1, 2),
        "pattern": (),
        "path_variable": ("p",),
        "path_mode": ("TRAIL",),
        "trail": ("p",),
        "simple": ("p",),
        "acyclic": ("p",),
        "walk": ("p",),
        "where": (p.age > 20,),
        "where_not": (p.age < 18,),
        "return_": (p.name,),
        "distinct": (),
        "order_by": (p.name,),
        "skip": (5,),
        "limit": (10,),
        "offset": (5,),
        "with_": (p.name,),
        "let_": ("x", 1),
        "linear_filter": (p.age > 20,),
        "create": (p,),
        "merge": (p,),
        "on_create_set": (p.name, "Alice"),
        "on_match_set": (p.name, "Alice"),
        "set": (p.name, "Alice"),
        "delete": (p,),
        "detach_delete": (p,),
        "remove": (p.city,),
        "unwind": ("$batch", "item"),
        "load_csv": ("file:///data.csv",),
        "yield_": ("x",),
        "explain": (),
        "profile": (),
        "optimize": (),
        "clone": (),
        "has_mutations": (),
    }

    for method_name in public_methods:
        if method_name not in dummy_args:
            continue
        q = Query()
        method = getattr(q, method_name)
        args = dummy_args[method_name]
        try:
            method(*args)
        except Exception:
            # Some methods may require prior clause context; we only check if __dict__ was polluted
            pass
        assert_architectural_integrity(q)
