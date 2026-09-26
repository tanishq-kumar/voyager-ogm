"""Unit & Live Integration Tests for NativeSchemaRegistry and Model Metadata Storage."""

from __future__ import annotations

import json
import socket

import pytest
from neo4j import GraphDatabase
from voyager_ogm import (
    Field,
    NativeClient,
    NativeSchemaRegistry,
    Node,
    Query,
    Relationship,
    SchemaRegistry,
    Session,
    node,
    relationship,
)


def _is_port_open(host: str, port: int, timeout: float = 0.3) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


# ---------------------------------------------------------------------------
# Unit Tests (No Database Required)
# ---------------------------------------------------------------------------


def test_registry_instantiation_and_isolation() -> None:
    """Verifies that individual instances of NativeSchemaRegistry are isolated."""
    assert SchemaRegistry is NativeSchemaRegistry

    reg1 = NativeSchemaRegistry()
    reg2 = NativeSchemaRegistry()

    assert len(reg1) == 0
    assert len(reg2) == 0
    assert "NativeSchemaRegistry(nodes=0, relationships=0)" in repr(reg1)

    reg1.register_node(
        "IsolatedUser", ["IsolatedUser"], {"id": {"type": "STRING", "primary_key": True}}
    )

    assert len(reg1) == 1
    assert reg1.has_node("IsolatedUser")
    assert not reg2.has_node("IsolatedUser")
    assert len(reg2) == 0


def test_global_registry_singleton() -> None:
    """Verifies that global_registry() and global_() access the shared singleton."""
    g1 = NativeSchemaRegistry.global_registry()
    g2 = NativeSchemaRegistry.global_()

    # Clear any previous test artifacts
    g1.clear()
    assert len(g1) == 0
    assert len(g2) == 0

    g1.register_node("GlobalItem", ["GlobalItem"], {"id": {"type": "STRING", "primary_key": True}})

    assert g2.has_node("GlobalItem")
    assert len(g2) == 1

    # Cleanup
    g1.clear()
    assert len(g1) == 0


def test_register_node_dict_and_field_descriptors() -> None:
    """Verifies node registration using dictionary specifications and inspects all properties."""
    reg = NativeSchemaRegistry()

    fields_spec = {
        "id": {"type": "STRING", "primary_key": True},
        "email": {"type": "STRING", "unique": True, "nullable": False},
        "age": {"type": "INTEGER", "indexed": True, "index_type": "BTREE"},
        "bio": {"type": "STRING", "indexed": True, "index_type": "TEXT"},
        "active": {"type": "BOOLEAN", "nullable": False, "default_value": True},
    }

    reg.register_node("User", ["User", "Account"], fields_spec)

    assert reg.has_node("User")
    node_meta = reg.get_node("User")
    assert node_meta is not None
    assert node_meta["name"] == "User"
    assert node_meta["labels"] == ["User", "Account"]
    assert node_meta["primary_key"] == "id"

    # Lookup by secondary label
    by_sec_label = reg.get_node_by_label("Account")
    assert by_sec_label is not None
    assert by_sec_label["name"] == "User"

    # Case-insensitive label lookup
    by_case = reg.get_node_by_label("account")
    assert by_case is not None
    assert by_case["name"] == "User"

    # Field inspections
    fields = node_meta["fields"]
    assert len(fields) == 5

    assert fields["id"]["primary_key"] is True
    assert fields["id"]["unique"] is True
    assert fields["id"]["nullable"] is False
    assert fields["id"]["type"] == "STRING"

    assert fields["email"]["unique"] is True
    assert fields["email"]["nullable"] is False
    assert fields["email"]["primary_key"] is False

    assert fields["age"]["indexed"] is True
    assert fields["age"]["index_type"] == "BTREE"
    assert fields["age"]["type"] == "INTEGER"

    assert fields["bio"]["indexed"] is True
    assert fields["bio"]["index_type"] == "TEXT"

    assert fields["active"]["nullable"] is False
    assert fields["active"]["default_value"] is True


