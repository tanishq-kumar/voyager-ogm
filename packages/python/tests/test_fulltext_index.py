"""Unit and integration tests for Full-Text Index DDL, FullTextIndex descriptor, and Full-Text Search."""

from __future__ import annotations

import os

import pytest
from conftest import is_port_open
from voyager_ogm import (
    Field,
    FullTextIndex,
    Node,
    Query,
    Relationship,
    SchemaManager,
    Session,
    TextIndex,
    VectorProperty,
    canonical_fulltext_index_name,
    fulltext_index,
)
from voyager_ogm._voyager_rs import (
    canonical_fulltext_index_name as native_canonical_fulltext_index_name,
)
from voyager_ogm.schema import SchemaRegistry
from voyager_ogm.session import AsyncSession


class TestFullTextIndexValidation:
    """Tests parameter validation and configuration for FullTextIndex."""

    def test_properties_required(self) -> None:
        with pytest.raises(ValueError, match="requires at least one property name"):
            FullTextIndex(properties=[])

    def test_single_property_normalized_to_list(self) -> None:
        fti = FullTextIndex(properties="headline")
        assert fti.properties == ["headline"]
        assert fti.name is None
        assert fti.analyzer == "standard"

    def test_multi_property_and_analyzer(self) -> None:
        fti = FullTextIndex(
            properties=["title", "content"],
            name="custom_fts_idx",
            analyzer="english",
        )
        assert fti.properties == ["title", "content"]
        assert fti.name == "custom_fts_idx"
        assert fti.analyzer == "english"

    def test_to_dict_serialization(self) -> None:
        fti = FullTextIndex(properties=["abstract", "body"], name="paper_fts")
        d = fti.to_dict()
        assert d == {
            "name": "paper_fts",
            "properties": ["abstract", "body"],
            "analyzer": "standard",
        }


