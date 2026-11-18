"""Voyager OGM: Vector & Full-Text Search Index DDL and Queries.

Teaches:
    - Defining vector embeddings on graph entities using `VectorProperty`
    - Defining BM25 full-text search indexes using `FullTextIndex`
    - Multi-dialect vector index DDL generation (Neo4j, Memgraph, DuckDB, PostgreSQL pgvector)
    - Multi-dialect full-text index DDL generation (Neo4j, Memgraph Tantivy)
    - Constructing vector similarity search queries with `Query.vector_search()`
    - Constructing full-text search queries with `Query.fulltext_search()`

Prerequisites:
    - Engine: None (offline / compile-only)
    - Extras: None

Run with:
    `uv run python examples/python/12_vector_and_fulltext_search.py`
"""

from __future__ import annotations

from voyager_ogm import (
    Field,
    FullTextIndex,
    Node,
    Query,
    Relationship,
    SchemaManager,
    VectorProperty,
    node,
    relationship,
)


# -----------------------------------------------------------------------------
# 1. Entity Models with Vector & Full-Text Descriptors
# -----------------------------------------------------------------------------
@node(label="Document")
class Document(Node):
    __fulltext_indexes__ = [
        FullTextIndex(["title", "body"], name="doc_fulltext_idx", analyzer="english")
    ]

    doc_id: str = Field(primary_key=True)
    title: str = Field(index=True)
    body: str = Field()
    embedding: list[float] = VectorProperty(
        dimensions=3,  # 3-dim embedding for demo; set 1536 for OpenAI / 384 for sentence-transformers
        similarity="cosine",
        index_name="doc_embedding_idx",
    )


@relationship(type_name="REFERENCES")
class References(Relationship):
    __fulltext_indexes__ = [
        FullTextIndex("citation_note", name="ref_notes_idx", analyzer="standard")
    ]

    citation_note: str = Field()
    weight: float = Field(default=1.0)


def main() -> None:
    print("=== Voyager OGM: Vector & Full-Text Search ===\n")

    # -------------------------------------------------------------------------
    # 2. Multi-Dialect Vector Index DDL Generation
    # -------------------------------------------------------------------------
    print("--- [1] Multi-Dialect Vector Index DDL ---")

    # Neo4j 5+ (db.index.vector)
    ddl_neo4j = SchemaManager.generate_index_ddl(Document, dialect="cypher")
    vec_ddl_neo4j = next(s for s in ddl_neo4j if "VECTOR" in s)
    print(f"Neo4j / openCypher:\n  {vec_ddl_neo4j}\n")

    # Memgraph (MAGE vector index)
    ddl_memgraph = SchemaManager.generate_index_ddl(Document, dialect="memgraph")
    vec_ddl_mg = next(s for s in ddl_memgraph if "VECTOR" in s)
    print(f"Memgraph:\n  {vec_ddl_mg}\n")

    # DuckDB (HNSW extension)
    ddl_duckdb = SchemaManager.generate_index_ddl(Document, dialect="duckdb")
    vec_ddl_duck = next(s for s in ddl_duckdb if "HNSW" in s)
    print(f"DuckDB HNSW:\n  {vec_ddl_duck}\n")

    # PostgreSQL (pgvector HNSW)
    ddl_pg = SchemaManager.generate_index_ddl(Document, dialect="postgres")
    vec_ddl_pg = next(s for s in ddl_pg if "hnsw" in s)
    print(f"PostgreSQL pgvector:\n  {vec_ddl_pg}\n")

    # -------------------------------------------------------------------------
    # 3. Multi-Dialect Full-Text Index DDL Generation
    # -------------------------------------------------------------------------
    print("--- [2] Multi-Dialect Full-Text Index DDL ---")

    # Neo4j Full-Text Index
    ft_ddl_neo4j = SchemaManager.generate_index_ddl(Document, dialect="cypher")
    node_ft_neo4j = next(s for s in ft_ddl_neo4j if "FULLTEXT" in s)
    print(f"Neo4j Node Full-Text:\n  {node_ft_neo4j}\n")

    # Memgraph Tantivy Full-Text Index
    ft_ddl_mg = SchemaManager.generate_index_ddl(Document, dialect="memgraph")
    node_ft_mg = next(s for s in ft_ddl_mg if "TEXT INDEX" in s)
    print(f"Memgraph Tantivy Full-Text:\n  {node_ft_mg}\n")

    # -------------------------------------------------------------------------
    # 4. Compiling Vector Search Queries
    # -------------------------------------------------------------------------
    print("--- [3] Vector Search Queries ---")

    query_embedding = [0.025, -0.114, 0.871]  # 3D query vector matching index definition

    # Neo4j vector search procedure
    q_vec_neo4j = Query.vector_search(Document, query_embedding, k=5, dialect="cypher")
    compiled_vec_neo4j = q_vec_neo4j.compile()
    print("Neo4j Vector Search Query:")
    print(f"  Statement:  {compiled_vec_neo4j.statement}")
    print(f"  Parameters: {compiled_vec_neo4j.parameters}\n")

    # Memgraph vector search procedure
    q_vec_mg = Query.vector_search(Document, query_embedding, k=10, dialect="memgraph")
    compiled_vec_mg = q_vec_mg.compile()
    print("Memgraph Vector Search Query:")
    print(f"  Statement:  {compiled_vec_mg.statement}")
    print(f"  Parameters: {compiled_vec_mg.parameters}\n")

    # -------------------------------------------------------------------------
    # 5. Compiling Full-Text Search Queries
    # -------------------------------------------------------------------------
    print("--- [4] Full-Text BM25 Search Queries ---")

    # Neo4j node full-text search
    q_fts_neo4j = Query.fulltext_search(
        Document, "knowledge graph AND vector search", dialect="cypher"
    )
    compiled_fts_neo4j = q_fts_neo4j.compile()
    print("Neo4j Full-Text Search Query:")
    print(f"  Statement:  {compiled_fts_neo4j.statement}")
    print(f"  Parameters: {compiled_fts_neo4j.parameters}\n")

    # FalkorDB full-text search
    q_fts_falkor = Query.fulltext_search(Document, "graph algorithms", dialect="falkordb")
    compiled_fts_falkor = q_fts_falkor.compile()
    print("FalkorDB Full-Text Search Query:")
    print(f"  Statement:  {compiled_fts_falkor.statement}")
    print(f"  Parameters: {compiled_fts_falkor.parameters}\n")

    print("[SUCCESS] Vector & full-text search demonstration completed successfully.")


if __name__ == "__main__":
    main()
