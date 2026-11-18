"""Regression tests for the ``__label__`` single-label alias on Node models (#180)."""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    NativeSchemaRegistry,
    Node,
    Query,
    Relationship,
    create_bulk_create_plan,
    node,
    relationship,
    reset_alias_counters,
)


@pytest.fixture(autouse=True)
def _reset_aliases() -> None:
    reset_alias_counters()


class Film(Node):
    __label__ = "Movie"
    title: str = Field()


class Series(Node):
    __labels__ = ["Series", "Show"]
    title: str = Field()


class ConsistentBoth(Node):
    __label__ = "Movie"
    __labels__ = ["Movie", "Film"]
    title: str = Field()


@node("Director")
class DecoratedFilm(Node):
    __label__ = "Movie"
    title: str = Field()


@node("Director")
class DecoratedPlainFilm:
    __label__ = "Movie"
    title = Field()


@node
class PlainFilm:
    __label__ = "Movie"
    title = Field()


class Directed(Relationship):
    __type__ = "DIRECTED"
    year: int = Field()


@relationship("PRODUCED")
class Produced(Relationship):
    __type__ = "HELD"
    year: int = Field()


def test_label_alias_compiles_to_match() -> None:
    film = Film(alias="f")
    compiled = Query.match(film).return_(film.title).compile("cypher")

    assert "MATCH (f:Movie)" in compiled.statement
    assert "MATCH (f:Film)" not in compiled.statement


def test_label_alias_used_by_bulk_create_plan() -> None:
    plan = create_bulk_create_plan(Film, [{"title": "Heat"}])

    assert ":Movie" in plan.statement
    assert ":Film" not in plan.statement


def test_label_alias_registered_in_schema_registry() -> None:
    registry = NativeSchemaRegistry()
    Film.register_schema(registry)

    node_meta = registry.get_node("Film")
    assert node_meta is not None
    assert node_meta["labels"] == ["Movie"]


def test_conflicting_label_and_labels_raise_type_error() -> None:
    with pytest.raises(TypeError, match=r"__label__ must equal __labels__\[0\]"):

        class Conflicting(Node):
            __label__ = "Movie"
            __labels__ = ["Film"]
            title: str = Field()


def test_decorator_label_wins_over_class_body_label() -> None:
    assert DecoratedFilm.__labels__ == ["Director"]
    assert DecoratedFilm._cached_label == "Director"

    film = DecoratedFilm(alias="d")
    compiled = Query.match(film).return_(film.title).compile("cypher")
    assert "MATCH (d:Director)" in compiled.statement


def test_decorator_label_wins_on_plain_class() -> None:
    assert DecoratedPlainFilm.__labels__ == ["Director"]
    assert DecoratedPlainFilm._cached_label == "Director"


def test_bare_node_decorator_honours_class_body_label() -> None:
    assert PlainFilm.__labels__ == ["Movie"]
    assert PlainFilm._cached_label == "Movie"


def test_matching_label_and_labels_are_both_honoured() -> None:
    assert ConsistentBoth.__labels__ == ["Movie", "Film"]
    assert ConsistentBoth._cached_labels == ["Movie", "Film"]
    assert ConsistentBoth._cached_label == "Movie"

    film = ConsistentBoth(alias="c")
    compiled = Query.match(film).return_(film.title).compile("cypher")
    assert "MATCH (c:Movie:Film)" in compiled.statement


def test_labels_form_is_unchanged() -> None:
    assert Series.__labels__ == ["Series", "Show"]
    assert Series._cached_labels == ["Series", "Show"]
    assert Series._cached_label == "Series"

    series = Series(alias="s")
    compiled = Query.match(series).return_(series.title).compile("cypher")
    assert "MATCH (s:Series:Show)" in compiled.statement


def test_relationship_class_body_type_is_honoured() -> None:
    assert Directed.__type__ == "DIRECTED"
    assert Directed._cached_type == "DIRECTED"
    assert Directed().edge_type == "DIRECTED"


def test_relationship_decorator_type_wins_over_class_body_type() -> None:
    assert Produced.__type__ == "PRODUCED"
    assert Produced._cached_type == "PRODUCED"
