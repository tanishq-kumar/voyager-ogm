"""Native schema identity, primary-key, and MERGE-key resolution.

These tests cover models with the same class name in different modules and
verify that the Python SDK asks the native schema registry for key policy
instead of deriving it from ``_schema_fields``.
"""

from __future__ import annotations

import types
from typing import Any

import pytest
from voyager_ogm import Field, NativeSchemaRegistry, Node, Query, Session, reset_alias_counters
from voyager_ogm.bridge import MockBridge
from voyager_ogm.models import _model_identity, get_primary_key_field


def _make_user_model(module_name: str, primary_key: str) -> type[Node]:
    """Builds a ``User`` model with the given primary key inside a fresh module."""
    module = types.ModuleType(module_name)
    exec(
        "\n".join(
            [
                "from voyager_ogm import Field, Node",
                "",
                "class User(Node):",
                f"    {primary_key}: str = Field(primary_key=True)",
                "    name: str = Field()",
            ]
        ),
        module.__dict__,
    )
    return module.User


def _make_follows_model(module_name: str) -> type[Any]:
    """Builds a ``Follows`` relationship with a primary key in a fresh module."""
    module = types.ModuleType(module_name)
    exec(
        "\n".join(
            [
                "from voyager_ogm import Field, Relationship",
                "",
                "class Follows(Relationship):",
                "    follow_id: str = Field(primary_key=True)",
                "    weight: float = Field()",
            ]
        ),
        module.__dict__,
    )
    return module.Follows


@pytest.fixture(autouse=True)
def _reset_aliases() -> None:
    reset_alias_counters()


@pytest.fixture(scope="module")
def same_named_models() -> tuple[type[Node], type[Node]]:
    return (
        _make_user_model("native_schema_identity.email_users", "email"),
        _make_user_model("native_schema_identity.login_users", "username"),
    )


def test_same_named_models_get_separate_native_entries(
    same_named_models: tuple[type[Node], type[Node]],
) -> None:
    email_user, login_user = same_named_models
    registry = NativeSchemaRegistry.global_registry()
    email_user.register_schema()
    login_user.register_schema()

    email_schema = registry.get_node(_model_identity(email_user))
    login_schema = registry.get_node(_model_identity(login_user))

    assert email_schema is not None
    assert login_schema is not None
    assert email_schema["name"] == "User"
    assert login_schema["name"] == "User"
    assert email_schema["primary_key"] == "email"
    assert login_schema["primary_key"] == "username"


def test_same_named_relationships_get_separate_native_entries() -> None:
    first = _make_follows_model("native_schema_identity.social_follows")
    second = _make_follows_model("native_schema_identity.feed_follows")
    first.register_schema()
    second.register_schema()

    registry = NativeSchemaRegistry.global_registry()
    first_schema = registry.get_relationship(_model_identity(first))
    second_schema = registry.get_relationship(_model_identity(second))

    assert first_schema is not None
    assert second_schema is not None
    assert first_schema["name"] == "Follows"
    assert second_schema["name"] == "Follows"
    assert first_schema["type_name"] == "FOLLOWS"
    assert second_schema["type_name"] == "FOLLOWS"
    assert get_primary_key_field(first) == "follow_id"
    assert get_primary_key_field(second) == "follow_id"


def test_each_model_resolves_its_own_primary_key(
    same_named_models: tuple[type[Node], type[Node]],
) -> None:
    email_user, login_user = same_named_models

    assert get_primary_key_field(email_user) == "email"
    assert get_primary_key_field(login_user) == "username"


def test_query_merge_uses_each_models_native_key(
    same_named_models: tuple[type[Node], type[Node]],
) -> None:
    email_user, login_user = same_named_models

    reset_alias_counters()
    email_compiled = Query.merge(email_user(email="ada@example.com", name="Ada")).compile("cypher")
    assert "MERGE (_user_0:User {email: $p0})" in email_compiled.statement
    assert "_user_0.name = $p1" in email_compiled.statement

    reset_alias_counters()
    login_compiled = Query.merge(login_user(username="ada", name="Ada")).compile("cypher")
    assert "MERGE (_user_0:User {username: $p0})" in login_compiled.statement
    assert "_user_0.name = $p1" in login_compiled.statement


