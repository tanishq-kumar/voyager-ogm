"""Unit and integration tests for Vector Index DDL, VectorProperty, and Vector Search."""

from __future__ import annotations

import os

import pytest
from conftest import is_port_open
from voyager_ogm import Field, Node, Query, SchemaManager, Session, VectorProperty
from voyager_ogm.schema import SchemaRegistry
from voyager_ogm.session import AsyncSession


class TestVectorPropertyValidation:
    """Tests parameter validation and canonicalization for VectorProperty."""

    def test_dimensions_positive_integer_required(self) -> None:
        with pytest.raises(ValueError, match="Vector dimensions must be a positive integer"):
            VectorProperty(dimensions=0)

        with pytest.raises(ValueError, match="Vector dimensions must be a positive integer"):
            VectorProperty(dimensions=-10)

        with pytest.raises(ValueError, match="Vector dimensions must be a positive integer"):
            VectorProperty(dimensions="1536")  # type: ignore[arg-type]

    def test_similarity_metric_validation_and_canonicalization(self) -> None:
        # Valid metrics
        vp_cos = VectorProperty(dimensions=1536, similarity="cosine")
        assert vp_cos.similarity == "cosine"

        vp_euc = VectorProperty(dimensions=1536, similarity="euclidean")
        assert vp_euc.similarity == "euclidean"

        vp_l2 = VectorProperty(dimensions=1536, similarity="l2")
        assert vp_l2.similarity == "euclidean"

        vp_dot = VectorProperty(dimensions=1536, similarity="dot")
        assert vp_dot.similarity == "dot"

        vp_ip = VectorProperty(dimensions=1536, similarity="inner_product")
        assert vp_ip.similarity == "dot"

        vp_ip2 = VectorProperty(dimensions=1536, similarity="ip")
        assert vp_ip2.similarity == "dot"

        # Invalid metrics
        with pytest.raises(ValueError, match="Unsupported vector similarity metric 'manhattan'"):
            VectorProperty(dimensions=1536, similarity="manhattan")

    def test_default_descriptor_attributes(self) -> None:
        vp = VectorProperty(dimensions=768, similarity="cosine", index_name="custom_idx")
        assert vp.dimensions == 768
        assert vp.similarity == "cosine"
        assert vp.index_name == "custom_idx"
        assert vp.index is True
        assert vp.unique is False
        assert vp.primary_key is False
        assert vp.nullable is True
        assert vp.index_type == "VECTOR"