def test_register_node_with_models_field() -> None:
    """Verifies that voyager_ogm.models.Field descriptors register seamlessly."""
    reg = NativeSchemaRegistry()

    fields = {
        "user_id": Field(primary_key=True, type_annotation=str),
        "username": Field(unique=True, type_annotation=str),
        "score": Field(index=True, type_annotation=int),
        "rating": Field(type_annotation=float),
        "is_verified": Field(type_annotation=bool),
        "tags": Field(type_annotation=list),
        "meta": Field(type_annotation=dict),
    }

    reg.register_node("Player", ["Player"], fields)

    meta = reg.get_node("Player")
    assert meta is not None
    f = meta["fields"]

    assert f["user_id"]["primary_key"] is True
    assert f["user_id"]["unique"] is True
    assert f["user_id"]["type"] == "STRING"

    assert f["username"]["unique"] is True
    assert f["username"]["type"] == "STRING"

    assert f["score"]["indexed"] is True
    assert f["score"]["index_type"] == "BTREE"
    assert f["score"]["type"] == "INTEGER"

    assert f["rating"]["type"] == "FLOAT"
    assert f["is_verified"]["type"] == "BOOLEAN"
    assert f["tags"]["type"] == "LIST"
    assert f["meta"]["type"] == "MAP"


def test_register_relationship_and_endpoints() -> None:
    """Verifies relationship registration, endpoint constraints, and case-insensitive type lookups."""
    reg = NativeSchemaRegistry()

    rel_fields = {
        "since": {"type": "INTEGER", "nullable": False},
        "role": {"type": "STRING", "indexed": True},
    }

    reg.register_relationship(
        name="WorksAt",
        type_name="WORKS_AT",
        source_labels=["Person"],
        target_labels=["Company"],
        fields=rel_fields,
        directed=True,
    )

    assert reg.has_relationship("WorksAt")
    rel_meta = reg.get_relationship("WorksAt")
    assert rel_meta is not None
    assert rel_meta["name"] == "WorksAt"
    assert rel_meta["type_name"] == "WORKS_AT"
    assert rel_meta["source_labels"] == ["Person"]
    assert rel_meta["target_labels"] == ["Company"]
    assert rel_meta["directed"] is True
    assert rel_meta["fields"]["since"]["type"] == "INTEGER"
    assert rel_meta["fields"]["role"]["indexed"] is True

    # Case-insensitive lookup by graph relationship type
    by_type = reg.get_relationship_by_type("works_at")
    assert by_type is not None
    assert by_type["name"] == "WorksAt"

    by_type_upper = reg.get_relationship_by_type("WORKS_AT")
    assert by_type_upper is not None
    assert by_type_upper["name"] == "WorksAt"


def test_register_via_schema_dict() -> None:
    """Verifies registering entities directly via schema dictionary specifications."""
    reg = NativeSchemaRegistry()

    reg.register_node_schema(
        {
            "name": "Document",
            "labels": ["Document", "File"],
            "primary_key": "doc_id",
            "fields": {
                "doc_id": {"type": "STRING", "primary_key": True},
                "content": {"type": "STRING", "indexed": True, "index_type": "TEXT"},
            },
        }
    )

    assert reg.has_node("Document")
    doc = reg.get_node("Document")
    assert doc is not None
    assert doc["primary_key"] == "doc_id"
    assert doc["fields"]["content"]["index_type"] == "TEXT"

    reg.register_relationship_schema(
        {
            "name": "References",
            "type_name": "REFERENCES",
            "source_labels": ["Document"],
            "target_labels": ["Document"],
            "directed": True,
            "fields": {
                "weight": {"type": "FLOAT"},
            },
        }
    )

    assert reg.has_relationship("References")
    assert reg.get_relationship("References") is not None


