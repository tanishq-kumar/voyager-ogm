"""Task 3.4: Local Multi-Engine Live Integration & Conformance Matrix.

Verifies end-to-end query compilation, transaction management, bulk ingestion,
schema constraints, and zero-copy Polars extraction across 4 live engines:
1. Neo4j 5.26 (Bolt Protocol on port 7687)
2. Memgraph (Bolt Protocol on port 7688)
3. Apache AGE (PostgreSQL + Cypher-in-SQL on port 5455)
4. DuckDB (Embedded In-Memory Relational Engine)
"""

from __future__ import annotations

import json

import duckdb
import polars as pl
import psycopg
import pytest
from neo4j import GraphDatabase
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    SchemaManager,
    Session,
    node,
    relationship,
)

# ---------------------------------------------------------------------------
# Test Graph Models
# ---------------------------------------------------------------------------


@node(label="MatrixUser")
class MatrixUser(Node):
    user_id: str = Field(primary_key=True)
    username: str = Field(unique=True)
    age: int = Field(index=True)
    city: str = Field()
    active: bool = Field()


@relationship(type_name="MATRIX_FOLLOWS")
class MatrixFollows(Relationship):
    since: int = Field()


# ---------------------------------------------------------------------------
# 1. Live Matrix Engine: Neo4j 5.26 (Bolt port 7687)
# ---------------------------------------------------------------------------


class TestNeo4jLiveMatrix:
    @pytest.fixture
    def neo4j_driver(self):
        uri = "bolt://localhost:7687"
        auth = ("neo4j", "voyagerpass123")
        try:
            driver = GraphDatabase.driver(uri, auth=auth)
            driver.verify_connectivity()
        except Exception as e:
            pytest.skip(f"Neo4j container not available on port 7687: {e}")
        yield driver
        driver.close()

    def test_neo4j_live_end_to_end_lifecycle(self, neo4j_driver):
        session = Session(bridge=neo4j_driver, dialect="cypher")

        # 1. Schema DDL: Create Constraints & Indexes
        SchemaManager.create_all(session, MatrixUser)

        # 2. Cleanup existing matrix nodes
        with neo4j_driver.session() as s:
            s.run("MATCH (n:MatrixUser) DETACH DELETE n")

        # 3. Bulk Ingestion via UNWIND $batch plan execution
        batch_data = [
            {
                "user_id": f"u_{i}",
                "username": f"user_{i}",
                "age": 20 + i,
                "city": "London" if i % 2 == 0 else "Paris",
                "active": True,
            }
            for i in range(100)
        ]
        plan = session.bulk_create(MatrixUser, batch_data, batch_size=50)
        bulk_res = session.run_bulk(plan)
        assert bulk_res.total_records == 100
        assert bulk_res.total_batches == 2

        # 4. Fluent Query Traversal with Predicates & Projections
        u = MatrixUser(alias="u")
        q = (
            Query.match(u)
            .where(u.age >= 25, u.city == "London")
            .order_by(u.age, ascending=True)
            .return_(user_id=u.user_id, username=u.username, age=u.age, city=u.city)
        )

        df = session.execute_to_polars(q)
        assert isinstance(df, pl.DataFrame)
        assert df.height > 0
        assert all(city == "London" for city in df["city"].to_list())
        assert all(age >= 25 for age in df["age"].to_list())

        # 5. Mutation: SET property
        set_q = Query.match(u).where(u.username == "user_0").set(u.city == "Berlin")
        session.execute(set_q)

        verify_q = Query.match(u).where(u.username == "user_0").return_(city=u.city)
        verify_df = session.execute_to_polars(verify_q)
        assert verify_df["city"][0] == "Berlin"

        # 6. Schema DDL: Drop Constraints & Cleanup
        SchemaManager.drop_all(session, MatrixUser)
        with neo4j_driver.session() as s:
            s.run("MATCH (n:MatrixUser) DETACH DELETE n")


# ---------------------------------------------------------------------------
# 2. Live Matrix Engine: Memgraph (Bolt port 7688)
# ---------------------------------------------------------------------------