class TestFullTextModelReflectionAndDDL:
    """Tests schema reflection and multi-dialect DDL generation for full-text indices."""

    def test_node_model_reflection(self) -> None:
        class DocumentNode(Node):
            __fulltext_indexes__ = [
                FullTextIndex(["title", "body"], name="doc_fulltext_idx", analyzer="english"),
            ]
            title: str = Field()
            body: str = Field()

        reg = SchemaRegistry.global_registry()
        node_spec = reg.get_node("DocumentNode")
        assert node_spec is not None
        assert "fulltext_indexes" in node_spec
        ft_indexes = node_spec["fulltext_indexes"]
        assert len(ft_indexes) == 1
        assert ft_indexes[0]["name"] == "doc_fulltext_idx"
        assert ft_indexes[0]["properties"] == ["title", "body"]
        assert ft_indexes[0]["analyzer"] == "english"

    def test_relationship_model_reflection(self) -> None:
        class ReviewedRel(Relationship):
            __fulltext_indexes__ = [
                FullTextIndex("notes", name="review_notes_idx"),
            ]
            notes: str = Field()

        reg = SchemaRegistry.global_registry()
        rel_spec = reg.get_relationship("ReviewedRel")
        assert rel_spec is not None
        assert "fulltext_indexes" in rel_spec
        ft_indexes = rel_spec["fulltext_indexes"]
        assert len(ft_indexes) == 1
        assert ft_indexes[0]["name"] == "review_notes_idx"
        assert ft_indexes[0]["properties"] == ["notes"]

    def test_tuple_fulltext_indexes_reflection_and_ddl(self) -> None:
        class TupleDoc(Node):
            __fulltext_indexes__ = (FullTextIndex(["title", "body"], name="tuple_doc_idx"),)
            title: str = Field()
            body: str = Field()

        reg = SchemaRegistry.global_registry()
        spec = reg.get_node("TupleDoc")
        assert spec is not None
        assert spec["fulltext_indexes"][0]["name"] == "tuple_doc_idx"

        ddl = SchemaManager.generate_index_ddl(TupleDoc, dialect="cypher")
        assert any("CREATE FULLTEXT INDEX tuple_doc_idx" in s for s in ddl)

    def test_neo4j_cypher_node_and_rel_ddl(self) -> None:
        class Article(Node):
            __fulltext_indexes__ = [
                FullTextIndex(["headline", "summary"]),
            ]
            headline: str = Field()
            summary: str = Field()

        class Authored(Relationship):
            __fulltext_indexes__ = [
                FullTextIndex("comments"),
            ]
            comments: str = Field()

        # Node CREATE & DROP
        create_stmts = SchemaManager.generate_index_ddl(Article, dialect="cypher")
        assert any(
            s
            == "CREATE FULLTEXT INDEX index_article_headline_summary IF NOT EXISTS FOR (n:Article) ON EACH [n.headline, n.summary] OPTIONS {indexConfig: {`fulltext.analyzer`: 'standard'}}"
            for s in create_stmts
        )

        drop_stmts = SchemaManager.generate_drop_index_ddl(Article, dialect="cypher")
        assert any(s == "DROP INDEX index_article_headline_summary IF EXISTS" for s in drop_stmts)

        # Relationship CREATE & DROP
        rel_create = SchemaManager.generate_index_ddl(Authored, dialect="cypher")
        assert any(
            s
            == "CREATE FULLTEXT INDEX index_authored_comments IF NOT EXISTS FOR ()-[r:AUTHORED]-() ON EACH [r.comments] OPTIONS {indexConfig: {`fulltext.analyzer`: 'standard'}}"
            for s in rel_create
        )

        rel_drop = SchemaManager.generate_drop_index_ddl(Authored, dialect="cypher")
        assert any(s == "DROP INDEX index_authored_comments IF EXISTS" for s in rel_drop)

    def test_memgraph_node_and_rel_ddl(self) -> None:
        class Post(Node):
            __fulltext_indexes__ = [
                FullTextIndex(["title", "content"], name="post_search_idx"),
            ]
            title: str = Field()
            content: str = Field()

        class Tagged(Relationship):
            __fulltext_indexes__ = [
                FullTextIndex("context", name="tagged_context_idx"),
            ]
            context: str = Field()

        # Node CREATE & DROP
        create_stmts = SchemaManager.generate_index_ddl(Post, dialect="memgraph")
        assert any(
            s == "CREATE TEXT INDEX post_search_idx ON :Post(title, content);" for s in create_stmts
        )
        drop_stmts = SchemaManager.generate_drop_index_ddl(Post, dialect="memgraph")
        assert any(s == "DROP TEXT INDEX post_search_idx;" for s in drop_stmts)

        # Edge CREATE & DROP
        rel_create = SchemaManager.generate_index_ddl(Tagged, dialect="memgraph")
        assert any(
            s == "CREATE TEXT EDGE INDEX tagged_context_idx ON :TAGGED(context);"
            for s in rel_create
        )
        rel_drop = SchemaManager.generate_drop_index_ddl(Tagged, dialect="memgraph")
        assert any(s == "DROP TEXT INDEX tagged_context_idx;" for s in rel_drop)

    def test_falkordb_node_ddl_and_unsupported_rel(self) -> None:
        class Product(Node):
            __fulltext_indexes__ = [
                FullTextIndex(["name", "description"]),
            ]
            name: str = Field()
            description: str = Field()

        class Purchased(Relationship):
            __fulltext_indexes__ = [
                FullTextIndex("notes"),
            ]
            notes: str = Field()

        # Node CREATE & DROP
        create_stmts = SchemaManager.generate_index_ddl(Product, dialect="falkordb")
        assert any(
            s == "CALL db.idx.fulltext.createNodeIndex('Product', 'name', 'description')"
            for s in create_stmts
        )
        drop_stmts = SchemaManager.generate_drop_index_ddl(Product, dialect="falkordb")
        assert any(s == "CALL db.idx.fulltext.drop('Product')" for s in drop_stmts)

        # Relationship CREATE should raise error (FalkorDB doesn't support rel fulltext index DDL)
        with pytest.raises(
            Exception, match="FalkorDB does not support Full-text index creation on relationships"
        ):
            SchemaManager.generate_index_ddl(Purchased, dialect="falkordb")

    def test_duckdb_node_ddl(self) -> None:
        class Book(Node):
            __fulltext_indexes__ = [
                FullTextIndex(["title", "synopsis"]),
            ]
            id: str = Field(primary_key=True)
            title: str = Field()
            synopsis: str = Field()

        create_stmts = SchemaManager.generate_index_ddl(Book, dialect="duckdb")
        assert any(
            s == "PRAGMA create_fts_index('book', 'id', 'title', 'synopsis');" for s in create_stmts
        )
        drop_stmts = SchemaManager.generate_drop_index_ddl(Book, dialect="duckdb")
        assert any(s == "PRAGMA drop_fts_index('book');" for s in drop_stmts)

    def test_age_sql_gin_ddl(self) -> None:
        class Message(Node):
            __fulltext_indexes__ = [
                FullTextIndex(["subject", "body"]),
            ]
            subject: str = Field()
            body: str = Field()

        create_stmts = SchemaManager.generate_index_ddl(Message, dialect="age")
        assert any("idx_message_subject_body_fts" in s and "USING gin" in s for s in create_stmts)
        drop_stmts = SchemaManager.generate_drop_index_ddl(Message, dialect="age")
        assert any(s == "DROP INDEX IF EXISTS idx_message_subject_body_fts;" for s in drop_stmts)


