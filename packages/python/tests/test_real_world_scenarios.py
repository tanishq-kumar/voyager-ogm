"""Real-world database scenario tests across openCypher, SQL:2023 PGQ, and ISO GQL.

Tests comprehensive query patterns on real engines:
1. LDBC Social Network Graph (2-hop traversals, Friend-of-a-Friend, aggregations)
2. Financial Transaction & Fraud Pattern Graph (cycle detection, range filters)
3. SQL:2023 PGQ schema & execution on local DuckDB
4. Multi-dialect parity across Cypher, SQL:2023 PGQ, and ISO GQL
"""

from __future__ import annotations

try:
    import duckdb
except ImportError:
    duckdb = None

import polars as pl
import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
    node,
    relationship,
    reset_alias_counters,
)

# Check if live Neo4j is reachable
try:
    from neo4j import GraphDatabase

    driver = GraphDatabase.driver("bolt://localhost:7687", auth=("neo4j", "voyagerpass123"))
    driver.verify_connectivity()
    driver.close()
    NEO4J_ONLINE = True
except Exception:
    NEO4J_ONLINE = False


@node("Person")
class Person(Node):
    id: int = Field(primary_key=True)
    name: str
    age: int
    city: str


@relationship("KNOWS")
class Knows(Relationship):
    since: int
    weight: float


@node("Account")
class Account(Node):
    id: int = Field(primary_key=True)
    owner: str
    balance: float


@relationship("TRANSFERRED")
class Transferred(Relationship):
    amount: float
    timestamp: int


@pytest.fixture(autouse=True)
def _reset():
    reset_alias_counters()


# =========================================================================
# 1. Local DuckDB: Relational Property Graph & SQL/PGQ Ingestion Scenarios
# =========================================================================


def test_duckdb_ldbc_social_network_scenario():
    """Test LDBC Social Network graph querying and aggregation on local DuckDB."""
    if duckdb is None:
        pytest.skip("duckdb is not installed in this environment")
    con = duckdb.connect(":memory:")

    # Setup relational schema
    con.execute("""
        CREATE TABLE person (id INTEGER PRIMARY KEY, name VARCHAR, age INTEGER, city VARCHAR);
        CREATE TABLE knows (from_id INTEGER, to_id INTEGER, since INTEGER, weight DOUBLE);

        INSERT INTO person VALUES
            (1, 'Alice', 30, 'London'),
            (2, 'Bob', 28, 'London'),
            (3, 'Charlie', 35, 'Paris'),
            (4, 'Diana', 22, 'Paris'),
            (5, 'Evan', 40, 'New York');

        INSERT INTO knows VALUES
            (1, 2, 2020, 0.9),
            (2, 3, 2021, 0.8),
            (3, 4, 2019, 0.95),
            (1, 5, 2022, 0.5);
    """)

    session = Session(bridge=con, dialect="sql_pgq")

    # Query 1: Direct Join traversal representing Person -> KNOWS -> Friend
    query_2hop = """
        SELECT
            p1.name AS person,
            p2.name AS friend,
            k.since,
            k.weight
        FROM person p1
        JOIN knows k ON p1.id = k.from_id
        JOIN person p2 ON k.to_id = p2.id
        WHERE p1.city = 'London'
        ORDER BY k.weight DESC;
    """
    records = session.execute(query_2hop)
    assert len(records) == 3
    assert records[0]["person"] == "Alice"
    assert records[0]["friend"] == "Bob"

    # Query 2: Zero-copy Polars aggregation: Average friend age by city
    agg_query = """
        SELECT
            p1.city,
            COUNT(p2.id) AS total_friends,
            AVG(p2.age) AS avg_friend_age
        FROM person p1
        JOIN knows k ON p1.id = k.from_id
        JOIN person p2 ON k.to_id = p2.id
        GROUP BY p1.city
        ORDER BY p1.city;
    """
    df = session.execute_to_polars(agg_query)
    assert isinstance(df, pl.DataFrame)
    assert len(df) == 2  # London and Paris have outgoing friendships
    assert "avg_friend_age" in df.columns


def test_duckdb_financial_fraud_pattern_scenario():
    """Test cyclic transaction fraud pattern matching on local DuckDB."""
    if duckdb is None:
        pytest.skip("duckdb is not installed in this environment")
    con = duckdb.connect(":memory:")

    con.execute("""
        CREATE TABLE accounts (id INTEGER PRIMARY KEY, owner VARCHAR, balance DOUBLE);
        CREATE TABLE transfers (from_id INTEGER, to_id INTEGER, amount DOUBLE, timestamp INTEGER);

        INSERT INTO accounts VALUES
            (101, 'Victim Corp', 50000.0),
            (102, 'Mule A', 100.0),
            (103, 'Mule B', 200.0),
            (104, 'Safe Account', 10000.0);

        -- Circular transfer fraud ring: 101 -> 102 -> 103 -> 101
        INSERT INTO transfers VALUES
            (101, 102, 9500.0, 1001),
            (102, 103, 9400.0, 1002),
            (103, 101, 9300.0, 1003),
            (101, 104, 500.0, 1004);
    """)

    session = Session(bridge=con, dialect="sql_pgq")

    # 3-hop Cycle Detection Query: A -> B -> C -> A
    cycle_query = """
        SELECT
            a1.owner AS source,
            a2.owner AS intermediary_1,
            a3.owner AS intermediary_2,
            t1.amount AS initial_amount,
            t3.amount AS return_amount
        FROM accounts a1
        JOIN transfers t1 ON a1.id = t1.from_id
        JOIN accounts a2 ON t1.to_id = a2.id
        JOIN transfers t2 ON a2.id = t2.from_id
        JOIN accounts a3 ON t2.to_id = a3.id
        JOIN transfers t3 ON a3.id = t3.from_id AND t3.to_id = a1.id
        WHERE t1.amount > 5000.0;
    """

    fraud_df = session.execute_to_polars(cycle_query)
    assert isinstance(fraud_df, pl.DataFrame)
    assert len(fraud_df) == 3
    assert any(fraud_df["source"] == "Victim Corp")

    # Filtered to Victim Corp source specifically
    victim_ring = fraud_df.filter(pl.col("source") == "Victim Corp")
    assert len(victim_ring) == 1
    assert victim_ring["intermediary_1"][0] == "Mule A"
    assert victim_ring["intermediary_2"][0] == "Mule B"


