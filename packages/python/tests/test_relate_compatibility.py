"""Integration and compatibility tests for Relate (https://relatedb.ai) embedded database.

Verifies:
1. RelateBridge and AsyncRelateBridge lifecycle, detection, and URI auto-instantiation.
2. Full openCypher / Cypher 25 dialect parity with Neo4j.
3. High-level Voyager OGM Session, Node, Relationship models, and query execution.
4. Zero-copy Apache Arrow / Polars DataFrame streaming.
5. High-throughput UNWIND batch ingestion.
6. Geospatial Point parameter serialization and retrieval.
7. Schema DDL: Unique constraints and indexes.
8. Asynchronous session operations via AsyncRelateBridge.
"""

from __future__ import annotations

import polars as pl
import pytest
from voyager_ogm import (
    AsyncRelateBridge,
    AsyncSession,
    Field,
    Node,
    Point,
    Query,
    RelateBridge,
    Relationship,
    Session,
    create_bridge,
    node,
    relationship,
    reset_alias_counters,
)

try:
    import relate

    RELATE_AVAILABLE = True
except ImportError:
    relate = None  # type: ignore[assignment]
    RELATE_AVAILABLE = False

pytestmark = pytest.mark.skipif(
    not RELATE_AVAILABLE,
    reason="relate-db is not installed. Install with 'pip install relate-db' to run Relate compatibility tests.",
)


@node("RelatePerson")
class RelatePerson(Node):
    id: int = Field(primary_key=True)
    name: str
    age: int


@node("RelateCity")
class RelateCity(Node):
    id: int = Field(primary_key=True)
    name: str


@relationship("LIVES_IN")
class LivesIn(Relationship):
    since: int


@pytest.fixture(autouse=True)
def _reset_aliases():
    reset_alias_counters()


@pytest.fixture
def relate_conn():
    """Provides a fresh in-memory Relate database connection."""
    conn = relate.connect(":memory:")
    yield conn
    conn.close()


def test_relate_bridge_direct_instantiation(relate_conn):
    """Verifies direct instantiation of RelateBridge and connectivity ping."""
    bridge = RelateBridge(relate_conn)
    assert bridge.ping() is True

    res = bridge.execute("RETURN 42 AS answer")
    assert res == [{"answer": 42}]


def test_relate_bridge_auto_detection(relate_conn):
    """Verifies create_bridge() automatically detects a relate.Connection object."""
    sync_bridge = create_bridge(relate_conn)
    assert isinstance(sync_bridge, RelateBridge)
    assert sync_bridge.ping() is True

    async_bridge = create_bridge(relate_conn, is_async=True)
    assert isinstance(async_bridge, AsyncRelateBridge)


def test_relate_uri_auto_instantiation():
    """Verifies create_bridge() auto-instantiates Relate from relate:// URIs."""
    sync_bridge = create_bridge("relate://:memory:")
    assert isinstance(sync_bridge, RelateBridge)
    assert sync_bridge.ping() is True
    sync_bridge.close()

    async_bridge = create_bridge("relate://:memory:", is_async=True)
    assert isinstance(async_bridge, AsyncRelateBridge)
    async_bridge.sync_bridge.close()


def test_relate_node_crud_and_cypher_parity(relate_conn):
    """Verifies openCypher CRUD and WHERE clause filtering matching Neo4j behavior."""
    session = Session(bridge=relate_conn, dialect="cypher")

    # 1. Create nodes
    session.execute("CREATE (p:RelatePerson {id: 1, name: 'Alice', age: 30})")
    session.execute("CREATE (p:RelatePerson {id: 2, name: 'Bob', age: 22})")
    session.execute("CREATE (p:RelatePerson {id: 3, name: 'Charlie', age: 35})")

    # 2. Query via Voyager OGM Query Builder
    p = RelatePerson()
    q = Query.match(p).where(p.age >= 30).return_(p.name, p.age).order_by(p.age)
    records = session.execute(q)

    assert len(records) == 2
    assert records[0]["_relateperson_0.name"] == "Alice"
    assert records[0]["_relateperson_0.age"] == 30
    assert records[1]["_relateperson_0.name"] == "Charlie"
    assert records[1]["_relateperson_0.age"] == 35

    # 3. SQLAlchemy-style view checks
    mappings = records.mappings().all()
    assert mappings[0]["_relateperson_0.name"] == "Alice"
    scalars = records.scalars().all()
    assert scalars == ["Alice", "Charlie"]


