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

pytestmark = pytest.mark.live

try:
    from neo4j import AsyncGraphDatabase, GraphDatabase

    NEO4J_AVAILABLE = True
except ImportError:
    NEO4J_AVAILABLE = False

try:
    import duckdb

    DUCKDB_AVAILABLE = True
except ImportError:
    duckdb = None
    DUCKDB_AVAILABLE = False


NEO4J_URI = "bolt://127.0.0.1:7687"
NEO4J_AUTH = ("neo4j", "voyagerpass123")


def _is_neo4j_online() -> bool:
    """Checks whether a live Neo4j database instance is running and accepting connections."""
    if not NEO4J_AVAILABLE:
        return False
    try:
        from voyager_ogm import NativeClient

        c = NativeClient(
            f"bolt://{NEO4J_AUTH[0]}:{NEO4J_AUTH[1]}@127.0.0.1:7687?connect_timeout=2",
            min_idle=1,
            max_size=2,
        )
        online = c.ping_sync()
        c.close()
        return online
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
            s.run("MATCH (n) DETACH DELETE n").consume()
        yield driver
        with driver.session() as s:
            s.run("MATCH (n) DETACH DELETE n").consume()
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
async def test_live_neo4j_async_crud(clean_neo4j):
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


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
def test_live_neo4j_fluent_builder_with_clause_and_pagination(clean_neo4j):
    """Test fluent query builder with_() pipeline, hybrid chaining, and pagination on live Neo4j."""
    session = Session(bridge=clean_neo4j, dialect="cypher")

    # 1. Test hybrid method chaining UNWIND -> CREATE
    items = [
        {"name": "Alice", "age": 30},
        {"name": "Bob", "age": 20},
        {"name": "Charlie", "age": 40},
        {"name": "David", "age": 25},
    ]
    unwind_q = Query.unwind("rows", "row").create(
        Node("LivePerson", name="row.name", age="row.age")
    )
    assert unwind_q is not None
    # Direct cypher execution for seed
    session.execute(
        "UNWIND $rows AS row CREATE (:LivePerson {name: row.name, age: row.age})",
        {"rows": items},
    )

    # 2. Test WITH clause pipeline: MATCH -> WHERE -> WITH -> WHERE -> RETURN
    p = LivePerson()
    query = (
        Query.match(p)
        .where(p.age >= 20)
        .with_(p)
        .where(p.age > 25)
        .return_(p.alias)
        .order_by(p.age)
    )
    records = session.execute(query)
    names = [r[p.alias]["name"] for r in records]
    assert names == ["Alice", "Charlie"]

    # 3. Test pagination: offset and limit
    p2 = LivePerson()
    page_q = Query.match(p2).return_(p2.alias).order_by(p2.age).offset(1).limit(2)
    page_records = session.execute(page_q)
    page_names = [r[p2.alias]["name"] for r in page_records]
    assert page_names == ["David", "Alice"]


def test_live_duckdb_session_ping_dialect_awareness():
    """Test Session.ping on live DuckDB runs SELECT 1 without syntax errors, while raw RETURN 1 fails."""
    if not DUCKDB_AVAILABLE:
        pytest.skip("DuckDB is not installed")
    con = duckdb.connect()
    # 1. Raw RETURN 1 must fail due to Cypher syntax in DuckDB
    with pytest.raises(duckdb.Error):
        con.execute("RETURN 1")

    # 2. Session.ping() must succeed using SELECT 1
    session = Session(bridge=con, dialect="sql_pgq")
    assert session.ping() is True


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
def test_live_neo4j_session_ping_and_parameters_merging(clean_neo4j):
    """Test Session.ping and runtime parameter merging on live Neo4j."""
    session = Session(bridge=clean_neo4j, dialect="cypher")

    # 1. Session.ping() liveness check
    assert session.ping() is True

    # 2. Seed data
    session.execute("CREATE (:LivePerson {id: 1, name: 'Alice', age: 30})")
    session.execute("CREATE (:LivePerson {id: 2, name: 'Bob', age: 20})")

    # 3. Query with runtime parameters merged
    p = LivePerson()
    query = Query.match(p).where(p.age > 25).return_(p.name)
    res = query.execute(session, parameters={"extra_param": "test"})
    names = res.scalars().all()
    assert names == ["Alice"]


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
@pytest.mark.asyncio
async def test_live_neo4j_async_session_ping():
    """Test AsyncSession.ping liveness check on live Neo4j."""
    async_driver = AsyncGraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH)
    try:
        session = AsyncSession(bridge=async_driver, dialect="cypher")
        assert await session.ping() is True
    finally:
        await async_driver.close()


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
def test_live_neo4j_bulk_create_relationships_string_descriptor(clean_neo4j):
    """Test bulk relationship creation with string type descriptor and inferred properties on live Neo4j."""
    session = Session(bridge=clean_neo4j, dialect="cypher")

    # 1. Seed endpoint nodes
    session.execute("CREATE (:LivePerson {id: 1, name: 'Alice', age: 30})")
    session.execute("CREATE (:LivePerson {id: 2, name: 'Bob', age: 25})")

    # 2. Ingest relationship using string type "COLLABORATES_WITH" with dynamic properties
    edges = [
        {"from_id": 1, "to_id": 2, "since": 2023, "role": "lead"},
    ]
    plan = session.bulk_create_relationships(
        "COLLABORATES_WITH",
        edges,
        from_label="LivePerson",
        from_key="id",
        to_label="LivePerson",
        to_key="id",
    )
    result = session.run_bulk(plan)
    assert result.total_records == 1

    # 3. Read back relationship from live Neo4j and assert properties were physically persisted
    res = session.execute(
        "MATCH (a:LivePerson {id: 1})-[r:COLLABORATES_WITH]->(b:LivePerson {id: 2}) "
        "RETURN r.since AS since, r.role AS role"
    )
    rows = res.mappings().all()
    assert len(rows) == 1
    assert rows[0]["since"] == 2023
    assert rows[0]["role"] == "lead"


