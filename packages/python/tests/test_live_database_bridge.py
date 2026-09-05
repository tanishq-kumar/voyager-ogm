"""Live integration tests for Voyager OGM database bridge against real databases.

Requires live database instances running (e.g. via `podman compose up -d`).
Tests are automatically skipped if the live database instances are not reachable.
"""

from __future__ import annotations

import polars as pl
import pytest
from voyager_ogm import (
    AsyncSession,
    Field,
    Node,
    Query,
    Relationship,
    Session,
    node,
    relationship,
    reset_alias_counters,
)

try:
    from neo4j import AsyncGraphDatabase, GraphDatabase

    NEO4J_AVAILABLE = True
except ImportError:
    NEO4J_AVAILABLE = False


NEO4J_URI = "bolt://127.0.0.1:7687"
NEO4J_AUTH = ("neo4j", "voyagerpass123")


def _is_neo4j_online() -> bool:
    """Checks whether a live Neo4j database instance is running and accepting connections."""
    if not NEO4J_AVAILABLE:
        return False
    try:
        driver = GraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH)
        driver.verify_connectivity()
        driver.close()
        return True
    except Exception:
        return False


NEO4J_ONLINE = _is_neo4j_online()


@node("LivePerson")
class LivePerson(Node):
    id: int = Field(primary_key=True)
    name: str
    age: int


@relationship("KNOWS")
class Knows(Relationship):
    since: int


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


@pytest.fixture
def clean_neo4j():
    """Wipes the test database before and after test execution."""
    if NEO4J_ONLINE:
        driver = GraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH)
        with driver.session() as s:
            s.run("MATCH (n) DETACH DELETE n")
        yield driver
        with driver.session() as s:
            s.run("MATCH (n) DETACH DELETE n")
        driver.close()
    else:
        yield None


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
def test_live_neo4j_sync_crud(clean_neo4j):
    """Test synchronous query compilation, execution, and Polars streaming against real Neo4j."""
    session = Session(bridge=clean_neo4j, dialect="cypher")

    # 1. Insert live records
    session.execute("CREATE (p:LivePerson {id: 1, name: 'Alice', age: 30})")
    session.execute("CREATE (p:LivePerson {id: 2, name: 'Bob', age: 25})")

    # 2. Query via Voyager Query Builder
    p = LivePerson()
    query = Query.match(p).where(p.age >= 30).return_(p.alias)
    records = session.execute(query)

    assert len(records) >= 1
    assert any(r.get(p.alias, {}).get("name") == "Alice" for r in records)

    # 3. Stream query results directly into Polars DataFrame
    df = session.execute_to_polars(
        "MATCH (p:LivePerson) RETURN p.id AS id, p.name AS name, p.age AS age ORDER BY id"
    )
    assert isinstance(df, pl.DataFrame)
    assert len(df) == 2
    assert df["name"].to_list() == ["Alice", "Bob"]
    assert df["age"].to_list() == [30, 25]


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
@pytest.mark.asyncio
async def test_live_neo4j_async_crud():
    """Test asynchronous non-blocking query execution against real Neo4j."""
    async_driver = AsyncGraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH)
    try:
        session = AsyncSession(bridge=async_driver, dialect="cypher")

        # Insert record asynchronously
        await session.execute("CREATE (p:LivePerson {id: 99, name: 'Trinity', age: 28})")

        # Query asynchronously
        p = LivePerson()
        query = Query.match(p).where(p.name == "Trinity").return_(p.alias)
        records = await session.execute(query)

        assert len(records) == 1
        assert records[0][p.alias]["name"] == "Trinity"

        # Stream into Polars asynchronously
        df = await session.execute_to_polars("MATCH (p:LivePerson {id: 99}) RETURN p.name AS name")
        assert isinstance(df, pl.DataFrame)
        assert df["name"][0] == "Trinity"
    finally:
        # Cleanup
        async with async_driver.session() as s:
            await s.run("MATCH (n:LivePerson {id: 99}) DETACH DELETE n")
        await async_driver.close()


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
def test_live_neo4j_bulk_ingestion_1000_records(clean_neo4j):
    """Test high-throughput UNWIND bulk ingestion of 1,000 real records into live Neo4j."""
    session = Session(bridge=clean_neo4j, dialect="cypher")

    # Generate 1,000 synthetic records with Polars
    df = pl.DataFrame(
        {
            "id": list(range(1000)),
            "name": [f"Person_{i}" for i in range(1000)],
            "age": [20 + (i % 50) for i in range(1000)],
        }
    )

    # Prepare bulk create plan (chunked in batches of 200)
    plan = session.bulk_create(LivePerson, df, batch_size=200)
    assert plan.num_batches == 5
    assert plan.total_records == 1000

    # Execute bulk plan against live Neo4j
    result = session.run_bulk(plan)
    assert result.total_batches == 5
    assert result.total_records == 1000

    # Verify all 1,000 nodes exist in Neo4j
    count_records = session.execute("MATCH (p:LivePerson) RETURN count(p) AS count")
    assert count_records[0]["count"] == 1000


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
def test_live_neo4j_official_social_dataset(clean_neo4j):
    """Test loading and querying the official LDBC Social Network dataset on live Neo4j."""
    import pathlib

    seed_path = pathlib.Path(__file__).parents[3] / "test_data" / "social" / "seed.cypher"
    if not seed_path.exists():
        pytest.skip(f"Seed file not found: {seed_path}")

    session = Session(bridge=clean_neo4j, dialect="cypher")

    # Read and seed the official social graph statements as a single graph transaction
    content = seed_path.read_text(encoding="utf-8")
    lines = [line for line in content.splitlines() if not line.strip().startswith("//")]
    script = " ".join(lines).replace(";", " ")
    session.execute(script)

    # Query multi-hop creators, posts, and tags
    df = session.execute_to_polars("""
        MATCH (p:Person)-[:CREATOR_OF]->(post:Post)-[:HAS_TAG]->(t:Tag)
        RETURN p.firstName AS author, post.content AS post, t.name AS tag
        ORDER BY author, tag
    """)
    assert isinstance(df, pl.DataFrame)
    assert len(df) >= 4
    authors = df["author"].to_list()
    assert "Alan" in authors
    assert "Grace" in authors
    assert "Claude" in authors
    assert "Margaret" in authors


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
def test_live_neo4j_official_movies_dataset(clean_neo4j):
    """Test loading and querying the official Movie Graph dataset on live Neo4j."""
    import pathlib

    seed_path = pathlib.Path(__file__).parents[3] / "test_data" / "movies" / "seed.cypher"
    if not seed_path.exists():
        pytest.skip(f"Seed file not found: {seed_path}")

    session = Session(bridge=clean_neo4j, dialect="cypher")

    content = seed_path.read_text(encoding="utf-8")
    lines = [line for line in content.splitlines() if not line.strip().startswith("//")]
    script = " ".join(lines).replace(";", " ")
    session.execute(script)

    # Query actors in The Matrix
    df = session.execute_to_polars("""
        MATCH (p:Person)-[:ACTED_IN]->(m:Movie {title: 'The Matrix'})
        RETURN p.name AS actor, m.released AS released
        ORDER BY actor
    """)
    assert isinstance(df, pl.DataFrame)
    assert len(df) == 5
    actors = df["actor"].to_list()
    assert "Keanu Reeves" in actors
    assert "Carrie-Anne Moss" in actors
    assert "Laurence Fishburne" in actors