def test_relate_relationship_traversals(relate_conn):
    """Verifies graph relationship creation and multi-hop pattern traversals."""
    session = Session(bridge=relate_conn, dialect="cypher")

    # Setup topology: Alice -[LIVES_IN]-> London
    session.execute("CREATE (p:RelatePerson {id: 1, name: 'Alice', age: 30})")
    session.execute("CREATE (c:RelateCity {id: 10, name: 'London'})")
    session.execute(
        "MATCH (p:RelatePerson {id: 1}), (c:RelateCity {id: 10}) "
        "CREATE (p)-[:LIVES_IN {since: 2018}]->(c)"
    )

    p = RelatePerson()
    c = RelateCity()
    q = Query.match(p).to(LivesIn, var="r").node(c).return_(p.name, c.name)

    records = session.execute(q)
    assert len(records) == 1
    row = records[0]
    assert row["_relateperson_0.name"] == "Alice"
    assert row["_relatecity_0.name"] == "London"


def test_relate_entity_return_and_graph_extraction(relate_conn):
    """Verifies full Node and Relationship entity extraction into res.nodes and res.edges."""
    session = Session(bridge=relate_conn, dialect="cypher")

    session.execute(
        "CREATE (a:RelatePerson {id: 1, name: 'Alice', age: 30})"
        "-[:LIVES_IN {since: 2020}]->"
        "(c:RelateCity {id: 10, name: 'Paris'})"
    )

    records = session.execute("MATCH (p:RelatePerson)-[r:LIVES_IN]->(c:RelateCity) RETURN p, r, c")
    assert len(records) == 1
    row = records[0]

    # Verify node extraction retains properties and labels
    assert row["p"]["name"] == "Alice"
    assert "RelatePerson" in row["p"]["labels"]
    assert row["c"]["name"] == "Paris"
    assert "RelateCity" in row["c"]["labels"]
    assert row["r"]["since"] == 2020
    assert row["r"]["type"] == "LIVES_IN"

    # Verify GraphViewer / NetworkX extractor extracts entities
    assert len(records.nodes) == 2
    assert len(records.edges) == 1
    node_labels = {n["label"] for n in records.nodes}
    assert "RelatePerson" in node_labels
    assert "RelateCity" in node_labels
    assert records.edges[0]["label"] == "LIVES_IN"


def test_relate_zero_copy_polars_streaming(relate_conn):
    """Verifies Arrow-backed zero-copy streaming into Polars DataFrames."""
    session = Session(bridge=relate_conn, dialect="cypher")

    session.execute("CREATE (:RelatePerson {id: 1, name: 'Alice', age: 30})")
    session.execute("CREATE (:RelatePerson {id: 2, name: 'Bob', age: 25})")

    df = session.execute_to_polars(
        "MATCH (p:RelatePerson) RETURN p.id AS id, p.name AS name, p.age AS age ORDER BY id"
    )
    assert isinstance(df, pl.DataFrame)
    assert df.shape == (2, 3)
    assert df["name"].to_list() == ["Alice", "Bob"]
    assert df["age"].to_list() == [30, 25]

    # Empty result test
    empty_df = session.execute_to_polars(
        "MATCH (p:RelatePerson {id: 999}) RETURN p.name AS name, p.age AS age"
    )
    assert isinstance(empty_df, pl.DataFrame)
    assert empty_df.shape == (0, 2)


def test_relate_bulk_ingestion_unwind(relate_conn):
    """Verifies UNWIND bulk batch ingestion matching Neo4j high-throughput patterns."""
    session = Session(bridge=relate_conn, dialect="cypher")

    records_data = [{"id": i, "name": f"User_{i}", "age": 20 + (i % 40)} for i in range(250)]
    plan = session.bulk_create(RelatePerson, records_data, batch_size=50)

    assert plan.num_batches == 5
    assert plan.total_records == 250

    result = session.run_bulk(plan)
    assert result.total_batches == 5
    assert result.total_records == 250
    assert "UNWIND $batch AS row" in result.statement

    count_records = session.execute("MATCH (p:RelatePerson) RETURN count(p) AS cnt")
    assert count_records[0]["cnt"] == 250


def test_relate_geospatial_point(relate_conn):
    """Verifies Point parameter adaptation and coordinate roundtrip."""
    session = Session(bridge=relate_conn, dialect="cypher")

    pt = Point(longitude=-122.4194, latitude=37.7749, srid=4326)
    session.execute(
        "CREATE (c:RelateCity {id: 1, name: 'San Francisco', location: $loc})",
        {"loc": pt},
    )

    records = session.execute(
        "MATCH (c:RelateCity {id: 1}) RETURN c.name AS name, c.location AS loc"
    )
    assert len(records) == 1
    loc = records[0]["loc"]
    # Point should roundtrip losslessly as a Voyager Point or spatial entity
    assert isinstance(loc, Point)
    assert round(loc.longitude, 4) == -122.4194
    assert round(loc.latitude, 4) == 37.7749
    assert loc.srid == 4326