def test_schema_registry_removal_and_clear() -> None:
    """Verifies removing single schemas and clearing the whole registry."""
    reg = NativeSchemaRegistry()

    reg.register_node("NodeA", ["NodeA"], {"id": {"type": "STRING"}})
    reg.register_node("NodeB", ["NodeB"], {"id": {"type": "STRING"}})
    reg.register_relationship("RelA", "REL_A")

    assert len(reg) == 3
    assert len(reg.node_schemas()) == 2
    assert len(reg.relationship_schemas()) == 1

    removed_node = reg.remove_node("NodeA")
    assert removed_node is not None
    assert removed_node["name"] == "NodeA"
    assert not reg.has_node("NodeA")
    assert len(reg) == 2

    removed_rel = reg.remove_relationship("RelA")
    assert removed_rel is not None
    assert removed_rel["name"] == "RelA"
    assert not reg.has_relationship("RelA")
    assert len(reg) == 1

    reg.clear()
    assert len(reg) == 0
    assert len(reg.node_schemas()) == 0
    assert len(reg.relationship_schemas()) == 0


def test_json_roundtrip_snapshot() -> None:
    """Verifies that full registry state serializes to JSON and restores accurately."""
    reg = NativeSchemaRegistry()

    reg.register_node(
        "Article",
        ["Article", "Post"],
        {
            "id": {"type": "STRING", "primary_key": True},
            "views": {"type": "INTEGER", "indexed": True},
        },
    )
    reg.register_relationship(
        "Authored",
        "AUTHORED",
        source_labels=["Author"],
        target_labels=["Article"],
        fields={"year": {"type": "INTEGER"}},
    )

    json_str = reg.to_json()
    assert "Article" in json_str
    assert "AUTHORED" in json_str

    parsed = json.loads(json_str)
    assert "nodes" in parsed
    assert "Article" in parsed["nodes"]
    assert "relationships" in parsed
    assert "Authored" in parsed["relationships"]

    # Restore in new registry
    reg2 = NativeSchemaRegistry()
    assert len(reg2) == 0
    reg2.from_json(json_str)

    assert len(reg2) == 2
    assert reg2.has_node("Article")
    assert reg2.has_relationship("Authored")

    article = reg2.get_node("Article")
    assert article is not None
    assert article["primary_key"] == "id"
    assert article["fields"]["views"]["indexed"] is True


def test_error_handling() -> None:
    """Verifies proper error handling for missing keys or invalid structures."""
    reg = NativeSchemaRegistry()

    # Missing name in schema dict
    with pytest.raises(ValueError, match="Missing required key 'name'"):
        reg.register_node_schema({"labels": ["User"]})

    with pytest.raises(ValueError, match="Missing required key 'name'"):
        reg.register_relationship_schema({"fields": {}})

    # Invalid fields argument type
    with pytest.raises(TypeError, match="fields must be a dict or list"):
        reg.register_node("Invalid", ["Invalid"], 12345)  # type: ignore[arg-type]


# ---------------------------------------------------------------------------
# Live Integration Tests against Neo4j
# ---------------------------------------------------------------------------


@node(label="LiveSchemaPerson")
class LiveSchemaPerson(Node):
    person_id: str = Field(primary_key=True)
    name: str = Field(index=True)
    age: int = Field()


@node(label="LiveSchemaProject")
class LiveSchemaProject(Node):
    project_id: str = Field(primary_key=True)
    title: str = Field()


@relationship(type_name="LIVE_CONTRIBUTED")
class LiveContributed(Relationship):
    role: str = Field()
    commits: int = Field()


