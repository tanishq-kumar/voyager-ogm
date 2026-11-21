"""Tests for creating and merging relationships between nodes already bound in a query."""

from __future__ import annotations

import socket

import pytest
from voyager_ogm import (
    Field,
    Query,
    Relationship,
    Session,
    create_bridge,
    fn,
    node,
    relationship,
)
from voyager_ogm.models import reset_alias_counters


@node(label="Feat192Person")
class Feat192Person:
    id: int = Field(primary_key=True)
    name: str


@relationship(type_name="FEAT192_KNOWS")
class Feat192Knows:
    since: int


class Feat192Incoming(Relationship, type_name="FEAT192_INCOMING", direction="incoming"):
    since: int


class Feat192Undirected(Relationship, type_name="FEAT192_UNDIRECTED", direction="undirected"):
    since: int


class Feat192Defaulted(Relationship, type_name="FEAT192_DEFAULTED"):
    since: int = 2024
    note: str = "default"


class Feat192Merge(Relationship, type_name="FEAT192_MERGE"):
    key: str
    note: str
    score: int = 5


class Feat192MergeKeyed(Relationship, type_name="FEAT192_MERGE_KEYED"):
    key: str = Field(primary_key=True)
    note: str
    score: int = 5


@node(label="Feat192LivePerson")
class Feat192LivePerson:
    id: int = Field(primary_key=True)
    name: str


@relationship(type_name="FEAT192_LIVE_KNOWS")
class Feat192LiveKnows:
    since: int


@relationship(type_name="FEAT192_LIVE_MERGE")
class Feat192LiveMerge:
    key: str = Field(primary_key=True)
    note: str


@pytest.fixture(autouse=True)
def _reset_aliases() -> None:
    reset_alias_counters()


def _people() -> tuple[Feat192Person, Feat192Person]:
    return Feat192Person(alias="a"), Feat192Person(alias="b")


def test_create_relationship_compiles_exact_statements_for_all_dialects() -> None:
    a, b = _people()
    query = (
        Query.match(a)
        .where(a.id == 1)
        .match(b)
        .where(b.id == 2)
        .create_relationship(a, Feat192Knows(since=2024), b)
    )

    cypher = query.compile("cypher")
    assert cypher.statement == (
        "MATCH (a:Feat192Person) WHERE a.id = $p0 "
        "MATCH (b:Feat192Person) WHERE b.id = $p1 "
        "CREATE (a)-[:FEAT192_KNOWS {since: $p2}]->(b)"
    )
    assert cypher.parameters == {"p0": 1, "p1": 2, "p2": 2024}

    gql = query.compile("iso_gql")
    assert gql.statement == (
        "MATCH (a:Feat192Person) WHERE a.id = $p0 "
        "MATCH (b:Feat192Person) WHERE b.id = $p1 "
        "INSERT (a)-[:FEAT192_KNOWS {since: $p2}]->(b)"
    )
    assert gql.parameters == cypher.parameters

    age = query.compile("age", graph_name="feat192_graph")
    assert age.statement == (
        "SELECT * FROM cypher('feat192_graph', $$ "
        "MATCH (a:Feat192Person) WHERE a.id = $p0 "
        "MATCH (b:Feat192Person) WHERE b.id = $p1 "
        "CREATE (a)-[:FEAT192_KNOWS {since: $p2}]->(b)"
        " $$, %s) AS (result agtype)"
    )
    assert age.parameters == cypher.parameters


def test_create_relationship_accepts_alias_strings_and_optional_variable() -> None:
    query = (
        Query.match("a", labels="Feat192Person")
        .match("b", labels="Feat192Person")
        .create_relationship(
            "a",
            Feat192Knows(alias="ignored", since=2024),
            "b",
            variable="r",
        )
        .return_()
    )

    compiled = query.compile("cypher")
    assert compiled.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        "CREATE (a)-[r:FEAT192_KNOWS {since: $p0}]->(b)"
    )
    assert compiled.parameters == {"p0": 2024}


def test_create_relationship_uses_explicit_instance_alias() -> None:
    a, b = _people()
    rel = Feat192Knows(alias="r", since=2024)

    compiled = (
        Query.match(a).match(b).create_relationship(a, rel, b).return_(rel.since).compile("cypher")
    )
    assert compiled.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        "CREATE (a)-[r:FEAT192_KNOWS {since: $p0}]->(b) RETURN r.since"
    )


def test_create_relationship_respects_direction() -> None:
    a, b = _people()

    incoming = (
        Query.match(a)
        .match(b)
        .create_relationship(a, Feat192Incoming(since=2024), b)
        .compile("cypher")
    )
    assert incoming.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        "CREATE (a)<-[:FEAT192_INCOMING {since: $p0}]-(b)"
    )

    undirected = (
        Query.match(a)
        .match(b)
        .create_relationship(a, Feat192Undirected(since=2024), b)
        .compile("cypher")
    )
    assert undirected.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        "CREATE (a)-[:FEAT192_UNDIRECTED {since: $p0}]->(b)"
    )


