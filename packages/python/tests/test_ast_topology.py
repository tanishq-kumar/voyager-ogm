"""Comprehensive tests for native AST topology extraction and ColumnMeta.

Verifies:
1. Native AST topology extraction on Query builder instances (zero regex).
2. Raw query string topology extraction across openCypher, ISO GQL, and SQL:2023 PGQ.
3. Resilience to comments (//, --, /* ... */), variable-length hops, and multi-hop paths.
4. CompiledQuery.columns with ColumnMeta(name, alias).
"""

from __future__ import annotations

from voyager_ogm import (
    ColumnMeta,
    CompiledQuery,
    Field,
    Node,
    Query,
    Relationship,
    node,
    relationship,
)
from voyager_ogm._voyager_rs import extract_topology_from_query
from voyager_ogm.viewer.extractor import (
    extract_graph_pattern_from_cypher,
    extract_path_topology_from_query,
)


@node(label="Person")
class Person(Node):
    name: str = Field(primary_key=True)
    age: int = Field(default=30)


@node(label="Movie")
class Movie(Node):
    title: str = Field(primary_key=True)
    released: int = Field(default=2000)


@relationship(type_name="ACTED_IN")
class ActedIn(Relationship):
    role: str = Field(default="Actor")


@relationship(type_name="DIRECTED")
class Directed(Relationship):
    pass


class TestNativeAstTopologyExtraction:
    def test_query_extract_topology_multi_hop(self):
        """Verifies direct Query.extract_topology() extraction from the native AST arena."""
        p = Person(alias="p")
        m = Movie(alias="m")
        act = ActedIn(alias="a")

        q = (
            Query.match(p)
            .where(p.name == "Keanu")
            .to(act)
            .node(m)
            .where(m.released == 1999)
            .return_(actor=p.name, movie=m.title)
        )

        topo = q.extract_topology()
        assert "nodes" in topo
        assert "edges" in topo

        nodes = topo["nodes"]
        edges = topo["edges"]

        assert len(nodes) == 2
        p_node = next(n for n in nodes if n["id"] == "p")
        m_node = next(n for n in nodes if n["id"] == "m")

        assert p_node["label"] == "Person"
        assert p_node["group"] == "Person"
        assert p_node["properties"].get("name") == "Keanu"

        assert m_node["label"] == "Movie"
        assert m_node["group"] == "Movie"
        assert m_node["properties"].get("released") == 1999

        assert len(edges) == 1
        edge = edges[0]
        assert edge["id"] == "a"
        assert edge["source"] == "p"
        assert edge["target"] == "m"
        assert edge["label"] == "ACTED_IN"
        assert edge["direction"] == "outgoing"

    def test_branching_and_reused_node_topology(self):
        """Verifies deduplication of node instances when patterns branch or traverse."""
        p = Person(alias="p")
        m1 = Movie(alias="m1")
        m2 = Movie(alias="m2")
        a1 = ActedIn(alias="a1")
        a2 = ActedIn(alias="a2")

        q = Query.match(p).to(a1).node(m1).match(p).to(a2).node(m2).return_(p.name)

        topo = q.extract_topology()
        assert len(topo["nodes"]) == 3  # p, m1, m2
        assert len(topo["edges"]) == 2  # a1, a2

        edge_sources = [e["source"] for e in topo["edges"]]
        assert edge_sources == ["p", "p"]


class TestRawQueryTopologyExtraction:
    def test_opencypher_with_comments_and_hops(self):
        """Verifies extraction of raw openCypher queries containing comments and hops."""
        query_text = """
        // Primary user lookup
        /* Multi-line
           block comment */
        MATCH (u:User {active: true})-[r:KNOWS*1..3]->(f:Friend)
        -- Trailing comment
        RETURN u, f
        """
        topo = extract_topology_from_query(query_text)
        assert len(topo["nodes"]) == 2
        assert len(topo["edges"]) == 1

        u_node = topo["nodes"][0]
        f_node = topo["nodes"][1]
        assert u_node["id"] == "u"
        assert u_node["label"] == "User"
        assert f_node["id"] == "f"
        assert f_node["label"] == "Friend"

        rel = topo["edges"][0]
        assert rel["source"] == "u"
        assert rel["target"] == "f"
        assert rel["label"] == "KNOWS"
        assert rel["min_hops"] == 1
        assert rel["max_hops"] == 3

    def test_iso_gql_syntax_topology(self):
        """Verifies extraction of ISO GQL style match path statements."""
        gql_text = "MATCH (a:Account) -[t:TRANSFERRED]-> (b:Account) RETURN a, b"
        nodes, edges = extract_path_topology_from_query(gql_text)
        assert len(nodes) == 2
        assert len(edges) == 1
        assert edges[0]["label"] == "TRANSFERRED"
        assert edges[0]["source"] == "a"
        assert edges[0]["target"] == "b"

    def test_sql_pgq_syntax_topology(self):
        """Verifies extraction of SQL:2023 PGQ MATCH path statements."""
        pgq_text = "SELECT * FROM GRAPH_TABLE (my_graph MATCH (x:Item)-[e:LINKS_TO]->(y:Item) COLUMNS (x.id, y.id))"
        nodes, edges = extract_graph_pattern_from_cypher(pgq_text)
        assert len(nodes) == 2
        assert len(edges) == 1
        assert edges[0]["label"] == "LINKS_TO"


class TestColumnMetaIntegration:
    def test_compiled_query_columns_metadata(self):
        """Verifies that Query.compile() populates ColumnMeta on CompiledQuery."""
        p = Person(alias="p")
        m = Movie(alias="m")
        act = ActedIn(alias="a")

        q = Query.match(p).to(act).node(m).return_(p.name, p.age, title_alias=m.title)

        compiled = q.compile("cypher")
        assert isinstance(compiled, CompiledQuery)
        assert hasattr(compiled, "columns")
        assert len(compiled.columns) == 3

        # First col: p.name (unaliased)
        assert compiled.columns[0].name == "p.name"
        assert compiled.columns[0].alias is None

        # Second col: p.age (unaliased)
        assert compiled.columns[1].name == "p.age"
        assert compiled.columns[1].alias is None

        # Third col: title_alias (aliased)
        assert compiled.columns[2].name == "title_alias"
        assert compiled.columns[2].alias == "title_alias"

    def test_compiled_query_multi_dialect_columns(self):
        """Verifies columns populated across Cypher, GQL, and SQL:PGQ emitters."""
        p = Person(alias="p")
        q = Query.match(p).return_(user_name=p.name)

        c_cypher = q.compile("cypher")
        c_gql = q.compile("iso_gql")
        c_pgq = q.compile("sql_pgq", graph_name="test_graph")

        assert len(c_cypher.columns) == 1
        assert c_cypher.columns[0] == ColumnMeta(name="user_name", alias="user_name")

        assert len(c_gql.columns) == 1
        assert c_gql.columns[0] == ColumnMeta(name="user_name", alias="user_name")

        assert len(c_pgq.columns) == 1
        assert c_pgq.columns[0] == ColumnMeta(name="user_name", alias="user_name")