class TestVectorModelReflectionAndDDL:
    """Tests model reflection and multi-dialect DDL generation for vector indices."""

    def test_model_reflection(self) -> None:
        class ArticleNode(Node):
            title: str = Field(unique=True)
            embedding: list[float] = VectorProperty(
                dimensions=1536, similarity="cosine", index_name="article_embed_idx"
            )

        reg = SchemaRegistry.global_registry()
        node_spec = reg.get_node("ArticleNode")
        assert node_spec is not None
        fields = node_spec["fields"]
        assert "embedding" in fields
        emb = fields["embedding"]
        assert emb["index_type"] == "VECTOR"
        assert emb["dimensions"] == 1536
        assert emb["similarity"] == "cosine"
        assert emb["index_name"] == "article_embed_idx"

    def test_neo4j_cypher_ddl_generation(self) -> None:
        class Document(Node):
            title: str = Field(index=True)
            vector: list[float] = VectorProperty(
                dimensions=1536, similarity="cosine", index_name="doc_vec_idx"
            )

        # Create DDL
        stmts = SchemaManager.generate_index_ddl(Document, dialect="cypher")
        assert len(stmts) == 2  # secondary index + vector index
        vec_stmt = next(s for s in stmts if "doc_vec_idx" in s)
        assert (
            vec_stmt
            == "CREATE VECTOR INDEX doc_vec_idx IF NOT EXISTS FOR (n:Document) ON (n.vector) OPTIONS {indexConfig: {`vector.dimensions`: 1536, `vector.similarity_function`: 'cosine'}}"
        )

        # Drop DDL
        drop_stmts = SchemaManager.generate_drop_index_ddl(Document, dialect="cypher")
        assert any("DROP INDEX doc_vec_idx IF EXISTS" in s for s in drop_stmts)

    def test_falkordb_ddl_generation(self) -> None:
        class DocumentFalkor(Node):
            text: str = Field(index=True)
            vec: list[float] = VectorProperty(
                dimensions=128, similarity="cosine", index_name="falkor_vec_idx"
            )

        # Create DDL
        stmts = SchemaManager.generate_index_ddl(DocumentFalkor, dialect="falkordb")
        vec_stmt = next(s for s in stmts if "VECTOR" in s)
        assert (
            vec_stmt
            == "CREATE VECTOR INDEX FOR (n:DocumentFalkor) ON (n.vec) OPTIONS {dimension: 128, similarityFunction: 'cosine'}"
        )

        # Drop DDL
        drop_stmts = SchemaManager.generate_drop_index_ddl(DocumentFalkor, dialect="falkordb")
        assert any(s == "DROP VECTOR INDEX FOR (n:DocumentFalkor) ON (n.vec)" for s in drop_stmts)

    def test_apache_age_and_postgres_ddl_generation(self) -> None:
        class ItemAge(Node):
            title: str = Field(index=True)
            feature: list[float] = VectorProperty(
                dimensions=384, similarity="euclidean", index_name="item_feature_idx"
            )

        # Apache AGE DDL
        age_stmts = SchemaManager.generate_index_ddl(ItemAge, dialect="age")
        assert any(
            'CREATE INDEX IF NOT EXISTS item_feature_idx ON ag_catalog."ItemAge" USING hnsw (feature vector_l2_ops);'
            in s
            for s in age_stmts
        )

        age_drop = SchemaManager.generate_drop_index_ddl(ItemAge, dialect="age")
        assert any("DROP INDEX IF EXISTS item_feature_idx;" in s for s in age_drop)

        # PostgreSQL DDL
        sql_stmts = SchemaManager.generate_index_ddl(ItemAge, dialect="postgres")
        assert any(
            'CREATE INDEX IF NOT EXISTS item_feature_idx ON "itemage" USING hnsw ("feature" vector_l2_ops);'
            in s
            for s in sql_stmts
        )

    def test_auto_generated_index_name_when_omitted(self) -> None:
        class Chunk(Node):
            content: str = Field()
            embedding: list[float] = VectorProperty(dimensions=768)

        stmts = SchemaManager.generate_index_ddl(Chunk, dialect="cypher")
        vec_stmt = next(s for s in stmts if "VECTOR" in s)
        assert (
            vec_stmt
            == "CREATE VECTOR INDEX index_chunk_embedding IF NOT EXISTS FOR (n:Chunk) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 768, `vector.similarity_function`: 'cosine'}}"
        )


class TestVectorSearchQuery:
    """Tests Query.vector_search builder for openCypher/Neo4j 5+."""

    def test_vector_search_with_model(self) -> None:
        class ArticleModel(Node):
            headline: str = Field()
            vec: list[float] = VectorProperty(dimensions=1536, index_name="art_vec_idx")

        q = Query.vector_search(ArticleModel, [0.1, 0.2, 0.3], k=5)
        compiled = q.compile()
        assert (
            compiled.statement == "CALL db.index.vector.queryNodes($p0, $p1, $p2) YIELD node, score"
        )
        assert compiled.parameters == {"p0": "art_vec_idx", "p1": 5, "p2": [0.1, 0.2, 0.3]}

    def test_vector_search_with_index_name(self) -> None:
        q = Query.vector_search(
            "custom_vector_index",
            [0.5, 0.6],
            k=20,
            yield_node="matched_doc",
            yield_score="similarity_score",
        )
        compiled = q.compile()
        assert (
            compiled.statement
            == "CALL db.index.vector.queryNodes($p0, $p1, $p2) YIELD node AS matched_doc, score AS similarity_score"
        )
        assert compiled.parameters == {"p0": "custom_vector_index", "p1": 20, "p2": [0.5, 0.6]}


