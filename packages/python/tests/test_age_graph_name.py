"""Offline unit tests verifying Apache AGE graph_name configuration and validation.

Verifies:
- Session(dialect="age", graph_name="g2") compiles match/create/merge to cypher('g2', ...)
- Default graph name remains "age_graph" for backwards compatibility
- Invalid graph names (SQL injection attempts, whitespace, invalid chars, empty strings) raise ValueError
- AsyncSession behaves identically
- Single source of truth: Session.graph_name takes precedence over bridge / URI graph name
- Bulk ingestion plans, Active Record save(), and transactions honor session graph_name
"""

import pytest
from voyager_ogm import (
    AsyncSession,
    Field,
    FullTextIndex,
    MockBridge,
    Node,
    Query,
    Relationship,
    Session,
    create_bridge,
    validate_graph_name,
)


class User(Node):
    __label__ = "User"
    id: int = Field(primary_key=True)
    name: str
    age: int = 30


class Knows(Relationship):
    __type__ = "KNOWS"
    since: int = 2020


def test_validate_graph_name() -> None:
    """Verifies that validate_graph_name accepts safe identifiers and rejects injection attempts."""
    # Valid identifiers
    assert validate_graph_name("age_graph") == "age_graph"
    assert validate_graph_name("production_graph") == "production_graph"
    assert validate_graph_name("g2") == "g2"
    assert validate_graph_name("_private_graph") == "_private_graph"
    assert validate_graph_name("Graph_123") == "Graph_123"
    assert validate_graph_name("a" * 63) == "a" * 63

    # Invalid identifiers
    invalid_cases = [
        "",
        "x'); DROP TABLE users; --",
        "a b",
        "a-b",
        "123abc",
        "graph.name",
        "graph;name",
        'g"2',
        "g'2",
        "a" * 64,  # Over 63 chars limit
        None,  # type: ignore[arg-type]
        123,  # type: ignore[arg-type]
    ]
    for invalid in invalid_cases:
        with pytest.raises(ValueError):
            validate_graph_name(invalid)  # type: ignore[arg-type]


def test_session_age_default_graph_name() -> None:
    """Verifies that Session(dialect='age') defaults to 'age_graph'."""
    session = Session(dialect="age")
    assert session.graph_name == "age_graph"

    u = User("u")
    q_match = Query.match(u).where(u.name == "Alice").return_(u.name)
    res = session.execute(q_match)
    assert "cypher('age_graph'," in res.statement

    q_create = Query.create(u).return_(u.name)
    res_create = session.execute(q_create)
    assert "cypher('age_graph'," in res_create.statement

    q_merge = Query.merge(u).return_(u.name)
    res_merge = session.execute(q_merge)
    assert "cypher('age_graph'," in res_merge.statement


def test_session_age_custom_graph_name() -> None:
    """Verifies that Session(dialect='age', graph_name='g2') targets 'g2'."""
    session = Session(dialect="age", graph_name="g2")
    assert session.graph_name == "g2"

    u = User("u")
    # MATCH
    q_match = Query.match(u).where(u.name == "Alice").return_(u.name)
    res_match = session.execute(q_match)
    assert "cypher('g2'," in res_match.statement
    assert "cypher('age_graph'," not in res_match.statement

    # CREATE
    q_create = Query.create(u).return_(u.name)
    res_create = session.execute(q_create)
    assert "cypher('g2'," in res_create.statement
    assert "cypher('age_graph'," not in res_create.statement

    # MERGE
    q_merge = Query.merge(u).return_(u.name)
    res_merge = session.execute(q_merge)
    assert "cypher('g2'," in res_merge.statement
    assert "cypher('age_graph'," not in res_merge.statement


def test_session_age_invalid_graph_name_raises() -> None:
    """Verifies that passing invalid graph names to Session raises ValueError."""
    for invalid in ["x'); DROP", "", "a b", "123abc", "a-b"]:
        with pytest.raises(ValueError):
            Session(dialect="age", graph_name=invalid)


