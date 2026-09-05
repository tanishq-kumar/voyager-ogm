"""Tests for the Multi-Database Batch Identity Map and Active Record / Data Mapper fusion."""

from __future__ import annotations

import gc

import pytest
from voyager_ogm.bridge import MockBridge
from voyager_ogm.models import Field, Node
from voyager_ogm.session import AsyncSession, Session


class Person(Node):
    """Test person node model."""

    id: str = Field(primary_key=True)
    name: str = Field(index=True)
    age: int = Field(default=0)
    salary: float = Field(default=0.0)


class Company(Node):
    """Test company node model."""

    id: str = Field(primary_key=True)
    title: str = Field(index=True)
    industry: str = Field(default="Tech")


class TestIdentityMapBasics:
    """Verifies default disabled state, opt-in activation, and registration."""

    def test_default_disabled(self) -> None:
        """By default, Identity Map is disabled to guarantee zero memory overhead."""
        session = Session()
        assert not session.identity_map_enabled

        p = Person(id="p1", name="Alice", age=30)
        registered = session.register(p)
        assert registered is p
        assert session.get_node(Person, "p1") is None
        assert session.flush() == []

    def test_opt_in_enabled(self) -> None:
        """When enabled, session tracks registered instances in Identity Map."""
        session = Session(enable_identity_map=True)
        assert session.identity_map_enabled

        p = Person(id="p1", name="Alice", age=30)
        registered = session.register(p)
        assert registered is p

        tracked = session.get_node(Person, "p1")
        assert tracked is p
        assert tracked.get("name") == "Alice"

    def test_deduplication_and_in_memory_identity(self) -> None:
        """Registering a new instance with existing key updates and returns the tracked instance."""
        session = Session(enable_identity_map=True)

        p1 = Person(id="p1", name="Alice", age=30)
        session.register(p1)

        p2 = Person(id="p1", name="Alice Smith", age=31)
        res = session.register(p2)

        # Should return the original in-memory instance p1 with updated fields
        assert res is p1
        assert p1.name == "Alice Smith"
        assert p1.age == 31


class TestBatchCoalescingFlush:
    """Verifies that flush coalesces N dirty nodes into 1 vectorized UNWIND batch."""

    def test_single_model_batch_flush_cypher(self) -> None:
        """Flushing 3 dirty Person nodes executes 1 single UNWIND batch, not 3 network calls."""
        mock_bridge = MockBridge()
        session = Session(bridge=mock_bridge, dialect="cypher", enable_identity_map=True)

        p1 = Person(id="p1", name="Alice", age=30)
        p2 = Person(id="p2", name="Bob", age=25)
        p3 = Person(id="p3", name="Charlie", age=35)

        session.register(p1)
        session.register(p2)
        session.register(p3)

        # Mutate properties
        p1.age = 31
        p2.age = 26
        p3.name = "Charles"

        assert len(p1.dirty_fields) > 0
        assert len(p2.dirty_fields) > 0
        assert len(p3.dirty_fields) > 0

        # Flush
        results = session.flush(key_field="id")
        assert len(results) == 1
        res = results[0]
        assert res.total_records == 3
        assert res.total_batches == 1

        # Verify dirty fields were cleared upon successful flush
        assert p1.dirty_fields == {}
        assert p2.dirty_fields == {}
        assert p3.dirty_fields == {}

    def test_multi_model_coalescing_flush(self) -> None:
        """Flushing dirty instances across multiple models executes exactly 1 batch per model."""
        mock_bridge = MockBridge()
        session = Session(bridge=mock_bridge, dialect="cypher", enable_identity_map=True)

        p1 = Person(id="p1", name="Alice", age=30)
        p2 = Person(id="p2", name="Bob", age=25)
        c1 = Company(id="c1", title="Acme Inc", industry="AI")
        c2 = Company(id="c2", title="Globex", industry="Cloud")

        session.register(p1, is_clean=True)
        session.register(p2, is_clean=True)
        session.register(c1, is_clean=True)
        session.register(c2, is_clean=True)

        p1.age = 31
        c1.industry = "Robotics"

        results = session.flush(key_field="id")
        # Exactly 2 batches: 1 for Person, 1 for Company
        assert len(results) == 2
        total_recs = sum(r.total_records for r in results)
        assert total_recs == 2

        assert p1.dirty_fields == {}
        assert c1.dirty_fields == {}

    def test_iso_gql_dialect_batch_flush(self) -> None:
        """Batch flush works across ISO GQL standard dialect."""
        mock_bridge = MockBridge()
        session = Session(bridge=mock_bridge, dialect="iso_gql", enable_identity_map=True)

        p1 = Person(id="p1", name="Alice", age=30)
        session.register(p1)
        p1.age = 32

        results = session.flush(key_field="id")
        assert len(results) == 1
        assert results[0].total_records == 1
        assert p1.dirty_fields == {}


