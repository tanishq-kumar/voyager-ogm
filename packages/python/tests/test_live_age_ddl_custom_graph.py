"""Live tests verifying Apache AGE index DDL schema targeting on custom graphs.

Verifies:
1. Label tables in Apache AGE live in PostgreSQL schema named after the graph ("<graph>"."<Label>").
2. Index DDL created via session.create_indexes() targets "<graph>"."<Label>".
3. Full-text GIN index with (properties->>'key'::text) agtype casting succeeds on AGE label tables.
4. The created index appears in pg_indexes with schemaname matching the custom graph name.
5. session.drop_indexes() drops "<graph>".<index_name> cleanly.
6. Vector index testing with pgvector (skipped with reason if extension is unavailable).
7. Any created graph is dropped in finally/fixture teardown only if created by the test.
"""

import socket

import pytest
from voyager_ogm import (
    Field,
    FullTextIndex,
    Node,
    Query,
    Session,
    VectorProperty,
    create_bridge,
)


def _is_age_reachable(host: str = "127.0.0.1", port: int = 5455) -> bool:
    """Checks whether the Apache AGE PostgreSQL port is reachable."""
    try:
        with socket.create_connection((host, port), timeout=1.0):
            return True
    except OSError:
        return False


class ArticleDoc(Node):
    __label__ = "ArticleDoc"
    __fulltext_indexes__ = [
        FullTextIndex(["title", "body"], name="idx_articledoc_fts"),
    ]
    id: int = Field(primary_key=True)
    title: str = Field()
    body: str = Field()


class VectorArticle(Node):
    __label__ = "VectorArticle"
    id: int = Field(primary_key=True)
    title: str = Field()
    embedding: list[float] = VectorProperty(
        dimensions=4, similarity="cosine", index_name="idx_vector_article_emb"
    )


@pytest.mark.live
def test_live_age_custom_graph_fulltext_index_ddl() -> None:
    """Verifies that full-text GIN index targets "<graph>"."<Label>" on a custom graph."""
    if not _is_age_reachable():
        pytest.skip("Apache AGE container not reachable on port 5455")

    try:
        import psycopg
    except ImportError:
        pytest.skip("psycopg is required for live Apache AGE tests")

    uri = "postgresql://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph"
    graph_name = "custom_ddl_graph"

    try:
        raw_conn = psycopg.connect(uri, autocommit=True, connect_timeout=2)
    except Exception as exc:
        pytest.skip(f"Failed to connect to Apache AGE: {exc}")

    created_graph = False
    try:
        with raw_conn.cursor() as cur:
            cur.execute("LOAD 'age';")
            cur.execute('SET search_path = ag_catalog, "$user", public;')

            cur.execute(
                "SELECT count(*) FROM ag_catalog.ag_graph WHERE name = %s;",
                (graph_name,),
            )
            if cur.fetchone()[0] == 0:
                cur.execute("SELECT ag_catalog.create_graph(%s);", (graph_name,))
                created_graph = True

        # Open session configured with custom graph
        bridge = create_bridge(uri)
        session = Session(bridge=bridge, dialect="age", graph_name=graph_name)
        assert session.graph_name == graph_name

        # Create a node through session so AGE creates the label table "<graph>"."ArticleDoc"
        doc = ArticleDoc("d")
        q_create = Query.create(
            ArticleDoc("d", id=1, title="Voyager OGM", body="Fast multi-backend graph library")
        ).return_(doc.id)
        res = session.execute(q_create)
        assert len(res) == 1

        # Run session.create_indexes
        session.create_indexes(ArticleDoc)

        # Query pg_indexes to verify index exists in schema matching the graph name
        with raw_conn.cursor() as cur:
            cur.execute(
                """
                SELECT schemaname, tablename, indexname, indexdef
                FROM pg_indexes
                WHERE schemaname = %s AND indexname = 'idx_articledoc_fts';
                """,
                (graph_name,),
            )
            rows = cur.fetchall()
            assert len(rows) == 1, f"Expected index in schema '{graph_name}', found: {rows}"
            schema_name, table_name, index_name, index_def = rows[0]
            assert schema_name == graph_name
            assert table_name == "ArticleDoc"
            assert index_name == "idx_articledoc_fts"
            assert "USING gin" in index_def
            assert "to_tsvector" in index_def
            assert (
                "(properties ->> 'title'::text)" in index_def
                or "properties->>'title'::text" in index_def
            )

        # Drop index via session
        session.drop_indexes(ArticleDoc)

        # Verify index was dropped from pg_indexes
        with raw_conn.cursor() as cur:
            cur.execute(
                """
                SELECT indexname FROM pg_indexes
                WHERE schemaname = %s AND indexname = 'idx_articledoc_fts';
                """,
                (graph_name,),
            )
            assert len(cur.fetchall()) == 0

        session.close()

    finally:
        try:
            if created_graph:
                with raw_conn.cursor() as cur:
                    cur.execute("SELECT ag_catalog.drop_graph(%s, true);", (graph_name,))
            raw_conn.close()
        except Exception:
            pass