@pytest.mark.asyncio
async def test_async_session_age_custom_graph_name() -> None:
    """Verifies that AsyncSession supports custom and default graph names and validates them."""
    # Default
    async_default = AsyncSession(dialect="age")
    assert async_default.graph_name == "age_graph"
    u = User("u")
    res_def = await async_default.execute(Query.match(u).return_(u.name))
    assert "cypher('age_graph'," in res_def.statement

    # Custom
    async_custom = AsyncSession(dialect="age", graph_name="analytics_graph")
    assert async_custom.graph_name == "analytics_graph"
    res_cust = await async_custom.execute(Query.match(u).return_(u.name))
    assert "cypher('analytics_graph'," in res_cust.statement

    # Invalid
    with pytest.raises(ValueError):
        AsyncSession(dialect="age", graph_name="bad-name")


def test_query_compile_graph_name_validation() -> None:
    """Verifies that Query.compile validates graph_name and raises ValueError on invalid identifiers."""
    u = User("u")
    q = Query.match(u).return_(u.name)

    compiled = q.compile(dialect="age", graph_name="custom_graph")
    assert "cypher('custom_graph'," in compiled.statement

    with pytest.raises(ValueError):
        q.compile(dialect="age", graph_name="x'); DROP TABLE users; --")

    with pytest.raises(ValueError):
        q.compile(dialect="age", graph_name="invalid name")


def test_precedence_and_single_source_of_truth() -> None:
    """Verifies that Session graph_name overrides bridge / URI graph_name."""
    # 1. Bridge has graph_name, session omits: session adopts bridge's graph_name
    bridge = MockBridge(graph_name="bridge_graph")
    s1 = Session(bridge=bridge, dialect="age")
    assert s1.graph_name == "bridge_graph"

    u = User("u")
    res1 = s1.execute(Query.match(u).return_(u.name))
    assert "cypher('bridge_graph'," in res1.statement

    # 2. Bridge has graph_name, session explicitly overrides: session wins
    s2 = Session(bridge=bridge, dialect="age", graph_name="session_override")
    assert s2.graph_name == "session_override"
    res2 = s2.execute(Query.match(u).return_(u.name))
    assert "cypher('session_override'," in res2.statement
    assert "cypher('bridge_graph'," not in res2.statement

    # 3. URI query parameter parsing via create_bridge
    uri_bridge = create_bridge("mock://?graph=uri_graph")
    assert getattr(uri_bridge, "graph_name", None) == "uri_graph"
    s3 = Session(bridge=uri_bridge, dialect="age")
    assert s3.graph_name == "uri_graph"


def test_bulk_operations_honor_session_graph_name() -> None:
    """Verifies that bulk_create, bulk_upsert, and bulk_create_relationships use session.graph_name."""
    session = Session(dialect="age", graph_name="tenant_42")

    plan_create = session.bulk_create(User, [{"id": 1, "name": "Alice"}])
    assert "cypher('tenant_42'," in plan_create.statement

    plan_upsert = session.bulk_upsert(User, [{"id": 1, "name": "Alice"}], key_field="id")
    assert "cypher('tenant_42'," in plan_upsert.statement

    plan_rel = session.bulk_create_relationships(
        Knows,
        [{"from_id": 1, "to_id": 2, "since": 2024}],
        from_label="User",
        from_key="id",
        to_label="User",
        to_key="id",
    )
    assert "cypher('tenant_42'," in plan_rel.statement


def test_session_transaction_write_honors_graph_name() -> None:
    """Verifies that writes inside session.transaction() target session.graph_name."""
    mock_bridge = MockBridge()
    session = Session(bridge=mock_bridge, dialect="age", graph_name="tx_graph")

    u = User("u")
    with session.transaction():
        session.execute(Query.create(u).return_(u.name))

    executed = mock_bridge.executed_queries
    assert len(executed) > 0
    tx_statement = executed[0][0]
    assert "cypher('tx_graph'," in tx_statement


def test_active_record_save_and_flush_honors_graph_name() -> None:
    """Verifies that identity map flush and active record .save() target session.graph_name."""
    session = Session(dialect="age", graph_name="flush_graph", enable_identity_map=True)

    user = User(id=1, name="Bob")
    session.register(user, is_clean=True)
    user.name = "Robert"

    batches = session._prepare_flush_batches()
    assert len(batches) == 1
    _, _, plan = batches[0]
    assert "cypher('flush_graph'," in plan.statement