def test_relate_schema_constraints_and_indexes(relate_conn):
    """Verifies openCypher CREATE CONSTRAINT and CREATE INDEX DDL on Relate."""
    session = Session(bridge=relate_conn, dialect="cypher")

    # 1. Unique constraint
    session.execute(
        "CREATE CONSTRAINT c_user_unique IF NOT EXISTS FOR (n:RelatePerson) REQUIRE n.id IS UNIQUE"
    )

    # 2. Index
    session.execute("CREATE INDEX idx_user_age IF NOT EXISTS FOR (n:RelatePerson) ON (n.age)")

    # 3. Test uniqueness enforcement
    session.execute("CREATE (:RelatePerson {id: 100, name: 'UniqueUser', age: 25})")
    with pytest.raises((relate.exceptions.IntegrityError, RuntimeError)):
        session.execute("CREATE (:RelatePerson {id: 100, name: 'DuplicateUser', age: 26})")


@pytest.mark.asyncio
async def test_relate_async_session_crud(relate_conn):
    """Verifies non-blocking AsyncSession execution over AsyncRelateBridge."""
    session = AsyncSession(bridge=relate_conn, dialect="cypher")

    await session.execute("CREATE (:RelatePerson {id: 10, name: 'AsyncAlice', age: 29})")

    p = RelatePerson()
    q = Query.match(p).where(p.id == 10).return_(p.name, p.age)
    records = await session.execute(q)

    assert len(records) == 1
    assert records[0]["_relateperson_0.name"] == "AsyncAlice"
    assert records[0]["_relateperson_0.age"] == 29

    df = await session.execute_to_polars("MATCH (p:RelatePerson {id: 10}) RETURN p.name AS name")
    assert isinstance(df, pl.DataFrame)
    assert df["name"][0] == "AsyncAlice"


def test_relate_and_neo4j_query_parity():
    """Verifies that the compiled Cypher statement executed against Relate matches Neo4j standards."""
    p = RelatePerson()
    c = RelateCity()
    q = (
        Query.match(p)
        .to(LivesIn, var="r")
        .node(c)
        .where(p.age > 21, c.name == "London")
        .return_(p.name, c.name)
    )

    compiled = q.compile("cypher")
    assert (
        compiled.statement
        == "MATCH (_relateperson_0:RelatePerson)-[r:LIVES_IN]->(_relatecity_0:RelateCity) WHERE (_relateperson_0.age > $p0) AND (_relatecity_0.name = $p1) RETURN _relateperson_0.name, _relatecity_0.name"
    )
    assert compiled.parameters == {"p0": 21, "p1": "London"}


def test_relate_neo4j_kernel_metadata_parity(relate_conn):
    """Verifies dbms.components() reports Neo4j Kernel compatibility."""
    session = Session(bridge=relate_conn, dialect="cypher")
    comps = session.execute("CALL dbms.components()")
    assert any(c.get("name") == "Neo4j Kernel" for c in comps)
    assert any("25" in c.get("versions", []) for c in comps)


def test_relate_neo4j_fulltext_and_vector_procedures(relate_conn):
    """Verifies Neo4j fulltext and vector index procedures work natively in Relate."""
    session = Session(bridge=relate_conn, dialect="cypher")

    # 1. Neo4j Fulltext index procedure
    session.execute("CREATE FULLTEXT INDEX article_ft FOR (n:Article) ON EACH [n.title, n.body]")
    session.execute(
        "CREATE (:Article {title: 'Voyager OGM Overview', body: 'Graph mapper with Polars streaming'})"
    )
    ft_res = session.execute(
        "CALL db.index.fulltext.queryNodes('article_ft', 'Voyager') YIELD node, score RETURN node.title AS title, score"
    )
    assert len(ft_res) == 1
    assert ft_res[0]["title"] == "Voyager OGM Overview"
    assert ft_res[0]["score"] > 0

    # 2. Neo4j Vector index procedure
    session.execute(
        "CREATE VECTOR INDEX article_vec FOR (n:Article) ON (n.embedding) "
        "OPTIONS {indexConfig: {'vector.dimensions': 4, 'vector.similarity_function': 'cosine'}}"
    )
    session.execute("CREATE (:Article {title: 'Doc A', embedding: [0.1, 0.2, 0.3, 0.4]})")
    session.execute("CREATE (:Article {title: 'Doc B', embedding: [0.9, 0.8, 0.7, 0.6]})")
    vec_res = session.execute(
        "CALL db.index.vector.queryNodes('article_vec', 2, [0.1, 0.2, 0.3, 0.4]) YIELD node, score RETURN node.title AS title, score"
    )
    assert len(vec_res) == 2
    assert vec_res[0]["title"] == "Doc A"
    assert round(vec_res[0]["score"], 2) == 1.0