@pytest.mark.live
def test_live_schema_registry_with_neo4j() -> None:
    """Verifies NativeSchemaRegistry metadata synchronization and query execution on live Neo4j."""
    if not _is_port_open("127.0.0.1", 7687):
        pytest.skip("Neo4j database not reachable on port 7687")

    uri = "bolt://127.0.0.1:7687"
    auth = ("neo4j", "voyagerpass123")

    try:
        client = NativeClient(
            f"bolt://{auth[0]}:{auth[1]}@127.0.0.1:7687?connect_timeout=2",
            min_idle=1,
            max_size=2,
        )
        client.ping_sync()
        client.close()
        driver = GraphDatabase.driver(uri, auth=auth, connection_timeout=2.0)
        driver.verify_connectivity()
    except Exception as e:
        pytest.skip(f"Neo4j container not available on port 7687: {e}")

    session = Session(bridge=driver, dialect="cypher")

    # 1. Register schemas in NativeSchemaRegistry
    registry = NativeSchemaRegistry()

    registry.register_node(
        name="LiveSchemaPerson",
        labels=["LiveSchemaPerson"],
        fields={
            "person_id": Field(primary_key=True, type_annotation=str),
            "name": Field(index=True, type_annotation=str),
            "age": Field(type_annotation=int),
        },
    )

    registry.register_node(
        name="LiveSchemaProject",
        labels=["LiveSchemaProject"],
        fields={
            "project_id": Field(primary_key=True, type_annotation=str),
            "title": Field(type_annotation=str),
        },
    )

    registry.register_relationship(
        name="LiveContributed",
        type_name="LIVE_CONTRIBUTED",
        source_labels=["LiveSchemaPerson"],
        target_labels=["LiveSchemaProject"],
        fields={
            "role": Field(type_annotation=str),
            "commits": Field(type_annotation=int),
        },
        directed=True,
    )

    assert registry.has_node("LiveSchemaPerson")
    assert registry.has_node("LiveSchemaProject")
    assert registry.has_relationship("LiveContributed")

    # 2. Clean up previous test artifacts using QueryBuilder (no raw query strings!)
    p_clean = LiveSchemaPerson(alias="p")
    clean_p_query = Query.match(p_clean).detach_delete(p_clean)
    session.execute(clean_p_query)

    proj_clean = LiveSchemaProject(alias="proj")
    clean_proj_query = Query.match(proj_clean).detach_delete(proj_clean)
    session.execute(clean_proj_query)

    try:
        # 3. Seed entities using Query.create(...) via models (no raw queries!)
        p1 = LiveSchemaPerson(alias="p")
        proj1 = LiveSchemaProject(alias="proj")
        rel = LiveContributed(alias="r")

        create_query = (
            Query.create(p1)
            .set(p1.person_id == "p_42", p1.name == "Ada Lovelace", p1.age == 36)
            .to(rel)
            .set(rel.role == "Architect", rel.commits == 150)
            .node(proj1)
            .set(proj1.project_id == "proj_101", proj1.title == "Analytical Engine")
        )
        session.execute(create_query)

        # 4. Query back using Query.match(...) and assert consistency with registry metadata
        match_query = (
            Query.match(p1)
            .to(rel)
            .node(proj1)
            .where(p1.person_id == "p_42")
            .return_(p1.name, p1.age, proj1.title, rel.role, rel.commits)
        )
        result = session.execute(match_query)
        rows = result.mappings().all()

        assert len(rows) == 1
        row = rows[0]
        assert row["p.name"] == "Ada Lovelace"
        assert row["p.age"] == 36
        assert row["proj.title"] == "Analytical Engine"
        assert row["r.role"] == "Architect"
        assert row["r.commits"] == 150

        # Assert returned field names match the registered schema metadata
        person_schema = registry.get_node("LiveSchemaPerson")
        assert person_schema is not None
        assert "person_id" in person_schema["fields"]
        assert "name" in person_schema["fields"]
        assert "age" in person_schema["fields"]

        rel_schema = registry.get_relationship("LiveContributed")
        assert rel_schema is not None
        assert "role" in rel_schema["fields"]
        assert "commits" in rel_schema["fields"]

    finally:
        # Clean up test nodes with QueryBuilder
        session.execute(Query.match(p_clean).detach_delete(p_clean))
        session.execute(Query.match(proj_clean).detach_delete(proj_clean))
        driver.close()