class Article(Node):
    __label__ = "Article"
    id: int = Field(primary_key=True)
    title: str = Field(index=True)
    body: str

    __fulltext_indexes__ = [
        FullTextIndex(properties=["title", "body"], name="article_ft_idx", analyzer="english"),
    ]


def test_session_create_indexes_targets_graph_schema() -> None:
    """Verifies that session.create_indexes() targets the session's graph schema in Apache AGE."""
    # Default graph -> "age_graph"
    session_default = Session(dialect="age")
    stmts_default = session_default.create_indexes(Article)
    assert len(stmts_default) == 1
    assert 'ON "age_graph"."Article"' in stmts_default[0]
    assert "(properties->>'title'::text) || ' ' || (properties->>'body'::text)" in stmts_default[0]

    # Custom graph -> "production_graph"
    session_custom = Session(dialect="age", graph_name="production_graph")
    stmts_custom = session_custom.create_indexes(Article)
    assert len(stmts_custom) == 1
    assert 'ON "production_graph"."Article"' in stmts_custom[0]
    assert "(properties->>'title'::text) || ' ' || (properties->>'body'::text)" in stmts_custom[0]

    # Explicit graph_name argument overrides session
    stmts_override = session_default.create_indexes(Article, graph_name="tenant_delta")
    assert len(stmts_override) == 1
    assert 'ON "tenant_delta"."Article"' in stmts_override[0]

    # Invalid graph name rejected with ValueError
    with pytest.raises(ValueError):
        session_default.create_indexes(Article, graph_name="invalid; DROP")


def test_session_drop_indexes_targets_graph_schema() -> None:
    """Verifies that session.drop_indexes() targets the session's graph schema in Apache AGE."""
    session_default = Session(dialect="age")
    dropped_default = session_default.drop_indexes(Article)
    assert len(dropped_default) == 1
    assert dropped_default[0] == 'DROP INDEX IF EXISTS "age_graph".article_ft_idx;'

    session_custom = Session(dialect="age", graph_name="production_graph")
    dropped_custom = session_custom.drop_indexes(Article)
    assert len(dropped_custom) == 1
    assert dropped_custom[0] == 'DROP INDEX IF EXISTS "production_graph".article_ft_idx;'

    # Explicit override
    dropped_override = session_custom.drop_indexes(Article, graph_name="tenant_omega")
    assert dropped_override[0] == 'DROP INDEX IF EXISTS "tenant_omega".article_ft_idx;'

    # Invalid graph name rejected
    with pytest.raises(ValueError):
        session_custom.drop_indexes(Article, graph_name="bad name")


def test_session_apply_schema_targets_graph_schema() -> None:
    """Verifies that session.apply_schema() passes session graph_name to DDL generation."""
    session = Session(dialect="age", graph_name="production_graph")
    applied = session.apply_schema(Article)
    assert len(applied) == 1
    assert 'ON "production_graph"."Article"' in applied[0]


@pytest.mark.asyncio
async def test_async_session_ddl_targets_graph_schema() -> None:
    """Verifies that AsyncSession DDL methods target session graph_name and validate identifiers."""
    session_custom = AsyncSession(dialect="age", graph_name="production_graph")
    stmts_create = await session_custom.create_indexes(Article)
    assert len(stmts_create) == 1
    assert 'ON "production_graph"."Article"' in stmts_create[0]

    stmts_drop = await session_custom.drop_indexes(Article)
    assert len(stmts_drop) == 1
    assert stmts_drop[0] == 'DROP INDEX IF EXISTS "production_graph".article_ft_idx;'

    stmts_apply = await session_custom.apply_schema(Article)
    assert len(stmts_apply) == 1
    assert 'ON "production_graph"."Article"' in stmts_apply[0]

    with pytest.raises(ValueError):
        await session_custom.create_indexes(Article, graph_name="invalid; DROP")

    with pytest.raises(ValueError):
        await session_custom.drop_indexes(Article, graph_name="a-b")