def test_create_relationship_writes_relationship_defaults() -> None:
    a, b = _people()
    rel = Feat192Defaulted()
    assert rel.get("since") == 2024
    assert rel.get("note") == "default"

    compiled = Query.match(a).match(b).create_relationship(a, rel, b).compile("cypher")
    assert compiled.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        "CREATE (a)-[:FEAT192_DEFAULTED {since: $p0, note: $p1}]->(b)"
    )
    assert compiled.parameters == {"p0": 2024, "p1": "default"}


def test_create_relationship_rejects_unbound_alias() -> None:
    a, _ = _people()

    with pytest.raises(ValueError, match="ghost"):
        Query.match(a).create_relationship(a, Feat192Knows(since=2024), "ghost")

    with pytest.raises(ValueError, match="missing"):
        Query.match(a).create_relationship("missing", Feat192Knows(since=2024), a)


def test_merge_relationship_uses_key_policy_and_sets() -> None:
    a, b = _people()
    query = (
        Query.match(a)
        .match(b)
        .merge_relationship(
            a,
            Feat192Merge(alias="r", key="k1", note="n"),
            b,
            variable="r",
        )
    )

    cypher = query.compile("cypher")
    assert cypher.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        "MERGE (a)-[r:FEAT192_MERGE {key: $p0, note: $p1}]->(b) "
        "ON CREATE SET r.score = $p2"
    )
    assert cypher.parameters == {"p0": "k1", "p1": "n", "p2": 5}

    with pytest.raises(ValueError, match="ON CREATE SET / ON MATCH SET"):
        query.compile("iso_gql")


def test_merge_relationship_without_variable_uses_instance_alias() -> None:
    a, b = _people()
    rel = Feat192MergeKeyed(key="k1", note="n")

    compiled = Query.match(a).match(b).merge_relationship(a, rel, b).compile("cypher")
    assert compiled.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        f"MERGE (a)-[{rel.alias}:FEAT192_MERGE_KEYED {{key: $p0}}]->(b) "
        f"ON CREATE SET {rel.alias}.note = $p1, {rel.alias}.score = $p2 "
        f"ON MATCH SET {rel.alias}.note = $p1"
    )
    assert rel.alias == "_feat192_merge_keyed_0"
    assert compiled.parameters == {"p0": "k1", "p1": "n", "p2": 5}


def test_merge_relationship_key_only_omits_auto_alias() -> None:
    a, b = _people()

    compiled = (
        Query.match(a).match(b).merge_relationship(a, Feat192Knows(since=2024), b).compile("cypher")
    )
    assert compiled.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        "MERGE (a)-[:FEAT192_KNOWS {since: $p0}]->(b)"
    )


def test_merge_relationship_uses_explicit_instance_alias_without_variable() -> None:
    a, b = _people()
    rel = Feat192MergeKeyed(alias="r", key="k1", note="n")

    compiled = Query.match(a).match(b).merge_relationship(a, rel, b).compile("cypher")
    assert compiled.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) "
        "MERGE (a)-[r:FEAT192_MERGE_KEYED {key: $p0}]->(b) "
        "ON CREATE SET r.note = $p1, r.score = $p2 "
        "ON MATCH SET r.note = $p1"
    )


def test_merge_relationship_without_variable_for_class_and_type_string() -> None:
    a, b = _people()

    from_class = (
        Query.match(a).match(b).merge_relationship(a, Feat192MergeKeyed, b).compile("cypher")
    )
    assert from_class.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) MERGE (a)-[:FEAT192_MERGE_KEYED]->(b)"
    )

    from_type = Query.match(a).match(b).merge_relationship(a, "FEAT192_CUSTOM", b).compile("cypher")
    assert from_type.statement == (
        "MATCH (a:Feat192Person) MATCH (b:Feat192Person) MERGE (a)-[:FEAT192_CUSTOM]->(b)"
    )


def test_merge_relationship_generates_alias_for_class_and_type_string_sets() -> None:
    assert (
        Query._relationship_set_variable(Feat192MergeKeyed, "FEAT192_MERGE_KEYED", None)
        == "_feat192_merge_keyed_0"
    )
    assert (
        Query._relationship_set_variable("FEAT192_CUSTOM", "FEAT192_CUSTOM", None)
        == "_feat192_custom_0"
    )


def _is_port_open(port: int, host: str = "127.0.0.1") -> bool:
    try:
        with socket.create_connection((host, port), timeout=0.5):
            return True
    except OSError:
        return False


def _exercise_create_relationship(session: Session, since: int = 2024) -> None:
    a = Feat192LivePerson(alias="a", id=192_001, name="Alice")
    b = Feat192LivePerson(alias="b", id=192_002, name="Bob")

    cleaner = Feat192LivePerson(alias="n")
    session.execute(Query.match(cleaner).detach_delete(cleaner))

    session.execute(Query.create(a))
    session.execute(Query.create(b))

    relationship_instance = Feat192LiveKnows(since=since)
    session.execute(
        Query.match(a)
        .where(a.id == 192_001)
        .match(b)
        .where(b.id == 192_002)
        .create_relationship(a, relationship_instance, b)
    )

    rel = Feat192LiveKnows(alias="r")
    rows = session.execute(
        Query.match(a)
        .where(a.id == 192_001)
        .to(rel)
        .node(b)
        .where(b.id == 192_002)
        .return_(
            source=a.id,
            target=b.id,
            rel_type=fn.type_(rel),
            since=rel.since,
        )
    )

    assert len(rows) == 1
    row = rows[0]
    assert str(row["source"]).startswith("192001")
    assert str(row["target"]).startswith("192002")
    assert "FEAT192_LIVE_KNOWS" in str(row["rel_type"])
    assert str(row["since"]).startswith(str(since))

    session.execute(Query.match(cleaner).detach_delete(cleaner))