class TestCanonicalFullTextIndexName:
    """Tests canonical_fulltext_index_name deterministic naming across dialects."""

    def test_canonical_name_matches_rust(self) -> None:
        dialects = ["cypher", "memgraph", "falkordb", "duckdb", "sql", "age"]
        for d in dialects:
            py_name = canonical_fulltext_index_name("MyEntity", ["title", "desc"], dialect=d)
            rs_name = native_canonical_fulltext_index_name("MyEntity", ["title", "desc"], d)
            assert py_name == rs_name

    def test_naming_conventions_per_dialect(self) -> None:
        assert (
            canonical_fulltext_index_name("Paper", ["title", "abstract"], "cypher")
            == "index_paper_title_abstract"
        )
        assert (
            canonical_fulltext_index_name("Paper", ["title", "abstract"], "memgraph")
            == "index_paper_title_abstract"
        )
        assert (
            canonical_fulltext_index_name("Paper", ["title", "abstract"], "falkordb")
            == "index_paper_title_abstract"
        )
        assert (
            canonical_fulltext_index_name("Paper", ["title", "abstract"], "duckdb")
            == "idx_paper_title_abstract_fts"
        )
        assert (
            canonical_fulltext_index_name("Paper", ["title", "abstract"], "age")
            == "idx_paper_title_abstract_fts"
        )