class TestMemgraphLiveMatrix:
    @pytest.fixture
    def memgraph_driver(self):
        uri = "bolt://localhost:7688"
        auth = ("", "")
        try:
            driver = GraphDatabase.driver(uri, auth=auth)
            driver.verify_connectivity()
        except Exception as e:
            pytest.skip(f"Memgraph container not available on port 7688: {e}")
        yield driver
        driver.close()

    def test_memgraph_live_traversal_and_polars(self, memgraph_driver):
        session = Session(bridge=memgraph_driver, dialect="cypher")

        # 1. Cleanup
        with memgraph_driver.session() as s:
            s.run("MATCH (n:MatrixUser) DETACH DELETE n")

        # 2. Create sample graph: Alice -> Bob -> Charlie
        with memgraph_driver.session() as s:
            s.run(
                """
                CREATE (a:MatrixUser {user_id: 'u_1', username: 'Alice', age: 30, city: 'London', active: true})
                CREATE (b:MatrixUser {user_id: 'u_2', username: 'Bob', age: 25, city: 'London', active: true})
                CREATE (c:MatrixUser {user_id: 'u_3', username: 'Charlie', age: 35, city: 'Paris', active: true})
                CREATE (a)-[:MATRIX_FOLLOWS {since: 2021}]->(b)
                CREATE (b)-[:MATRIX_FOLLOWS {since: 2023}]->(c)
                """
            )

        # 3. Multi-hop Traversal Query (1..2 hops)
        a = MatrixUser(alias="a")
        f = MatrixFollows(alias="r")
        b = MatrixUser(alias="b")

        q = (
            Query.match(a)
            .to(f)
            .hops(1, 2)
            .node(b)
            .where(a.username == "Alice")
            .return_(source=a.username, target=b.username)
        )

        df = session.execute_to_polars(q)
        assert isinstance(df, pl.DataFrame)
        assert df.height == 2
        targets = df["target"].to_list()
        assert "Bob" in targets
        assert "Charlie" in targets

        # 4. Cleanup
        with memgraph_driver.session() as s:
            s.run("MATCH (n:MatrixUser) DETACH DELETE n")


# ---------------------------------------------------------------------------
# 3. Live Matrix Engine: Apache AGE (PostgreSQL port 5455)
# ---------------------------------------------------------------------------


class TestApacheAgeLiveMatrix:
    @pytest.fixture
    def age_conn(self):
        conn_str = (
            "host=localhost port=5455 user=postgres password=voyagerpass123 dbname=voyager_graph"
        )
        try:
            conn = psycopg.connect(conn_str, autocommit=True)
        except Exception as e:
            pytest.skip(f"Apache AGE container not available on port 5455: {e}")
        yield conn
        conn.close()

    def test_apache_age_live_end_to_end(self, age_conn):
        with age_conn.cursor() as cur:
            cur.execute("CREATE EXTENSION IF NOT EXISTS age;")
            cur.execute("LOAD 'age';")
            cur.execute('SET search_path = ag_catalog, "$user", public;')
            cur.execute(
                """
                DO $$
                BEGIN
                    IF NOT EXISTS (SELECT 1 FROM ag_catalog.ag_graph WHERE name = 'live_matrix_graph') THEN
                        PERFORM ag_catalog.create_graph('live_matrix_graph');
                    END IF;
                END
                $$;
                """
            )

            # Cleanup existing nodes in matrix graph
            cur.execute(
                "SELECT * FROM ag_catalog.cypher('live_matrix_graph', $$ MATCH (n) DETACH DELETE n $$) AS (res agtype);"
            )

            # Insert sample node into AGE graph
            cur.execute(
                """
                SELECT * FROM ag_catalog.cypher('live_matrix_graph', $$
                    CREATE (u:MatrixUser {user_id: 'u_age_1', username: 'Diana', age: 28, city: 'London', active: true})
                    RETURN u
                $$) AS (u ag_catalog.agtype);
                """
            )

            # Read using AgeEmitter compilation
            u = MatrixUser(alias="u")
            q = Query.match(u).where(u.username == "Diana").return_(u.username, u.age, u.city)
            compiled = q.compile("apache_age", graph_name="live_matrix_graph")

            params = (json.dumps(compiled.parameters),) if compiled.parameters else ()
            cur.execute(compiled.statement, params)
            rows = cur.fetchall()
            assert len(rows) == 1
            assert "Diana" in str(rows[0][0])
            assert "28" in str(rows[0][1])

            # Teardown graph
            cur.execute("SELECT ag_catalog.drop_graph('live_matrix_graph', true);")


# ---------------------------------------------------------------------------
# 4. Live Matrix Engine: DuckDB (In-Memory)
# ---------------------------------------------------------------------------