@pytest.mark.live
def test_live_age_custom_graph_vector_index_ddl() -> None:
    """Verifies that vector HNSW index targets "<graph>"."<Label>" if pgvector is installed."""
    if not _is_age_reachable():
        pytest.skip("Apache AGE container not reachable on port 5455")

    try:
        import psycopg
    except ImportError:
        pytest.skip("psycopg is required for live Apache AGE tests")

    uri = "postgresql://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph"
    graph_name = "custom_vec_graph"

    try:
        raw_conn = psycopg.connect(uri, autocommit=True, connect_timeout=2)
    except Exception as exc:
        pytest.skip(f"Failed to connect to Apache AGE: {exc}")

    # Check if pgvector extension is available in container
    try:
        with raw_conn.cursor() as cur:
            cur.execute("SELECT count(*) FROM pg_available_extensions WHERE name = 'vector';")
            has_vector = cur.fetchone()[0] > 0
    except Exception as exc:
        raw_conn.close()
        pytest.skip(f"Error querying available extensions: {exc}")

    if not has_vector:
        raw_conn.close()
        pytest.skip("pgvector extension ('vector') is not available in the Apache AGE container")

    created_graph = False
    try:
        with raw_conn.cursor() as cur:
            cur.execute("LOAD 'age';")
            cur.execute('SET search_path = ag_catalog, "$user", public;')
            cur.execute("CREATE EXTENSION IF NOT EXISTS vector;")

            cur.execute(
                "SELECT count(*) FROM ag_catalog.ag_graph WHERE name = %s;",
                (graph_name,),
            )
            if cur.fetchone()[0] == 0:
                cur.execute("SELECT ag_catalog.create_graph(%s);", (graph_name,))
                created_graph = True

        bridge = create_bridge(uri)
        session = Session(bridge=bridge, dialect="age", graph_name=graph_name)
        assert session.graph_name == graph_name

        v = VectorArticle("v")
        q_create = Query.create(
            VectorArticle("v", id=1, title="Embedding Doc", embedding=[0.1, 0.2, 0.3, 0.4])
        ).return_(v.id)
        session.execute(q_create)

        session.create_indexes(VectorArticle)

        with raw_conn.cursor() as cur:
            cur.execute(
                """
                SELECT schemaname, tablename, indexname, indexdef
                FROM pg_indexes
                WHERE schemaname = %s AND indexname = 'idx_vector_article_emb';
                """,
                (graph_name,),
            )
            rows = cur.fetchall()
            assert len(rows) == 1
            assert rows[0][0] == graph_name
            assert rows[0][1] == "VectorArticle"
            assert "USING hnsw" in rows[0][3]

        session.drop_indexes(VectorArticle)
        with raw_conn.cursor() as cur:
            cur.execute(
                """
                SELECT indexname FROM pg_indexes
                WHERE schemaname = %s AND indexname = 'idx_vector_article_emb';
                """,
                (graph_name,),
            )
            assert len(cur.fetchall()) == 0

        session.close()

    finally:
        try:
            if created_graph:
                with raw_conn.cursor() as cur:
                    cur.execute("SELECT ag_catalog.drop_graph(%s, true);", (graph_name,))
            raw_conn.close()
        except Exception:
            pass