def _exercise_merge_relationship_no_variable(session: Session) -> None:
    a = Feat192LivePerson(alias="a", id=192_011, name="Alice")
    b = Feat192LivePerson(alias="b", id=192_012, name="Bob")

    cleaner = Feat192LivePerson(alias="n")
    session.execute(Query.match(cleaner).detach_delete(cleaner))

    session.execute(Query.create(a))
    session.execute(Query.create(b))

    first = Feat192LiveMerge(key="k1", note="first")
    session.execute(
        Query.match(a)
        .where(a.id == 192_011)
        .match(b)
        .where(b.id == 192_012)
        .merge_relationship(a, first, b)
    )

    second = Feat192LiveMerge(key="k1", note="second")
    session.execute(
        Query.match(a)
        .where(a.id == 192_011)
        .match(b)
        .where(b.id == 192_012)
        .merge_relationship(a, second, b)
    )

    rel = Feat192LiveMerge(alias="r")
    rows = session.execute(
        Query.match(a)
        .where(a.id == 192_011)
        .to(rel)
        .node(b)
        .where(b.id == 192_012)
        .return_(note=rel.note)
    )

    assert len(rows) == 1
    assert rows[0]["note"] == "second"

    session.execute(Query.match(cleaner).detach_delete(cleaner))


@pytest.mark.live
def test_live_neo4j_create_relationship() -> None:
    if not _is_port_open(7687):
        pytest.skip("Neo4j container not reachable on port 7687")
    from neo4j import GraphDatabase

    driver = GraphDatabase.driver(
        "bolt://127.0.0.1:7687",
        auth=("neo4j", "voyagerpass123"),
        connection_timeout=2.0,
    )
    try:
        driver.verify_connectivity()
    except Exception as exc:  # pragma: no cover - environment dependent
        driver.close()
        pytest.skip(f"Neo4j container not reachable on port 7687: {exc}")

    session = Session(bridge=driver, dialect="cypher")
    try:
        _exercise_create_relationship(session)
        _exercise_merge_relationship_no_variable(session)
    finally:
        session.close()
        driver.close()


@pytest.mark.live
def test_live_memgraph_create_relationship() -> None:
    if not _is_port_open(7688):
        pytest.skip("Memgraph container not reachable on port 7688")
    from neo4j import GraphDatabase

    driver = GraphDatabase.driver(
        "bolt://127.0.0.1:7688",
        auth=("", ""),
        connection_timeout=2.0,
    )
    try:
        driver.verify_connectivity()
    except Exception as exc:  # pragma: no cover - environment dependent
        driver.close()
        pytest.skip(f"Memgraph container not reachable on port 7688: {exc}")

    session = Session(bridge=driver, dialect="cypher")
    try:
        _exercise_create_relationship(session)
    finally:
        session.close()
        driver.close()


@pytest.mark.live
def test_live_falkordb_create_relationship() -> None:
    if not _is_port_open(6379):
        pytest.skip("FalkorDB container not reachable on port 6379")
    try:
        from falkordb import FalkorDB
    except ImportError:
        pytest.skip("falkordb package is required for live FalkorDB tests")

    client = FalkorDB(host="127.0.0.1", port=6379)
    graph = client.select_graph("feat192_live")
    session = Session(bridge=create_bridge(graph), dialect="cypher")
    try:
        _exercise_create_relationship(session)
    finally:
        session.close()
        try:
            graph.delete()
        except Exception:
            pass


@pytest.mark.live
def test_live_age_create_relationship() -> None:
    if not _is_port_open(5455):
        pytest.skip("Apache AGE container not reachable on port 5455")
    try:
        import psycopg
    except ImportError:
        pytest.skip("psycopg is required for live Apache AGE tests")

    uri = "postgresql://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph"
    graph_name = "feat192_live_graph"
    try:
        raw_conn = psycopg.connect(uri, autocommit=True, connect_timeout=2)
    except Exception as exc:  # pragma: no cover - environment dependent
        pytest.skip(f"Apache AGE container not reachable on port 5455: {exc}")

    created_graph = False
    session: Session | None = None
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

        bridge = create_bridge(uri, graph_name=graph_name)
        session = Session(bridge=bridge, dialect="age", graph_name=graph_name)
        _exercise_create_relationship(session)
    finally:
        if session is not None:
            session.close()
        try:
            if created_graph:
                with raw_conn.cursor() as cur:
                    cur.execute("SELECT ag_catalog.drop_graph(%s, true);", (graph_name,))
            raw_conn.close()
        except Exception:
            pass
