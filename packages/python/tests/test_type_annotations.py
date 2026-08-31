"""Tests for Voyager OGM Python Type Annotations and Schema Reflection."""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
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
