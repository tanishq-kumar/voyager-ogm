"""Tests for Query AST Topology Conformance Validation against SchemaRegistry."""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    SchemaRegistry,
    SchemaValidationError,
    node,
    validate_query,
)


@node(labels=["Person"])
class Person(Node):
    name: str = Field(primary_key=True)
    age: int = Field(default=0)


@node(labels=["Movie"])
class Movie(Node):
    title: str = Field(primary_key=True)
    released: int = Field(default=0)


@node(labels=["Company"])
class Company(Node):
    company_id: str = Field(primary_key=True)
    name: str = Field(default="")


class ActedIn(Relationship, type_name="ACTED_IN"):
    role: str = Field(default="")


@pytest.fixture(autouse=True)
def setup_models() -> None:
    """Ensures test models are registered in the global registry."""
    reg = SchemaRegistry.global_registry()
    Person.register_schema(reg)
    Movie.register_schema(reg)
    Company.register_schema(reg)
    reg.register_relationship(
        name="ActedIn",
        type_name="ACTED_IN",
        source_labels=["Person"],
        target_labels=["Movie"],
        fields={"role": {"type": "STRING"}},
        directed=True,
    )


def test_query_validate_conforming() -> None:
    """Verifies that a query correctly conforming to the schema passes validation."""
    p = Person("p", name="Keanu Reeves")
    m = Movie("m")
    act = ActedIn("a", role="Neo")

    q = Query.match(p).to(act).node(m).where(m.released == 1999).return_(p.name, m.title)

    report = q.validate()
    assert report.is_valid
    assert len(report.errors) == 0
    # Should not raise
    report.raise_if_invalid()


def test_query_validate_unknown_node_label() -> None:
    """Verifies that an unknown node label triggers an error with a fuzzy suggestion."""
    report = validate_query("MATCH (p:Persn) RETURN p")
    assert not report.is_valid
    assert len(report.errors) == 1
    assert "Persn" in report.errors[0]
    assert "Person" in report.errors[0]

    with pytest.raises(SchemaValidationError) as excinfo:
        report.raise_if_invalid()
    assert "UNKNOWN_NODE_LABEL" in str(excinfo.value)
    assert "Did you mean label 'Person'?" in str(excinfo.value)


def test_query_validate_unknown_relationship_type() -> None:
    """Verifies that an unknown relationship type triggers an error with a fuzzy suggestion."""
    p = Person("p")
    m = Movie("m")
    q = Query.match(p).to(edge_type="ACTED_ON").node(m).return_(p.name)

    report = q.validate()
    assert not report.is_valid
    assert len(report.errors) == 1
    assert "ACTED_ON" in report.errors[0]
    assert "ACTED_IN" in report.errors[0]

    with pytest.raises(SchemaValidationError) as excinfo:
        report.raise_if_invalid()
    assert "UNKNOWN_RELATIONSHIP_TYPE" in str(excinfo.value)
    assert "Did you mean relationship 'ACTED_IN'?" in str(excinfo.value)


def test_query_validate_incompatible_endpoints() -> None:
    """Verifies that connecting incompatible endpoints triggers an error."""
    c = Company("c")
    m = Movie("m")
    act = ActedIn("a")
    q = Query.match(c).to(act).node(m).return_(c.name)

    report = q.validate()
    assert not report.is_valid
    assert any("INCOMPATIBLE_ENDPOINTS" in d.code for d in report.diagnostics)
    assert any('cannot connect source label(s) ["Company"]' in err for err in report.errors)


def test_query_validate_property_type_mismatch() -> None:
    """Verifies that filtering with an incompatible property type triggers an error."""
    p = Person("p")
    # Person.age is an integer, but filtered with a string
    q = Query.match(p).where(p.age == "twenty").return_(p.name)

    report = q.validate()
    assert not report.is_valid
    assert any("PROPERTY_TYPE_MISMATCH" in d.code for d in report.diagnostics)
    assert any("expects type INTEGER" in err for err in report.errors)


def test_query_validate_undirected_warning() -> None:
    """Verifies that querying a directed relationship as undirected produces a warning but remains valid."""
    raw = "MATCH (p:Person)-[:ACTED_IN]-(m:Movie) RETURN p, m"
    report = validate_query(raw)
    assert report.is_valid
    assert len(report.warnings) == 1
    assert "UNDIRECTED_TRAVERSAL" in report.diagnostics[0].code
    assert "traversed undirected" in report.warnings[0]


def test_isolated_custom_registry() -> None:
    """Verifies that queries can be validated against an isolated SchemaRegistry."""
    custom_reg = SchemaRegistry()
    # No schemas registered in custom_reg
    p = Person("p")
    m = Movie("m")
    act = ActedIn("a")
    q = Query.match(p).to(act).node(m).return_(p.name)

    # With empty registry, everything is valid because no rules are registered
    report = q.validate(registry=custom_reg)
    assert report.is_valid
    assert len(report.errors) == 0