class TestActiveRecordErgonomics:
    """Verifies node.save() and session attachment."""

    def test_save_with_attached_session(self) -> None:
        """node.save() uses attached session to perform batched upsert."""
        mock_bridge = MockBridge()
        session = Session(bridge=mock_bridge, enable_identity_map=True)

        p = Person(id="p1", name="Alice", age=30)
        session.register(p)

        p.age = 31
        res = p.save()
        assert res is not None
        assert res.total_records == 1
        assert p.dirty_fields == {}

    def test_save_with_explicit_session(self) -> None:
        """node.save(session=session) persists through explicit session."""
        mock_bridge = MockBridge()
        session = Session(bridge=mock_bridge)

        p = Person(id="p1", name="Alice", age=30)
        p.age = 32
        res = p.save(session=session)
        assert res is not None
        assert res.total_records == 1
        assert p.dirty_fields == {}

    def test_save_without_session_raises(self) -> None:
        """Calling save() without any active session raises RuntimeError."""
        p = Person(id="p1", name="Alice", age=30)
        p.age = 31
        with pytest.raises(RuntimeError, match="No active Session attached"):
            p.save()

    def test_save_clean_node_is_noop(self) -> None:
        """Calling save() on a clean node with no dirty fields returns None without network calls."""
        mock_bridge = MockBridge()
        session = Session(bridge=mock_bridge, enable_identity_map=True)

        p = Person(id="p1", name="Alice", age=30)
        session.register(p)
        p.clear_dirty()

        assert p.save() is None


class TestWeakRefMemoryReclaim:
    """Verifies that tracked entities are automatically freed by Python GC."""

    def test_weakref_garbage_collection(self) -> None:
        """When user code drops entity references, identity map automatically purges them."""
        session = Session(enable_identity_map=True)

        def create_and_register() -> None:
            p = Person(id="temp_p1", name="Temp", age=20)
            session.register(p)
            assert session.get_node(Person, "temp_p1") is not None

        create_and_register()
        gc.collect()

        # After out-of-scope and gc, weakref is automatically reclaimed
        assert session.get_node(Person, "temp_p1") is None


class TestAsyncSessionIdentityMap:
    """Verifies asynchronous session identity map and async_save."""

    @pytest.mark.asyncio
    async def test_async_identity_map_flush(self) -> None:
        mock_bridge = MockBridge()
        session = AsyncSession(bridge=mock_bridge, dialect="cypher", enable_identity_map=True)

        p1 = Person(id="p1", name="Alice", age=30)
        p2 = Person(id="p2", name="Bob", age=25)

        session.register(p1)
        session.register(p2)

        p1.age = 31
        p2.age = 26

        results = await session.flush(key_field="id")
        assert len(results) == 1
        assert results[0].total_records == 2
        assert p1.dirty_fields == {}
        assert p2.dirty_fields == {}

        await session.close()

    @pytest.mark.asyncio
    async def test_async_save(self) -> None:
        mock_bridge = MockBridge()
        session = AsyncSession(bridge=mock_bridge, enable_identity_map=True)

        p = Person(id="p1", name="Alice", age=30)
        session.register(p)
        p.age = 35

        res = await p.async_save()
        assert res is not None
        assert res.total_records == 1
        assert p.dirty_fields == {}

        await session.close()


class TestLiveDatabaseIdentityMap:
    """Live database integration tests for Identity Map with real database engines."""

    def test_live_duckdb_batch_identity_map(self) -> None:
        """Tests Batch Identity Map with in-memory DuckDB relational-graph engine."""
        try:
            import duckdb
        except ImportError:
            pytest.skip("duckdb not installed")

        con = duckdb.connect(":memory:")
        session = Session(bridge=con, enable_identity_map=True)

        p1 = Person(id="duck_1", name="Donald", age=40)
        p2 = Person(id="duck_2", name="Daisy", age=38)

        session.register(p1)
        session.register(p2)

        p1.age = 41
        p2.name = "Daisy Duck"

        results = session.flush(key_field="id")
        assert len(results) == 1
        assert results[0].total_records == 2

        assert p1.dirty_fields == {}
        assert p2.dirty_fields == {}

        session.close()

    def test_live_neo4j_batch_identity_map(self) -> None:
        """Tests Batch Identity Map against live Neo4j container."""
        try:
            from neo4j import GraphDatabase

            driver = GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "voyagerpass123"))
            driver.verify_connectivity()
        except Exception:
            pytest.skip("Neo4j database not reachable at bolt://127.0.0.1:7687")

        session = Session(bridge=driver, dialect="cypher", enable_identity_map=True)

        p = Person(id="neo_live_1", name="Neo Person", age=100)
        session.register(p)
        p.age = 101

        results = session.flush(key_field="id")
        assert len(results) == 1
        assert results[0].total_records == 1
        assert p.dirty_fields == {}

        # Verify query
        res = session.execute("MATCH (n:Person {id: 'neo_live_1'}) RETURN n.age AS age")
        assert res.fetchone() == {"age": 101}

        session.close()