@node("LiveSimultaneousPerson")
class LiveSimultaneousPerson(Node):
    id: int = Field(primary_key=True)
    name: str
    age: int


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
@pytest.mark.asyncio
async def test_live_neo4j_async_simultaneous_explain_and_profile():
    """Test 2 simultaneous connections executing explain and profile queries concurrently on live Neo4j."""
    import asyncio

    async_driver = AsyncGraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH)
    try:
        session1 = AsyncSession(bridge=async_driver, dialect="cypher")
        session2 = AsyncSession(bridge=async_driver, dialect="cypher")

        # 1. Seed data
        await session1.execute("MATCH (n:LiveSimultaneousPerson) DETACH DELETE n")
        await session1.execute(
            "CREATE (:LiveSimultaneousPerson {id: 1, name: 'Alice', age: 30}), "
            "(:LiveSimultaneousPerson {id: 2, name: 'Bob', age: 25})"
        )

        p = LiveSimultaneousPerson()

        # Build explain query (Query.explain())
        q_explain = Query.match(p).where(p.age >= 20).return_(p.name).order_by(p.age).explain()
        assert q_explain.execution_mode == "explain"

        # Build profile query (Query.profile())
        q_profile = Query.match(p).where(p.age >= 20).return_(p.name).order_by(p.age).profile()
        assert q_profile.execution_mode == "profile"

        # 2. Run both queries simultaneously across two separate sessions / connections
        res_explain, res_profile = await asyncio.gather(
            session1.execute(q_explain),
            session2.execute(q_profile),
        )

        assert res_explain is not None
        assert res_profile is not None

        # In Neo4j Bolt, EXPLAIN plans without returning data records
        assert len(res_explain.all()) == 0

        # In Neo4j Bolt, PROFILE plans and returns data records
        names = res_profile.scalars().all()
        assert len(names) == 2
        assert names == ["Bob", "Alice"]

        # 3. Clean up
        await session1.execute("MATCH (n:LiveSimultaneousPerson) DETACH DELETE n")
    finally:
        await async_driver.close()


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j instance not online on localhost:7687")
def test_live_neo4j_sync_simultaneous_explain_and_profile():
    """Test 2 simultaneous sync connections executing explain and profile queries concurrently on live Neo4j."""
    import concurrent.futures

    driver = GraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH)
    try:
        session_setup = Session(bridge=driver, dialect="cypher")
        session_setup.execute("MATCH (n:LiveSimultaneousPerson) DETACH DELETE n")
        session_setup.execute(
            "CREATE (:LiveSimultaneousPerson {id: 1, name: 'Alice', age: 30}), "
            "(:LiveSimultaneousPerson {id: 2, name: 'Bob', age: 25})"
        )

        p = LiveSimultaneousPerson()
        q_explain = Query.match(p).where(p.age >= 20).return_(p.name).order_by(p.age).explain()
        q_profile = Query.match(p).where(p.age >= 20).return_(p.name).order_by(p.age).profile()

        def run_query(query):
            s = Session(bridge=driver, dialect="cypher")
            return s.execute(query)

        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            fut_explain = executor.submit(run_query, q_explain)
            fut_profile = executor.submit(run_query, q_profile)
            res_explain = fut_explain.result()
            res_profile = fut_profile.result()

        assert len(res_explain.all()) == 0
        names = res_profile.scalars().all()
        assert len(names) == 2
        assert names == ["Bob", "Alice"]

        session_setup.execute("MATCH (n:LiveSimultaneousPerson) DETACH DELETE n")
    finally:
        driver.close()