class TestDuckDbLiveMatrix:
    @pytest.fixture
    def duck_conn(self):
        conn = duckdb.connect(":memory:")
        yield conn
        conn.close()

    def test_duckdb_relational_polars_bridge(self, duck_conn):
        session = Session(bridge=duck_conn, dialect="sql_pgq")

        # 1. Create relational tables
        duck_conn.execute(
            """
            CREATE TABLE users (user_id VARCHAR PRIMARY KEY, username VARCHAR, age INT, city VARCHAR, active BOOLEAN);
            INSERT INTO users VALUES
                ('u1', 'Alice', 30, 'London', true),
                ('u2', 'Bob', 25, 'London', true),
                ('u3', 'Charlie', 35, 'Paris', false);
            """
        )

        # 2. Execute SQL query with zero-copy Polars extraction via Voyager session
        res = session.execute_to_polars("SELECT * FROM users WHERE age >= 25 AND city = 'London'")
        assert isinstance(res, pl.DataFrame)
        assert res.height == 2
        assert set(res["username"].to_list()) == {"Alice", "Bob"}

    def test_duckdb_live_duckpgq_graph_table_execution(self):
        """Verifies live execution of SQL:2023 GRAPH_TABLE compiled query on DuckDB with DuckPGQ extension."""
        conn = duckdb.connect(config={"allow_unsigned_extensions": "true"})
        try:
            conn.execute(
                "SET custom_extension_repository = 'http://duckpgq.s3.eu-north-1.amazonaws.com';"
            )
            conn.execute("FORCE INSTALL 'duckpgq';")
            conn.execute("LOAD 'duckpgq';")
        except Exception as e:
            pytest.skip(f"DuckPGQ extension could not be loaded: {e}")

        # 1. Create schema and Property Graph catalog
        conn.execute(
            """
            CREATE TABLE users (user_id VARCHAR PRIMARY KEY, username VARCHAR, age INT, city VARCHAR, active BOOLEAN);
            CREATE TABLE follows (from_id VARCHAR, to_id VARCHAR, since INT);

            INSERT INTO users VALUES
                ('u1', 'Alice', 30, 'London', true),
                ('u2', 'Bob', 25, 'London', true),
                ('u3', 'Charlie', 35, 'Paris', false);

            INSERT INTO follows VALUES
                ('u1', 'u2', 2020),
                ('u2', 'u3', 2022);

            CREATE PROPERTY GRAPH duck_matrix_graph
              VERTEX TABLES (users LABEL MatrixUser)
              EDGE TABLES (
                follows SOURCE KEY (from_id) REFERENCES users (user_id)
                        DESTINATION KEY (to_id) REFERENCES users (user_id)
                LABEL MATRIX_FOLLOWS
              );
            """
        )

        # 2. Compile Voyager Query targeting SQL:2023 PGQ dialect
        a = MatrixUser(alias="a")
        f = MatrixFollows(alias="r")
        b = MatrixUser(alias="b")

        q = (
            Query.match(a)
            .to(f)
            .node(b)
            .where(a.city == "London")
            .return_(source=a.username, target=b.username)
        )
        compiled = q.compile("sql_pgq", graph_name="duck_matrix_graph")

        # Format parameter values into GRAPH_TABLE statement for DuckPGQ
        stmt = compiled.statement
        for k_param, v_param in compiled.parameters.items():
            val_str = f"'{v_param}'" if isinstance(v_param, str) else str(v_param)
            stmt = stmt.replace(f"${k_param}", val_str)

        # 3. Execute live GRAPH_TABLE query and stream into Polars DataFrame
        df = conn.execute(stmt).pl()
        assert isinstance(df, pl.DataFrame)
        assert df.height == 2
        assert df["source"].to_list() == ["Alice", "Bob"]
        assert df["target"].to_list() == ["Bob", "Charlie"]
        conn.close()


# ---------------------------------------------------------------------------
# 5. Live Matrix Engine: PostgreSQL 19 Beta 3 (Port 5456)
# ---------------------------------------------------------------------------


