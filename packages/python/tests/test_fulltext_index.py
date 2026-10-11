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
    canonical_fulltext_index_name,
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
            s == "CREATE TEXT INDEX tagged_context_idx ON :TAGGED(context);" for s in rel_create
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


class TestLiveEngineFullTextIntegration:
    """Integration tests against live Neo4j, FalkorDB, and Memgraph instances when reachable."""

    @pytest.mark.skipif(
        not is_port_open("127.0.0.1", 7687),
        reason="Neo4j container not reachable on port 7687",
    )
    def test_live_neo4j_fulltext_lifecycle(self) -> None:
        pytest.importorskip("neo4j")
        from neo4j import GraphDatabase

        neo4j_uri = os.environ.get("NEO4J_URI", "bolt://127.0.0.1:7687")
        neo4j_user = os.environ.get("NEO4J_USER", "neo4j")
        neo4j_pass = os.environ.get("NEO4J_PASSWORD", "voyagerpass123")

        driver = GraphDatabase.driver(neo4j_uri, auth=(neo4j_user, neo4j_pass))
        try:
            session = Session(driver, dialect="cypher")

            class LiveFtDoc(Node):
                title: str = Field(index=True)
                content: str = Field()
                __fulltext_indexes__ = [
                    FullTextIndex(["title", "content"], name="live_neo4j_ft_idx")
                ]

            try:
                # 1. Apply schema (creates fulltext index)
                applied = session.apply_schema(LiveFtDoc)
                assert any("live_neo4j_ft_idx" in s for s in applied)

                # 2. Insert test node with text
                session.execute(
                    "MERGE (a:LiveFtDoc {title: 'Voyager Search', content: 'Distributed graph database search indexing'})"
                )

                # 3. Query via fulltext_search
                results = session.fulltext_search(LiveFtDoc, "distributed")
                records = results.all()
                assert len(records) > 0
                first = records[0]
                assert "score" in first
                assert first["node"]["title"] == "Voyager Search"
            finally:
                try:
                    session.execute("MATCH (a:LiveFtDoc) DETACH DELETE a")
                    session.drop_indexes(LiveFtDoc)
                except Exception:
                    pass
        finally:
            driver.close()

    @pytest.mark.skipif(
        not is_port_open("127.0.0.1", 7688),
        reason="Memgraph container not reachable on port 7688",
    )
    def test_live_memgraph_fulltext_lifecycle(self) -> None:
        pytest.importorskip("neo4j")
        from neo4j import GraphDatabase

        uri = os.environ.get("MEMGRAPH_URI", "bolt://127.0.0.1:7688")
        driver = GraphDatabase.driver(uri, auth=("", ""))
        try:
            session = Session(driver, dialect="memgraph")

            class LiveMgFtDoc(Node):
                title: str = Field()
                summary: str = Field()
                __fulltext_indexes__ = [FullTextIndex(["title", "summary"], name="live_mg_ft_idx")]

            try:
                # 1. Apply schema (creates text index)
                applied = session.apply_schema(LiveMgFtDoc)
                assert any("live_mg_ft_idx" in s for s in applied)

                # 2. Insert test node
                session.execute(
                    "CREATE (a:LiveMgFtDoc {title: 'Voyager Memgraph', summary: 'Ultra-fast memory graph analytics'})"
                )

                # 3. Query via fulltext_search
                results = session.fulltext_search(LiveMgFtDoc, "analytics")
                records = results.all()
                assert len(records) > 0
                first = records[0]
                assert "score" in first
                assert first["node"]["title"] == "Voyager Memgraph"
            finally:
                try:
                    session.execute("MATCH (a:LiveMgFtDoc) DETACH DELETE a")
                    session.drop_indexes(LiveMgFtDoc)
                except Exception:
                    pass
        finally:
            driver.close()

    @pytest.mark.skipif(
        not is_port_open("127.0.0.1", 6379),
        reason="FalkorDB container not reachable on port 6379",
    )
    def test_live_falkordb_fulltext_lifecycle(self) -> None:
        pytest.importorskip("falkordb")
        from falkordb import FalkorDB

        client = FalkorDB(host="127.0.0.1", port=6379)
        g = client.select_graph("voyager_ft_test")
        session = Session(g, dialect="falkordb")

        class LiveFalkorFtDoc(Node):
            title: str = Field()
            description: str = Field()
            __fulltext_indexes__ = [FullTextIndex(["title", "description"])]

        try:
            # 1. Apply schema (creates fulltext index)
            applied = session.create_indexes(LiveFalkorFtDoc)
            assert any("db.idx.fulltext.createNodeIndex" in s for s in applied)

            # 2. Insert test node
            session.execute(
                "CREATE (a:LiveFalkorFtDoc {title: 'Voyager FalkorDB', description: 'Low latency redis graph database'})"
            )

            # 3. Query via fulltext_search
            results = session.fulltext_search(LiveFalkorFtDoc, "latency")
            records = results.all()
            assert len(records) > 0
            first = records[0]
            assert "score" in first
            assert first["node"]["title"] == "Voyager FalkorDB"
        finally:
            try:
                session.execute("MATCH (a:LiveFalkorFtDoc) DETACH DELETE a")
                session.drop_indexes(LiveFalkorFtDoc)
            except Exception:
                pass
