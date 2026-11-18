"""Regression tests for #178: register/flush/save must use the declared primary key."""

from __future__ import annotations

import pytest
from voyager_ogm.bridge import AsyncMockBridge, MockBridge
from voyager_ogm.models import Field, Node
from voyager_ogm.session import AsyncSession, Session


class User(Node):
    """Node whose primary key is not named 'id'."""

    user_id: str = Field(primary_key=True)
    username: str = Field()


class LegacyEntity(Node):
    """Node that declares no primary key but does declare an 'id' field."""

    id: str = Field()
    label: str = Field()


class TestRegisterPrimaryKey:
    """Verifies register() auto-detects the declared primary key."""

    def test_register_and_get_by_primary_key(self) -> None:
        """A model with user_id as primary key is registered under user_id, not id."""
        session = Session(enable_identity_map=True)
        user = User(user_id="usr_001", username="ada")

        assert session.register(user) is user
        assert session.get(User, "usr_001") is user
        assert session.get(User, user_id="usr_001") is user
        assert session.get(User, "usr_001") is user

    def test_register_deduplicates_on_primary_key(self) -> None:
        """A second instance with the same primary key resolves to the tracked instance."""
        session = Session(enable_identity_map=True)
        first = User(user_id="usr_001", username="ada")
        session.register(first)

        second = User(user_id="usr_001", username="ada lovelace")
        result = session.register(second)

        assert result is first
        assert first.get("username") == "ada lovelace"

    def test_flush_merges_on_primary_key(self) -> None:
        """Flush writes a MERGE keyed by user_id and clears dirty state."""
        bridge = MockBridge()
        session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)
        user = User(user_id="usr_001", username="ada")
        session.register(user)

        user.username = "ada_l"
        results = session.flush()

        assert len(results) == 1
        assert results[0].total_records == 1
        assert user.dirty_fields == {}

        statement, params = bridge.executed_queries[0]
        assert "user_id" in statement
        assert params["batch"][0]["user_id"] == "usr_001"
        assert params["batch"][0]["username"] == "ada_l"

    def test_save_uses_primary_key(self) -> None:
        """Node.save() defaults to the model's primary key."""
        bridge = MockBridge()
        session = Session(bridge=bridge, enable_identity_map=True)
        user = User(user_id="usr_001", username="ada")
        session.register(user, is_clean=True)

        user.username = "ada_l"
        result = user.save()

        assert result is not None
        assert result.total_records == 1
        statement, params = bridge.executed_queries[0]
        assert "user_id" in statement
        assert params["batch"][0]["user_id"] == "usr_001"


class TestKeyFieldFallbacks:
    """Verifies the no-primary-key fallback and explicit overrides."""

    def test_model_without_primary_key_falls_back_to_id(self) -> None:
        """Without Field(primary_key=True), register falls back to the 'id' field."""
        session = Session(enable_identity_map=True)
        entity = LegacyEntity(id="legacy-1", label="old")

        assert session.register(entity) is entity
        assert session.get(LegacyEntity, "legacy-1") is entity

    def test_explicit_key_field_overrides_primary_key(self) -> None:
        """An explicit key_field keeps working and wins over the declared primary key."""
        session = Session(enable_identity_map=True)
        user = User(user_id="usr_001", username="ada")

        assert session.register(user, key_field="username") is user
        assert session.get(User, "ada") is user
        assert session.get(User, "usr_001") is None


class TestAsyncRegisterPrimaryKey:
    """Verifies AsyncSession inherits the same primary key resolution."""

    @pytest.mark.asyncio
    async def test_async_register_and_get_by_primary_key(self) -> None:
        session = AsyncSession(enable_identity_map=True)
        user = User(user_id="usr_001", username="ada")

        assert session.register(user) is user
        assert session.get(User, "usr_001") is user
        assert session.get(User, user_id="usr_001") is user

        await session.close()

    @pytest.mark.asyncio
    async def test_async_flush_merges_on_primary_key(self) -> None:
        bridge = AsyncMockBridge()
        session = AsyncSession(bridge=bridge, dialect="cypher", enable_identity_map=True)
        user = User(user_id="usr_001", username="ada")
        session.register(user)

        user.username = "ada_l"
        results = await session.flush()

        assert len(results) == 1
        assert results[0].total_records == 1

        statement, params = bridge.executed_queries[0]
        assert "user_id" in statement
        assert params["batch"][0]["user_id"] == "usr_001"

        await session.close()

    @pytest.mark.asyncio
    async def test_async_save_uses_primary_key(self) -> None:
        bridge = AsyncMockBridge()
        session = AsyncSession(bridge=bridge, enable_identity_map=True)
        user = User(user_id="usr_001", username="ada")
        session.register(user, is_clean=True)

        user.username = "ada_l"
        result = await user.async_save()

        assert result is not None
        assert result.total_records == 1
        statement, params = bridge.executed_queries[0]
        assert "user_id" in statement
        assert params["batch"][0]["user_id"] == "usr_001"

        await session.close()