class TestPostgres19LiveMatrix:
    @pytest.fixture
    def pg19_conn(self):
        conn_str = (
            "host=localhost port=5456 user=postgres password=voyagerpass123 dbname=voyager_graph"
        )
        try:
            conn = psycopg.connect(conn_str, autocommit=True)
        except Exception as e:
            pytest.skip(f"PostgreSQL 19 Beta container not available on port 5456: {e}")
        yield conn
        conn.close()

    def test_postgres19_live_recursive_graph_and_polars(self, pg19_conn):
        with pg19_conn.cursor() as cur:
            cur.execute("SELECT version();")
            version_str = cur.fetchone()[0]
            assert "PostgreSQL 19" in version_str

            # 1. Setup Relational Graph Schema
            cur.execute(
                """
                DROP TABLE IF EXISTS follows CASCADE;
                DROP TABLE IF EXISTS users CASCADE;

                CREATE TABLE users (
                    id INT PRIMARY KEY,
                    username TEXT UNIQUE NOT NULL,
                    age INT NOT NULL,
                    city TEXT NOT NULL
                );

                CREATE TABLE follows (
                    from_id INT REFERENCES users(id),
                    to_id INT REFERENCES users(id),
                    since INT NOT NULL,
                    PRIMARY KEY (from_id, to_id)
                );

                INSERT INTO users VALUES
                    (1, 'Alice', 30, 'London'),
                    (2, 'Bob', 25, 'London'),
                    (3, 'Charlie', 35, 'Paris'),
                    (4, 'Diana', 28, 'London');

                INSERT INTO follows VALUES
                    (1, 2, 2020),
                    (2, 3, 2021),
                    (3, 4, 2023);
                """
            )

            # 2. Multi-hop Recursive Graph Traversal Query
            cur.execute(
                """
                WITH RECURSIVE graph_path AS (
                    SELECT f.from_id, f.to_id, 1 AS depth, ARRAY[f.from_id, f.to_id] AS path
                    FROM follows f
                    WHERE f.from_id = 1
                    UNION ALL
                    SELECT gp.from_id, f.to_id, gp.depth + 1, gp.path || f.to_id
                    FROM graph_path gp
                    JOIN follows f ON gp.to_id = f.from_id
                    WHERE gp.depth < 3 AND NOT (f.to_id = ANY(gp.path))
                )
                SELECT
                    u1.username AS source_user,
                    u2.username AS target_user,
                    gp.depth AS hops
                FROM graph_path gp
                JOIN users u1 ON gp.from_id = u1.id
                JOIN users u2 ON gp.to_id = u2.id
                ORDER BY gp.depth, u2.username;
                """
            )
            rows = cur.fetchall()
            assert len(rows) == 3
            assert rows[0] == ("Alice", "Bob", 1)
            assert rows[1] == ("Alice", "Charlie", 2)
            assert rows[2] == ("Alice", "Diana", 3)

            # 3. Stream query results directly into Polars DataFrame
            cur.execute("SELECT username, age, city FROM users WHERE city = 'London' ORDER BY age")
            col_names = [desc[0] for desc in cur.description]
            data = cur.fetchall()

            df = pl.DataFrame(data, schema=col_names, orient="row")
            assert df.shape == (3, 3)
            assert set(df["username"].to_list()) == {"Alice", "Bob", "Diana"}


# ---------------------------------------------------------------------------
# 6. Live Matrix Engine: FalkorDB (Port 6379)
# ---------------------------------------------------------------------------


class TestFalkorDBLiveMatrix:
    @pytest.fixture
    def falkor_client(self):
        try:
            from falkordb import FalkorDB

            db = FalkorDB(host="localhost", port=6379)
            g = db.select_graph("voyager_matrix_graph")
            g.query("RETURN 1")
        except Exception as e:
            pytest.skip(f"FalkorDB container not available on port 6379: {e}")
        yield g

    def test_falkordb_live_cypher_traversal_and_polars(self, falkor_client):
        # 1. Cleanup
        falkor_client.query("MATCH (n) DETACH DELETE n")

        # 2. Seed graph: Frank -> Grace
        falkor_client.query(
            """
            CREATE (a:MatrixUser {user_id: 'u1', username: 'Frank', age: 40, city: 'London'})
            CREATE (b:MatrixUser {user_id: 'u2', username: 'Grace', age: 32, city: 'Paris'})
            CREATE (a)-[:MATRIX_FOLLOWS {since: 2021}]->(b)
            """
        )

        # 3. Compile Voyager Cypher Query
        a = MatrixUser(alias="a")
        f = MatrixFollows(alias="r")
        b = MatrixUser(alias="b")

        q = (
            Query.match(a)
            .to(f)
            .node(b)
            .where(a.city == "London")
            .return_(source=a.username, target=b.username)
        )
        compiled = q.compile("cypher")

        # 4. Execute on FalkorDB
        res = falkor_client.query(compiled.statement, compiled.parameters)
        rows = [{"source": r[0], "target": r[1]} for r in res.result_set]

        # 5. Extract to Polars DataFrame
        df = pl.DataFrame(rows)
        assert df.shape == (1, 2)
        assert df["source"][0] == "Frank"
        assert df["target"][0] == "Grace"

        # 6. Cleanup
        falkor_client.query("MATCH (n) DETACH DELETE n")