class TestFullTextSearchQuery:
    """Tests Query.fulltext_search builder for openCypher/Neo4j, Memgraph, and FalkorDB."""

    def test_fulltext_search_node_cypher(self) -> None:
        class DocModel(Node):
            __fulltext_indexes__ = [FullTextIndex(["title", "body"])]
            title: str = Field()
            body: str = Field()

        q = Query.fulltext_search(DocModel, "machine learning")
        compiled = q.compile()
        assert compiled.statement == "CALL db.index.fulltext.queryNodes($p0, $p1) YIELD node, score"
        assert compiled.parameters == {
            "p0": "index_docmodel_title_body",
            "p1": "machine learning",
        }

    def test_fulltext_search_relationship_cypher(self) -> None:
        class AuthoredRel(Relationship):
            __fulltext_indexes__ = [FullTextIndex("notes")]
            notes: str = Field()

        q = Query.fulltext_search(AuthoredRel, "peer review", yield_node="rel", yield_score="rank")
        compiled = q.compile()
        assert (
            compiled.statement
            == "CALL db.index.fulltext.queryRelationships($p0, $p1) YIELD relationship AS rel, score AS rank"
        )
        assert compiled.parameters == {
            "p0": "index_authoredrel_notes",
            "p1": "peer review",
        }

    def test_fulltext_search_node_memgraph(self) -> None:
        class MgArticle(Node):
            __fulltext_indexes__ = [FullTextIndex("content", name="mg_content_idx")]
            content: str = Field()

        q = Query.fulltext_search(MgArticle, "graph algorithms", dialect="memgraph")
        compiled = q.compile()
        assert compiled.statement == "CALL text_search.search_all($p0, $p1) YIELD node, score"
        assert compiled.parameters == {
            "p0": "mg_content_idx",
            "p1": "graph algorithms",
        }

    def test_fulltext_search_relationship_memgraph(self) -> None:
        class MgEdge(Relationship):
            __fulltext_indexes__ = [FullTextIndex("summary", name="mg_edge_idx")]
            summary: str = Field()

        q = Query.fulltext_search(MgEdge, "transaction", yield_node="e", dialect="memgraph")
        compiled = q.compile()
        assert (
            compiled.statement
            == "CALL text_search.search_all_edges($p0, $p1) YIELD edge AS e, score"
        )
        assert compiled.parameters == {
            "p0": "mg_edge_idx",
            "p1": "transaction",
        }

    def test_fulltext_search_node_falkordb(self) -> None:
        class FalkorItem(Node):
            __fulltext_indexes__ = [FullTextIndex(["name", "details"])]
            name: str = Field()
            details: str = Field()

        q = Query.fulltext_search(FalkorItem, "database engine", dialect="falkordb")
        compiled = q.compile()
        # FalkorDB targets the node label as first argument
        assert compiled.statement == "CALL db.idx.fulltext.queryNodes($p0, $p1) YIELD node, score"
        assert compiled.parameters == {
            "p0": "FalkorItem",
            "p1": "database engine",
        }

    def test_fulltext_search_relationship_falkordb(self) -> None:
        class FalkorRel(Relationship, type_="CONNECTED_TO"):
            __fulltext_indexes__ = [FullTextIndex("notes")]
            notes: str = Field()

        q = Query.fulltext_search(FalkorRel, "high-speed", dialect="falkordb")
        compiled = q.compile()
        assert (
            compiled.statement
            == "CALL db.idx.fulltext.queryRelationships($p0, $p1) YIELD relationship AS node, score"
        )
        assert compiled.parameters == {
            "p0": "CONNECTED_TO",
            "p1": "high-speed",
        }

    def test_fulltext_search_by_index_name_string(self) -> None:
        q = Query.fulltext_search("custom_idx", "term", yield_node="item", yield_score="s")
        compiled = q.compile()
        assert (
            compiled.statement
            == "CALL db.index.fulltext.queryNodes($p0, $p1) YIELD node AS item, score AS s"
        )
        assert compiled.parameters == {
            "p0": "custom_idx",
            "p1": "term",
        }

    def test_memgraph_colon_query_field_adaptation(self) -> None:
        class MgArticle(Node):
            __fulltext_indexes__ = [FullTextIndex(["title", "content"], name="mg_content_idx")]
            title: str = Field()
            content: str = Field()

        class MgEdge(Relationship):
            __fulltext_indexes__ = [FullTextIndex("summary", name="mg_edge_idx")]
            summary: str = Field()

        # Node colon query -> text_search.search with data. namespace adaptation
        q_node = Query.fulltext_search(MgArticle, "title:algorithms", dialect="memgraph")
        c_node = q_node.compile()
        assert c_node.statement == "CALL text_search.search($p0, $p1) YIELD node, score"
        assert c_node.parameters == {
            "p0": "mg_content_idx",
            "p1": "data.title:algorithms",
        }

        # Edge colon query -> text_search.search_edges with data. namespace adaptation
        q_edge = Query.fulltext_search(MgEdge, "summary:transaction", dialect="memgraph")
        c_edge = q_edge.compile()
        assert (
            c_edge.statement == "CALL text_search.search_edges($p0, $p1) YIELD edge AS node, score"
        )
        assert c_edge.parameters == {
            "p0": "mg_edge_idx",
            "p1": "data.summary:transaction",
        }

        # Property with regex metacharacters properly escaped via re.escape
        class SpecialFieldDoc(Node):
            __fulltext_indexes__ = [
                FullTextIndex(["meta.title", "plain_field"], name="special_idx")
            ]
            plain_field: str = Field()

        q_special = Query.fulltext_search(SpecialFieldDoc, "meta.title:test", dialect="memgraph")
        c_special = q_special.compile()
        assert c_special.statement == "CALL text_search.search($p0, $p1) YIELD node, score"
        assert c_special.parameters == {
            "p0": "special_idx",
            "p1": "data.meta.title:test",
        }

    def test_fulltext_search_custom_index_no_lossy_split(self) -> None:
        # A custom index name starting with 'index_' like 'index_search' must not be split into 'search'
        q = Query.fulltext_search("index_search", "query", dialect="falkordb")
        c = q.compile()
        assert c.statement == "CALL db.idx.fulltext.queryNodes($p0, $p1) YIELD node, score"
        assert c.parameters == {
            "p0": "index_search",
            "p1": "query",
        }