def test_identity_map_and_flush_use_each_models_native_key(
    same_named_models: tuple[type[Node], type[Node]],
) -> None:
    email_user, login_user = same_named_models
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)

    email_instance = email_user(email="ada@example.com", name="Ada")
    login_instance = login_user(username="grace", name="Grace")
    session.register(email_instance)
    session.register(login_instance)

    assert session.get(email_user, "ada@example.com") is email_instance
    assert session.get(login_user, "grace") is login_instance

    session.flush()

    statements = [statement for statement, _ in bridge.executed_queries]
    assert any("email" in statement for statement in statements)
    assert any("username" in statement for statement in statements)


def test_session_register_and_save_consult_native_primary_key(
    same_named_models: tuple[type[Node], type[Node]],
) -> None:
    email_user, _ = same_named_models
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)
    instance = email_user(email="save@example.com", name="Ada")
    session.register(instance, is_clean=True)

    instance.name = "Ada Lovelace"
    assert instance.save() is not None

    statement, parameters = bridge.executed_queries[0]
    assert "email" in statement
    assert parameters["batch"][0]["email"] == "save@example.com"


def test_python_delegates_key_policy_to_native(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[tuple[str, tuple[Any, ...]]] = []

    class ModelWithNativeKey(Node):
        native_key: str = Field()
        other: str = Field()

    class FakeRegistry:
        def has_node(self, identity: str) -> bool:
            return True

        def has_relationship(self, identity: str) -> bool:
            return False

        def primary_key_for(self, identity: str) -> str | None:
            calls.append(("primary_key_for", (identity,)))
            return "native_pk"

        def merge_key_for(
            self, identity: str, supplied: list[str], defaulted: list[str]
        ) -> list[str] | None:
            calls.append(("merge_key_for", (identity, tuple(supplied), tuple(defaulted))))
            return ["native_key"]

    fake_registry = FakeRegistry()

    class FakeNativeRegistry:
        @staticmethod
        def global_registry() -> FakeRegistry:
            return fake_registry

    monkeypatch.setattr("voyager_ogm.models.NativeSchemaRegistry", FakeNativeRegistry)

    # This class has a fresh cache, so the native lookup must decide the key.
    assert get_primary_key_field(ModelWithNativeKey) == "native_pk"
    assert [name for name, _ in calls] == ["primary_key_for"]

    compiled = Query.merge(ModelWithNativeKey(native_key="value", other="ignored")).compile(
        "cypher"
    )

    assert (
        "MERGE (_modelwithnativekey_0:ModelWithNativeKey {native_key: $p0})" in compiled.statement
    )
    assert "other" in compiled.statement
    assert (
        "merge_key_for",
        (_model_identity(ModelWithNativeKey), ("native_key", "other"), ()),
    ) in calls


def test_subclass_does_not_inherit_parent_cached_primary_key() -> None:
    class ParentModel(Node):
        parent_id: str = Field(primary_key=True)
        name: str = Field()

    assert get_primary_key_field(ParentModel) == "parent_id"

    class ChildModel(ParentModel):
        parent_id: str = Field()
        child_id: str = Field(primary_key=True)

    assert get_primary_key_field(ChildModel) == "child_id"
    assert get_primary_key_field(ParentModel) == "parent_id"


def test_reregistration_refreshes_cached_primary_key() -> None:
    class RekeyedModel(Node):
        first: str = Field(primary_key=True)
        second: str = Field()

    assert get_primary_key_field(RekeyedModel) == "first"

    first_field = RekeyedModel._schema_fields["first"]
    first_field.primary_key = False
    first_field.unique = False
    RekeyedModel._schema_fields["second"].primary_key = True
    RekeyedModel.register_schema()

    assert get_primary_key_field(RekeyedModel) == "second"