class TestSessionSchemaLifecycle:
    """Tests session.apply_schema() and session.vector_search() execution."""

    def test_sync_session_apply_schema_and_vector_search(self) -> None:
        class Blog(Node):
            slug: str = Field(unique=True)
            embedding: list[float] = VectorProperty(dimensions=1536, index_name="blog_idx")

        session = Session()
        applied = session.apply_schema(Blog)
        assert len(applied) == 2
        assert any("CREATE CONSTRAINT" in s for s in applied)
        assert any("CREATE VECTOR INDEX blog_idx" in s for s in applied)

        # Vector search execution via Session facade
        res = session.vector_search(Blog, [0.1] * 1536, k=5)
        assert res is not None

    def test_sync_session_apply_schema_all_models(self) -> None:
        class AutoRegistered(Node):
            key: str = Field(unique=True)
            embed: list[float] = VectorProperty(dimensions=512)

        session = Session()
        applied = session.apply_schema()
        assert len(applied) >= 2
        assert any("AutoRegistered" in s and "VECTOR" in s for s in applied)

    @pytest.mark.asyncio
    async def test_async_session_apply_schema_and_vector_search(self) -> None:
        class AsyncBlog(Node):
            slug: str = Field(unique=True)
            embedding: list[float] = VectorProperty(dimensions=768, index_name="async_blog_idx")

        session = AsyncSession()
        applied = await session.apply_schema(AsyncBlog)
        assert len(applied) == 2
        assert any("async_blog_idx" in s for s in applied)

        res = await session.vector_search(AsyncBlog, [0.1] * 768, k=3)
        assert res is not None


class TestLiveEngineVectorIntegration:
    """Integration tests against live Neo4j and FalkorDB instances when reachable."""

    @pytest.mark.skipif(
        not is_port_open("127.0.0.1", 7687),
        reason="Neo4j container not reachable on port 7687",
    )
    def test_live_neo4j_vector_lifecycle(self) -> None:
        pytest.importorskip("neo4j")

        neo4j_uri = os.environ.get("NEO4J_URI", "bolt://127.0.0.1:7687")
        neo4j_user = os.environ.get("NEO4J_USER", "neo4j")
        neo4j_pass = os.environ.get("NEO4J_PASSWORD", "voyagerpass123")

        from neo4j import GraphDatabase

        driver = GraphDatabase.driver(neo4j_uri, auth=(neo4j_user, neo4j_pass))
        session = Session(driver, dialect="cypher")

        class LiveArticle(Node):
            title: str = Field(index=True)
            embedding: list[float] = VectorProperty(
                dimensions=4, similarity="cosine", index_name="live_art_vec_idx"
            )

        try:
            # 1. Apply vector index
            applied = session.apply_schema(LiveArticle)
            assert any("live_art_vec_idx" in s for s in applied)

            # 2. Insert test node with vector
            session.execute(
                "MERGE (a:LiveArticle {title: 'Voyager Vector Test'}) "
                "SET a.embedding = [1.0, 0.0, 0.0, 0.0]"
            )

            # 3. Query via vector_search
            results = session.vector_search(
                LiveArticle, [1.0, 0.0, 0.0, 0.0], k=1, yield_node="n", yield_score="score"
            )
            assert len(results) > 0
            first = results[0]
            assert "score" in first
            assert first["score"] > 0.99  # Identical cosine similarity
        finally:
            # 4. Clean up
            try:
                session.execute("MATCH (a:LiveArticle) DETACH DELETE a")
                session.drop_indexes(LiveArticle)
            except Exception:
                pass
            driver.close()

    @pytest.mark.skipif(
        not is_port_open("127.0.0.1", 6379),
        reason="FalkorDB container not reachable on port 6379",
    )
    def test_live_falkordb_vector_lifecycle(self) -> None:
        pytest.importorskip("falkordb")
        from falkordb import FalkorDB

        client = FalkorDB(host="127.0.0.1", port=6379)
        g = client.select_graph("voyager_vector_test")
        session = Session(g, dialect="falkordb")

        class FalkorLiveDoc(Node):
            title: str = Field(index=True)
            vector: list[float] = VectorProperty(
                dimensions=128, similarity="cosine", index_name="falkor_doc_vec_idx"
            )

        try:
            # Apply schema creates vector index on FalkorDB
            applied = session.create_indexes(FalkorLiveDoc)
            assert any("CREATE VECTOR INDEX" in s for s in applied)
        finally:
            try:
                session.drop_indexes(FalkorLiveDoc)
            except Exception:
                pass