class TestDeveloperErgonomicsAliases:
    """Tests developer ergonomics aliases: TextIndex, text_search, and model-level entrypoints."""

    def test_text_index_alias_and_model_reflection(self) -> None:
        assert TextIndex is FullTextIndex
        assert fulltext_index is FullTextIndex

        class ArticleWithAlias(Node):
            __fulltext_indexes__ = [TextIndex(["headline", "content"], name="article_alias_idx")]
            headline: str = Field()
            content: str = Field()

        reg = SchemaRegistry.global_registry()
        node_spec = reg.get_node("ArticleWithAlias")
        assert node_spec is not None
        assert node_spec["fulltext_indexes"][0]["name"] == "article_alias_idx"

    def test_query_text_search_alias(self) -> None:
        q1 = Query.fulltext_search("my_idx", "graph traversal")
        q2 = Query.text_search("my_idx", "graph traversal")
        assert q1.compile("cypher").statement == q2.compile("cypher").statement

    def test_session_text_search_alias(self) -> None:
        session = Session()
        assert hasattr(session, "text_search")
        assert session.text_search == session.fulltext_search

    @pytest.mark.asyncio
    async def test_async_session_text_search_alias(self) -> None:
        session = AsyncSession()
        assert hasattr(session, "text_search")
        assert session.text_search == session.fulltext_search

    def test_node_and_relationship_model_search_entrypoints(self) -> None:
        class Paper(Node):
            __fulltext_indexes__ = [TextIndex(["abstract"], name="paper_abstract_idx")]
            abstract: str = Field()
            embedding: list[float] = VectorProperty(dimensions=128)

        class Cited(Relationship):
            __fulltext_indexes__ = [TextIndex(["context"], name="cited_context_idx")]
            context: str = Field()

        # 1. Model text_search returns executable Query
        q_ft = Paper.text_search("deep learning")
        c_ft = q_ft.compile("cypher")
        assert (
            "paper_abstract_idx" in c_ft.statement
            or "paper_abstract_idx" in c_ft.parameters.values()
        )

        # 2. Model fulltext_search returns executable Query
        q_ft2 = Paper.fulltext_search("neural networks")
        assert q_ft2.compile("cypher").statement == q_ft.compile("cypher").statement

        # 3. Model vector_search returns executable Query
        q_vec = Paper.vector_search([0.1] * 128, k=5)
        c_vec = q_vec.compile("cypher")
        assert "db.index.vector.queryNodes" in c_vec.statement

        # 4. Relationship text_search & fulltext_search return executable Query
        q_rel = Cited.text_search("survey paper")
        c_rel = q_rel.compile("cypher")
        assert (
            "cited_context_idx" in c_rel.statement
            or "cited_context_idx" in c_rel.parameters.values()
        )