# =========================================================================
# 2. Multi-Dialect AST Parity: openCypher vs SQL:2023 PGQ vs ISO GQL
# =========================================================================


def test_multi_dialect_query_compilation_parity():
    """Verify AST compilation produces syntactically valid queries across all 3 dialects."""
    p = Person()

    # Query: MATCH (p:Person) WHERE p.age >= 21 AND p.city == 'London' RETURN p.name, p.age
    q = Query.match(p).where(p.age >= 21, p.city == "London").return_(p.name, p.age)

    # 1. openCypher Dialect
    cypher_compiled = q.compile(dialect="cypher")
    assert cypher_compiled.statement == (
        "MATCH (_person_0:Person) "
        "WHERE (_person_0.age >= $p0) AND (_person_0.city = $p1) "
        "RETURN _person_0.name, _person_0.age"
    )
    assert cypher_compiled.parameters == {"p0": 21, "p1": "London"}

    # 2. ISO GQL Dialect (ISO/IEC 39075:2024)
    gql_compiled = q.compile(dialect="iso_gql")
    assert gql_compiled.statement == (
        "MATCH (_person_0:Person) "
        "WHERE (_person_0.age >= $p0) AND (_person_0.city = $p1) "
        "RETURN _person_0.name, _person_0.age"
    )

    # 3. SQL:2023 PGQ Dialect
    pgq_compiled = q.compile(dialect="sql_pgq")
    assert "GRAPH_TABLE" in pgq_compiled.statement
    assert "COLUMNS" in pgq_compiled.statement
    assert "_person_0.age >= $p0" in pgq_compiled.statement


# =========================================================================
# 3. Real Neo4j Live Scenario: LDBC Variable-Length & Aggregations
# =========================================================================


@pytest.mark.skipif(not NEO4J_ONLINE, reason="Live Neo4j not online on localhost:7687")
def test_live_neo4j_ldbc_multihop_and_aggregations():
    """Test multi-hop relationship traversal and aggregations against real running Neo4j."""
    driver = GraphDatabase.driver("bolt://localhost:7687", auth=("neo4j", "voyagerpass123"))

    try:
        session = Session(bridge=driver, dialect="cypher")

        # 1. Seed LDBC graph data
        session.execute("""
            CREATE (a:Person {id: 1, name: 'Alice', age: 30, city: 'London'})
            CREATE (b:Person {id: 2, name: 'Bob', age: 28, city: 'London'})
            CREATE (c:Person {id: 3, name: 'Charlie', age: 35, city: 'Paris'})
            CREATE (d:Person {id: 4, name: 'Diana', age: 22, city: 'Paris'})
            CREATE (a)-[:KNOWS {since: 2020, weight: 0.9}]->(b)
            CREATE (b)-[:KNOWS {since: 2021, weight: 0.8}]->(c)
            CREATE (c)-[:KNOWS {since: 2019, weight: 0.95}]->(d)
        """)

        # 2. Friend-of-Friend (2-hop) query in real Neo4j
        fof_query = """
            MATCH (a:Person {name: 'Alice'})-[:KNOWS]->(b:Person)-[:KNOWS]->(c:Person)
            RETURN a.name AS starter, b.name AS friend, c.name AS friend_of_friend
        """
        fof_records = session.execute(fof_query)
        assert len(fof_records) == 1
        assert fof_records[0]["starter"] == "Alice"
        assert fof_records[0]["friend"] == "Bob"
        assert fof_records[0]["friend_of_friend"] == "Charlie"

        # 3. Aggregation Query into Polars: Count friends grouped by city
        agg_df = session.execute_to_polars("""
            MATCH (p:Person)
            RETURN p.city AS city, count(p) AS count, avg(p.age) AS avg_age
            ORDER BY count DESC
        """)
        assert isinstance(agg_df, pl.DataFrame)
        assert len(agg_df) == 2
        assert agg_df["city"].to_list() == ["London", "Paris"]
        assert agg_df["count"].to_list() == [2, 2]

    finally:
        # Clean up database
        with driver.session() as s:
            s.run("MATCH (n:Person) DETACH DELETE n")
        driver.close()
