"""Tests for Voyager OGM Python Type Annotations and Schema Reflection."""

from __future__ import annotations

from pathlib import Path

import pytest
import voyager_ogm
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    node,
    relationship,
    reset_alias_counters,
)


@pytest.fixture(autouse=True)
def reset_aliases():
    reset_alias_counters()


# Pure Python type annotations without writing Field()
@node
class Developer:
    name: str
    age: int
    skills: list[str]
    level: str = "Senior"


# Mix of type annotations and explicit Field constraints
@node(label="Software")
class Project:
    title: str = Field(unique=True)
    stars: int = Field(default=0, index=True)
    active: bool = True


@relationship
class ContributedTo:
    commits: int
    role: str = "Author"


def test_pure_type_annotation_field_detection():
    dev = Developer()
    assert isinstance(dev, Node)
    assert dev.alias == "_developer_0"
    assert dev.labels == ["Developer"]

    # Verify type-safe expression generation works on pure type annotations
    expr1 = dev.age > 25
    assert expr1.target == "_developer_0"
    assert expr1.field == "age"
    assert expr1.op == "gt"
    assert expr1.value == 25

    expr2 = dev.name == "Linus"
    assert expr2.op == "eq"
    assert expr2.value == "Linus"


def test_schema_reflection_metadata():
    assert "name" in Developer._schema_fields
    assert "age" in Developer._schema_fields
    assert "skills" in Developer._schema_fields
    assert "level" in Developer._schema_fields

    assert Developer._schema_fields["name"].type_annotation is str
    assert Developer._schema_fields["age"].type_annotation is int

    assert Project._schema_fields["title"].unique is True
    assert Project._schema_fields["stars"].index is True


def test_query_with_pure_type_annotated_models():
    d = Developer()
    p = Project()
    c = ContributedTo()

    query = (
        Query.match(d)
        .to(c)
        .node(p)
        .where(d.age >= 21, p.stars > 100)
        .return_(d.name, p.title, commits=c.commits)
    )

    compiled = query.compile("cypher")
    expected = (
        "MATCH (_developer_0:Developer)-[_contributedto_0:CONTRIBUTEDTO]->(_software_0:Software) "
        "WHERE (_developer_0.age >= $p0) AND (_software_0.stars > $p1) "
        "RETURN _developer_0.name, _software_0.title, _contributedto_0.commits AS commits"
    )
    assert compiled.statement == expected
    assert compiled.parameters == {"p0": 21, "p1": 100}


def test_pep_561_py_typed_marker():
    """Verify that PEP 561 py.typed marker file exists in the package root."""
    package_dir = Path(voyager_ogm.__file__).parent
    py_typed_file = package_dir / "py.typed"
    assert py_typed_file.exists(), f"py.typed marker not found in {package_dir}"
    assert py_typed_file.is_file()


def test_pep_681_dataclass_transform_metadata():
    """Verify that PEP 681 dataclass_transform decorator metadata is attached."""
    for target in (Node, Relationship, node, relationship):
        dct = getattr(target, "__dataclass_transform__", None)
        assert dct is not None, f"__dataclass_transform__ missing on {target}"
        assert Field in dct.get("field_specifiers", ())
        assert dct.get("eq_default") is True


def test_pep_681_model_instantiation():
    """Verify that models decorated with PEP 681 dataclass transform can be instantiated."""
    dev = Developer(name="Alice", age=30, skills=["Rust", "Python"], level="Lead")
    assert dev.get("name") == "Alice"
    assert dev.get("age") == 30
    assert dev.get("skills") == ["Rust", "Python"]
    assert dev.get("level") == "Lead"

    rel = ContributedTo(commits=42, role="Maintainer")
    assert rel.get("commits") == 42
    assert rel.get("role") == "Maintainer"