class TestSessionFullTextLifecycle:
    """Tests session.apply_schema() and session.fulltext_search() execution."""

    def test_sync_session_apply_schema_and_fulltext_search(self) -> None:
        class ArticleBlog(Node):
            slug: str = Field(unique=True)
            title: str = Field()
            body: str = Field()
            __fulltext_indexes__ = [FullTextIndex(["title", "body"], name="blog_ft_idx")]

        session = Session()
        applied = session.apply_schema(ArticleBlog)
        assert any("blog_ft_idx" in s for s in applied)

        res = session.fulltext_search(ArticleBlog, "tutorial")
        assert res is not None

        # Verify text_search alias
        res_alias = session.text_search(ArticleBlog, "tutorial")
        assert res_alias is not None

    @pytest.mark.asyncio
    async def test_async_session_apply_schema_and_fulltext_search(self) -> None:
        class AsyncArticle(Node):
            slug: str = Field(unique=True)
            body: str = Field()
            __fulltext_indexes__ = [FullTextIndex("body", name="async_ft_idx")]

        session = AsyncSession()
        applied = await session.apply_schema(AsyncArticle)
        assert any("async_ft_idx" in s for s in applied)

        res = await session.fulltext_search(AsyncArticle, "async guide")
        assert res is not None

        res_alias = await session.text_search(AsyncArticle, "async guide")
        assert res_alias is not None


class TestLiveEngineFullTextIntegration:
    """Integration tests against live Neo4j, FalkorDB, and Memgraph instances when reachable."""

    @pytest.mark.skipif(
        not is_port_open("127.0.0.1", 7687),
        reason="Neo4j container not reachable on port 7687",
    )
    def test_live_neo4j_rich_multi_doc_and_relationship(self) -> None:
        pytest.importorskip("neo4j")
        from neo4j import GraphDatabase

        neo4j_uri = os.environ.get("NEO4J_URI", "bolt://127.0.0.1:7687")
        neo4j_user = os.environ.get("NEO4J_USER", "neo4j")
        neo4j_pass = os.environ.get("NEO4J_PASSWORD", "voyagerpass123")

        driver = GraphDatabase.driver(neo4j_uri, auth=(neo4j_user, neo4j_pass))
        try:
            session = Session(driver, dialect="cypher")

            class LiveNeoDoc(Node):
                title: str = Field()
                body: str = Field()
                __fulltext_indexes__ = [TextIndex(["title", "body"], name="live_neo_doc_ft")]

            class LiveReviewedRel(Relationship):
                comment: str = Field()
                __fulltext_indexes__ = [TextIndex(["comment"], name="live_neo_rev_ft")]

            try:
                # 1. Clean up & Apply schema (creates fulltext indexes on node & rel)
                try:
                    session.drop_indexes(LiveNeoDoc, LiveReviewedRel)
                except Exception:
                    pass
                applied = session.apply_schema(LiveNeoDoc, LiveReviewedRel)
                assert any("live_neo_doc_ft" in s for s in applied)
                assert any("live_neo_rev_ft" in s for s in applied)

                # 2. Populate rich multi-document corpus and relationship
                session.execute("MATCH (a:LiveNeoDoc) DETACH DELETE a")
                session.execute("""
                    CREATE (d1:LiveNeoDoc {
                        title: 'Graph Databases in Practice',
                        body: 'Neo4j and openCypher allow complex graph traversal, pattern matching, and high performance graph queries.'
                    }),
                    (d2:LiveNeoDoc {
                        title: 'Vector Search and Embeddings',
                        body: 'Dense vectors enable semantic similarity and retrieval augmented generation across documents.'
                    }),
                    (d3:LiveNeoDoc {
                        title: 'Hybrid GraphRAG Systems',
                        body: 'Combining graph traversal, vector embeddings, and fulltext search yields high precision reasoning.'
                    }),
                    (d1)-[:LIVEREVIEWEDREL {comment: 'stellar comprehensive graph query analysis'}]->(d3)
                """)

                # 3. Single term query with BM25 score ranking
                results_term = session.text_search(LiveNeoDoc, "graph").all()
                assert len(results_term) >= 2
                assert all("score" in r and r["score"] > 0 for r in results_term)
                # d1 has multiple occurrences of 'graph', so it ranks top
                assert results_term[0]["node"]["title"] == "Graph Databases in Practice"
                assert results_term[0]["score"] >= results_term[1]["score"]

                # 4. Boolean AND query
                results_bool = session.text_search(LiveNeoDoc, "vector AND retrieval").all()
                assert len(results_bool) >= 1
                assert results_bool[0]["node"]["title"] == "Vector Search and Embeddings"

                # 5. Relationship full-text search via model entrypoint
                query_rel = LiveReviewedRel.text_search("stellar")
                rel_results = session.execute(query_rel).all()
                assert len(rel_results) >= 1
                assert "score" in rel_results[0]
                assert rel_results[0]["score"] > 0
                assert (
                    rel_results[0]["node"]["comment"]
                    == "stellar comprehensive graph query analysis"
                )
            finally:
                try:
                    session.execute("MATCH (a:LiveNeoDoc) DETACH DELETE a")
                    session.drop_indexes(LiveNeoDoc, LiveReviewedRel)
                except Exception:
                    pass
        finally:
            driver.close()

    @pytest.mark.skipif(
        not is_port_open("127.0.0.1", 7688),
        reason="Memgraph container not reachable on port 7688",
    )
    def test_live_memgraph_rich_multi_doc_and_relationship(self) -> None:
        pytest.importorskip("neo4j")
        from neo4j import GraphDatabase

        uri = os.environ.get("MEMGRAPH_URI", "bolt://127.0.0.1:7688")
        driver = GraphDatabase.driver(uri, auth=("", ""))
        try:
            session = Session(driver, dialect="memgraph")

            class LiveMgDoc(Node):
                title: str = Field()
                summary: str = Field()
                __fulltext_indexes__ = [TextIndex(["title", "summary"], name="live_mg_doc_ft")]

            class LiveMgReviewedRel(Relationship):
                comment: str = Field()
                __fulltext_indexes__ = [TextIndex(["comment"], name="live_mg_rev_ft")]

            try:
                # 1. Clean up & Apply schema (creates Tantivy node index and edge index)
                try:
                    session.drop_indexes(LiveMgDoc, LiveMgReviewedRel)
                except Exception:
                    pass
                applied = session.apply_schema(LiveMgDoc, LiveMgReviewedRel)
                assert any("live_mg_doc_ft" in s for s in applied)
                assert any("live_mg_rev_ft" in s for s in applied)

                # 2. Populate rich multi-document corpus and edge
                session.execute("MATCH (a:LiveMgDoc) DETACH DELETE a")
                session.execute("""
                    CREATE (d1:LiveMgDoc {
                        title: 'In-Memory Graph Analytics',
                        summary: 'Memgraph provides high throughput MAGE algorithms and in-memory graph storage.'
                    }),
                    (d2:LiveMgDoc {
                        title: 'Tantivy Full-Text Engine',
                        summary: 'Tantivy powers lightning-fast text indexing and BM25 relevance scoring.'
                    }),
                    (d3:LiveMgDoc {
                        title: 'Graph Streaming and Real-time Kafka',
                        summary: 'Kafka streaming connects directly into Memgraph for real-time graph updates.'
                    }),
                    (d1)-[:LIVEMGREVIEWEDREL {comment: 'superb in-memory real-time graph performance'}]->(d3)
                """)

                # 3. Term search with score ranking
                results_term = session.text_search(LiveMgDoc, "graph").all()
                assert len(results_term) >= 2
                assert all("score" in r and r["score"] > 0 for r in results_term)

                # 4. Boolean AND query
                results_bool = session.text_search(LiveMgDoc, "tantivy AND indexing").all()
                assert len(results_bool) >= 1
                assert results_bool[0]["node"]["title"] == "Tantivy Full-Text Engine"

                # 5. Relationship full-text search via model entrypoint
                query_rel = LiveMgReviewedRel.text_search("superb", dialect="memgraph")
                rel_results = session.execute(query_rel).all()
                assert len(rel_results) >= 1
                assert "score" in rel_results[0]
                assert rel_results[0]["score"] > 0
                assert (
                    rel_results[0]["node"]["comment"]
                    == "superb in-memory real-time graph performance"
                )

                # 6. Colon field query on Node (executes text_search.search with data. namespace adaptation)
                results_colon = session.text_search(LiveMgDoc, "title:Tantivy").all()
                assert len(results_colon) == 1
                assert results_colon[0]["node"]["title"] == "Tantivy Full-Text Engine"
                assert results_colon[0]["score"] > 0

                # 7. Colon field query on Relationship (executes text_search.search_edges with data. namespace adaptation)
                results_edge_colon = session.text_search(LiveMgReviewedRel, "comment:superb").all()
                assert len(results_edge_colon) == 1
                assert (
                    results_edge_colon[0]["node"]["comment"]
                    == "superb in-memory real-time graph performance"
                )
                assert results_edge_colon[0]["score"] > 0
            finally:
                try:
                    session.execute("MATCH (a:LiveMgDoc) DETACH DELETE a")
                    session.drop_indexes(LiveMgDoc, LiveMgReviewedRel)
                except Exception:
                    pass
        finally:
            driver.close()

    @pytest.mark.skipif(
        not is_port_open("127.0.0.1", 6379),
        reason="FalkorDB container not reachable on port 6379",
    )
    def test_live_falkordb_rich_multi_doc(self) -> None:
        pytest.importorskip("falkordb")
        from falkordb import FalkorDB

        client = FalkorDB(host="127.0.0.1", port=6379)
        g = client.select_graph("voyager_ft_rich_test")
        session = Session(g, dialect="falkordb")

        class LiveFalkorDoc(Node):
            title: str = Field()
            description: str = Field()
            __fulltext_indexes__ = [TextIndex(["title", "description"])]

        try:
            # 1. Clean up & Create indexes
            try:
                session.drop_indexes(LiveFalkorDoc)
            except Exception:
                pass
            applied = session.create_indexes(LiveFalkorDoc)
            assert any("db.idx.fulltext.createNodeIndex" in s for s in applied)

            # 2. Populate multi-document corpus
            session.execute("MATCH (a:LiveFalkorDoc) DETACH DELETE a")
            session.execute("""
                CREATE (:LiveFalkorDoc {title: 'Low Latency Graph Queries', description: 'FalkorDB leverages Redis data structures and RediSearch for sub-millisecond responses.'}),
                       (:LiveFalkorDoc {title: 'RediSearch Integration', description: 'Fulltext search queries are executed directly on RediSearch index trees.'}),
                       (:LiveFalkorDoc {title: 'Graph Algorithms and Traversal', description: 'Graph traversal matrix multiplication algorithms provide high performance.'})
            """)

            # 3. Query term matching multiple docs
            res_graph = session.text_search(LiveFalkorDoc, "graph").all()
            assert len(res_graph) == 2
            assert all("score" in r and r["score"] > 0 for r in res_graph)

            # 4. Query term matching specific doc via model entrypoint
            res_redis = session.execute(
                LiveFalkorDoc.text_search("redis", dialect="falkordb")
            ).all()
            assert len(res_redis) >= 1
            assert res_redis[0]["node"]["title"] == "Low Latency Graph Queries"
            assert res_redis[0]["score"] > 0
        finally:
            try:
                session.execute("MATCH (a:LiveFalkorDoc) DETACH DELETE a")
                session.drop_indexes(LiveFalkorDoc)
            except Exception:
                pass
