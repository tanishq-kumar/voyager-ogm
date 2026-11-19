"""Tests for declarative Field defaults (#197).

Covers instance-level default application, per-instance default factories,
explicit overrides, query/write paths, bulk ingestion, and relationship
defaults. Instance attributes stay bound to the query DSL; stored values are
read with ``Node.get`` / ``Relationship.get``. All tests are offline and do not
require a database.
"""

from __future__ import annotations

import pytest
from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    Session,
    node,
    relationship,
    reset_alias_counters,
)
from voyager_ogm.bridge import MockBridge
from voyager_ogm.models import BoundField


@node("Account")
class Account(Node):
    acc_no: str = Field(primary_key=True)
    status: str = Field(default="PENDING")
    name: str = Field()


@node("Bag")
class Bag(Node):
    id: int = Field(primary_key=True)
    tags: list[str] = Field(default_factory=list)


@node("Plain")
class Plain(Node):
    id: int = Field(primary_key=True)
    name: str = Field()


@node("Setting")
class Setting(Node):
    status: str = Field(default="PENDING")


@relationship("RATED")
class Rated(Relationship):
    score: int = Field(default=5)
    tags: list[str] = Field(default_factory=list)


@pytest.fixture(autouse=True)
def _reset_aliases() -> None:
    reset_alias_counters()


def _split_merge_statement(statement: str) -> tuple[str, str]:
    """Splits a MERGE statement into its create and match assignment parts."""
    create_part, _, match_part = statement.partition("ON MATCH SET")
    return create_part, match_part


def test_issue_repro_exposes_default_through_get_and_create() -> None:
    """The stored default is readable with get() and written by Query.create."""
    account = Account(acc_no="A1")

    assert account.get("status") == "PENDING"

    compiled = Query.create(account).compile("cypher")
    assert compiled.statement == "CREATE (_account_0:Account {acc_no: $p0, status: $p1})"
    assert compiled.parameters == {"p0": "A1", "p1": "PENDING"}


def test_instance_attribute_is_still_bound_field_in_query_dsl() -> None:
    """Aliased instances keep building expressions from their field attributes."""
    account = Account(alias="a")
    assert isinstance(account.status, BoundField)

    compiled = Query.match(account).where(account.status == "ACTIVE").return_(account.name)
    compiled_query = compiled.compile("cypher")
    assert compiled_query.statement == "MATCH (a:Account) WHERE a.status = $p0 RETURN a.name"
    assert compiled_query.parameters == {"p0": "ACTIVE"}


def test_merge_with_primary_key_puts_default_only_on_create() -> None:
    """A default-filled field never appears in the MERGE key or ON MATCH SET."""
    account = Account(acc_no="A1")

    compiled = Query.merge(account).compile("cypher")
    assert "MERGE (_account_0:Account {acc_no: $p0})" in compiled.statement
    assert "ON CREATE SET _account_0.status = $p1" in compiled.statement
    assert "ON MATCH SET" not in compiled.statement
    assert compiled.parameters["p1"] == "PENDING"


def test_merge_without_primary_key_does_not_key_on_default() -> None:
    """The no-key fallback ignores default-filled values."""
    compiled = Query.merge(Setting()).compile("cypher")

    assert compiled.statement == "MERGE (_setting_0:Setting) ON CREATE SET _setting_0.status = $p0"
    assert compiled.parameters == {"p0": "PENDING"}


def test_explicitly_set_defaultable_value_is_written_on_match() -> None:
    """Once supplied explicitly, the field behaves like a normal merge property."""
    account = Account(acc_no="A1", status="ACTIVE")

    compiled = Query.merge(account).compile("cypher")
    assert "ON CREATE SET _account_0.status = $p1" in compiled.statement
    assert "ON MATCH SET _account_0.status = $p1" in compiled.statement
    assert compiled.parameters["p1"] == "ACTIVE"


def test_setting_a_defaulted_field_later_makes_it_a_normal_value() -> None:
    """Assigning the field after construction clears its default-only marker."""
    account = Account(acc_no="A1")
    assert account.get("status") == "PENDING"

    account.status = "ACTIVE"
    assert account.get("status") == "ACTIVE"

    compiled = Query.merge(account).compile("cypher")
    assert "ON MATCH SET _account_0.status = $p1" in compiled.statement


def test_default_factory_is_called_per_instance() -> None:
    """Each instance receives a distinct value from the factory."""
    calls: list[int] = []

    def make_tags() -> list[str]:
        calls.append(1)
        return []

    @node("FactoryBag")
    class FactoryBag(Node):
        id: int = Field(primary_key=True)
        tags: list[str] = Field(default_factory=make_tags)

    # Defining the model must not invoke the factory.
    assert calls == []

    first = FactoryBag(id=1)
    second = FactoryBag(id=2)
    assert calls == [1, 1]
    assert first.get("tags") == []
    assert second.get("tags") == []
    assert first.get("tags") is not second.get("tags")

    first.get("tags").append("shared?")
    assert second.get("tags") == []


def test_default_factory_list_is_distinct_for_declared_model() -> None:
    """The module-level list factory also yields independent lists."""
    first = Bag(id=1)
    second = Bag(id=2)

    first.get("tags").append("only-first")

    assert first.get("tags") == ["only-first"]
    assert second.get("tags") == []
    assert first.get("tags") is not second.get("tags")


def test_explicit_value_overrides_default() -> None:
    """An explicitly supplied value wins over the default."""
    account = Account(acc_no="A2", status="ACTIVE")
    assert account.get("status") == "ACTIVE"

    compiled = Query.create(account).compile("cypher")
    assert compiled.parameters["p1"] == "ACTIVE"


def test_explicit_none_overrides_default() -> None:
    """An explicitly supplied None also wins and is not replaced."""
    account = Account(acc_no="A3", status=None)
    assert account.get("status") is None
    assert "status" not in account._defaulted_fields


def test_both_default_and_default_factory_raises() -> None:
    """Declaring both forms is rejected when the Field is constructed."""
    with pytest.raises(TypeError, match="default.*default_factory"):
        Field(default="value", default_factory=list)


def test_class_level_access_still_returns_field_reference() -> None:
    """Class-level access keeps returning the descriptor used by the query DSL."""
    assert isinstance(Account.status, Field)
    assert Account.status is Account._schema_fields["status"]
    assert Account.status is Account.__dict__["status"]


def test_field_without_default_still_returns_bound_field() -> None:
    """Fields with no declared default keep today's instance behaviour."""
    plain = Plain(id=1)
    assert isinstance(plain.name, BoundField)


def test_bulk_create_plan_fills_defaults() -> None:
    """Bulk CREATE records receive the declared default."""
    session = Session(dialect="cypher")
    plan = session.bulk_create(Account, [{"acc_no": "B1"}], batch_size=10)

    assert "status" in plan.statement
    assert plan.batches_data[0][0]["status"] == "PENDING"

    # The caller's record must not be mutated.
    source = {"acc_no": "B2"}
    session.bulk_create(Account, [source], batch_size=10)
    assert source == {"acc_no": "B2"}


def test_bulk_merge_plan_writes_default_only_on_create() -> None:
    """Bulk MERGE applies defaults on creation and never overwrites on match."""
    session = Session(dialect="cypher")
    plan = session.bulk_upsert(Account, [{"acc_no": "B3"}], key_field="acc_no")

    assert plan.batches_data[0][0]["status"] == "PENDING"
    create_part, match_part = _split_merge_statement(plan.statement)
    assert "_account_0.status = row.status" in create_part
    assert "_account_0.status = row.status" not in match_part


def test_bulk_merge_plan_keeps_explicit_values_on_match() -> None:
    """An explicitly supplied defaultable field still updates on match."""
    session = Session(dialect="cypher")
    plan = session.bulk_upsert(Account, [{"acc_no": "B4", "status": "ACTIVE"}], key_field="acc_no")

    assert "ON CREATE SET _account_0.status = row.status" in plan.statement
    assert "ON MATCH SET _account_0.status = row.status" in plan.statement


def test_session_flush_writes_default_only_on_create() -> None:
    """Flushing a new tracked node writes the default on the create path."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)
    account = Account(acc_no="F1")

    session.register(account)
    results = session.flush()

    assert len(results) == 1
    statement, parameters = bridge.executed_queries[0]
    create_part, match_part = _split_merge_statement(statement)
    assert "_account_0.status = row.status" in create_part
    assert "_account_0.status = row.status" not in match_part
    assert parameters["batch"][0]["status"] == "PENDING"


def test_session_flush_update_does_not_overwrite_default() -> None:
    """Flushing an update writes a default on create only, never on match."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)
    hydrated = Account(acc_no="H2")
    session.register(hydrated, is_clean=True)

    hydrated.name = "Stored"
    session.flush()

    statement, parameters = bridge.executed_queries[0]
    create_part, match_part = _split_merge_statement(statement)
    assert "_account_0.status = row.status" in create_part
    assert "_account_0.status = row.status" not in match_part
    assert "_account_0.name = row.name" in match_part
    assert parameters["batch"][0]["status"] == "PENDING"


def test_node_save_writes_default_only_on_create() -> None:
    """Node.save() writes the default on the create path."""
    bridge = MockBridge()
    session = Session(bridge=bridge, enable_identity_map=True)
    account = Account(acc_no="S1")

    session.register(account)
    result = account.save()

    assert result is not None
    statement, parameters = bridge.executed_queries[0]
    create_part, match_part = _split_merge_statement(statement)
    assert "_account_0.status = row.status" in create_part
    assert "_account_0.status = row.status" not in match_part
    assert parameters["batch"][0]["status"] == "PENDING"


def test_hydrated_instance_does_not_flush_defaults() -> None:
    """An entity loaded from stored data is clean, so defaults are not flushed."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)

    # Query results are plain mappings in this OGM; constructing the model from
    # a stored row and registering it as clean is the hydration path.
    hydrated = Account(**{"acc_no": "H1"})
    session.register(hydrated, is_clean=True)

    assert session.flush() == []
    assert bridge.executed_queries == []

    hydrated.name = "Stored"
    hydrated.save()
    statement, parameters = bridge.executed_queries[0]
    create_part, match_part = _split_merge_statement(statement)
    assert "_account_0.status = row.status" in create_part
    assert "_account_0.status = row.status" not in match_part
    assert parameters["batch"][0]["status"] == "PENDING"
    assert parameters["batch"][0]["name"] == "Stored"


def test_relationship_defaults_are_applied_and_written() -> None:
    """Relationship defaults are readable and included in CREATE."""
    source = Account(acc_no="R1")
    target = Account(acc_no="R2")
    rating = Rated()

    assert rating.get("score") == 5
    assert rating.get("tags") == []

    compiled = Query.create(source).to(rating).node(target).compile("cypher")
    assert "score" in compiled.statement
    assert 5 in compiled.parameters.values()
    assert [] in compiled.parameters.values()


def test_bulk_relationship_plan_fills_defaults() -> None:
    """Bulk relationship ingestion records receive relationship defaults."""
    session = Session(dialect="cypher")
    plan = session.bulk_create_relationships(
        Rated,
        [{"from_acc_no": "R1", "to_acc_no": "R2"}],
        from_label="Account",
        from_key="acc_no",
        to_label="Account",
        to_key="acc_no",
    )

    assert plan.batches_data[0][0]["score"] == 5
    assert plan.batches_data[0][0]["tags"] == []


def test_bulk_merge_plan_mixed_explicit_and_defaulted_records() -> None:
    """One defaulted record cannot make its field create-only for other rows."""
    session = Session(dialect="cypher")
    plan = session.bulk_upsert(
        Account,
        [{"acc_no": "A1", "status": "ACTIVE"}, {"acc_no": "A2"}],
        key_field="acc_no",
    )

    assert plan.num_batches == 2
    by_key = {batch.parameters["batch"][0]["acc_no"]: batch.statement for batch in plan.batches}
    assert set(by_key) == {"A1", "A2"}

    explicit_create, explicit_match = _split_merge_statement(by_key["A1"])
    assert "_account_0.status = row.status" in explicit_create
    assert "_account_0.status = row.status" in explicit_match

    defaulted_create, defaulted_match = _split_merge_statement(by_key["A2"])
    assert "_account_0.status = row.status" in defaulted_create
    assert "_account_0.status = row.status" not in defaulted_match


def test_bulk_merge_plan_all_explicit_matches_origin_main_statement() -> None:
    """A batch where every row supplies the field keeps the old MERGE output."""
    session = Session(dialect="cypher")
    plan = session.bulk_upsert(
        Account,
        [{"acc_no": "A1", "status": "ACTIVE", "name": "Alice"}],
        key_field="acc_no",
    )

    expected = (
        "UNWIND $batch AS row MERGE (_account_0:Account {acc_no: row.acc_no}) "
        "ON CREATE SET _account_0.status = row.status, _account_0.name = row.name "
        "ON MATCH SET _account_0.status = row.status, _account_0.name = row.name"
    )
    assert plan.statement == expected
    assert plan.batches[0].statement == expected
    assert plan.total_records == 1


def test_bulk_merge_defaulted_batches_use_session_age_graph_name() -> None:
    """Every create-only batch statement targets the session's Apache AGE graph."""
    session = Session(dialect="age", graph_name="g2")
    plan = session.bulk_upsert(
        Account,
        [
            {"acc_no": "G1"},
            {"acc_no": "G2", "status": "ACTIVE"},
            {"acc_no": "G3"},
        ],
        key_field="acc_no",
        batch_size=1,
    )

    assert plan.num_batches > 1
    for batch in plan.batches:
        assert "cypher('g2'," in batch.statement
        assert "age_graph" not in batch.statement


def test_bulk_merge_plan_all_defaulted_records_are_create_only() -> None:
    """When every row defaults a field, the field stays create-only throughout."""
    session = Session(dialect="cypher")
    plan = session.bulk_upsert(Account, [{"acc_no": "A2"}, {"acc_no": "A3"}], key_field="acc_no")

    assert plan.num_batches == 1
    create_part, match_part = _split_merge_statement(plan.batches[0].statement)
    assert "_account_0.status = row.status" in create_part
    assert "_account_0.status = row.status" not in match_part
    assert [row["acc_no"] for row in plan.batches[0].parameters["batch"]] == ["A2", "A3"]


def test_bulk_merge_plan_batch_size_splits_per_group_and_preserves_order() -> None:
    """Each create-only group honours batch_size and keeps its own record order."""
    session = Session(dialect="cypher")
    plan = session.bulk_upsert(
        Account,
        [
            {"acc_no": "E1", "status": "S1"},
            {"acc_no": "D1"},
            {"acc_no": "E2", "status": "S2"},
            {"acc_no": "D2"},
            {"acc_no": "E3", "status": "S3"},
        ],
        key_field="acc_no",
        batch_size=2,
    )

    assert plan.num_batches == 3
    ordered_keys = [[row["acc_no"] for row in batch.parameters["batch"]] for batch in plan.batches]
    assert ordered_keys == [["E1", "E2"], ["E3"], ["D1", "D2"]]

    for batch in plan.batches[:2]:
        create_part, match_part = _split_merge_statement(batch.statement)
        assert "_account_0.status = row.status" in create_part
        assert "_account_0.status = row.status" in match_part
    create_part, match_part = _split_merge_statement(plan.batches[2].statement)
    assert "_account_0.status = row.status" in create_part
    assert "_account_0.status = row.status" not in match_part


def test_session_flush_mixed_instances_use_per_record_statements() -> None:
    """Flushing explicit and defaulted instances selects the statement per row."""
    bridge = MockBridge()
    session = Session(bridge=bridge, dialect="cypher", enable_identity_map=True)
    explicit = Account(acc_no="A1", status="ACTIVE")
    defaulted = Account(acc_no="A2")
    session.register(explicit)
    session.register(defaulted)

    session.flush()

    by_key = {
        parameters["batch"][0]["acc_no"]: (statement, parameters)
        for statement, parameters in bridge.executed_queries
    }
    assert set(by_key) == {"A1", "A2"}

    explicit_create, explicit_match = _split_merge_statement(by_key["A1"][0])
    assert "_account_0.status = row.status" in explicit_create
    assert "_account_0.status = row.status" in explicit_match
    assert by_key["A1"][1]["batch"][0]["status"] == "ACTIVE"

    defaulted_create, defaulted_match = _split_merge_statement(by_key["A2"][0])
    assert "_account_0.status = row.status" in defaulted_create
    assert "_account_0.status = row.status" not in defaulted_match
    assert by_key["A2"][1]["batch"][0]["status"] == "PENDING"


def test_bulk_upsert_mixed_instance_records_use_per_record_default_markers() -> None:
    """Instance-derived records keep their own default markers in bulk_upsert."""
    session = Session(dialect="cypher")
    explicit = Account(acc_no="A1", status="ACTIVE")
    defaulted = Account(acc_no="A2")

    plan = session.bulk_upsert(
        Account,
        [
            {**explicit.dirty_fields, "acc_no": "A1"},
            {**defaulted.dirty_fields, "acc_no": "A2"},
        ],
        key_field="acc_no",
        defaulted_fields=[
            set(explicit._defaulted_fields),
            set(defaulted._defaulted_fields),
        ],
    )

    assert plan.num_batches == 2
    by_key = {batch.parameters["batch"][0]["acc_no"]: batch.statement for batch in plan.batches}
    explicit_create, explicit_match = _split_merge_statement(by_key["A1"])
    assert "_account_0.status = row.status" in explicit_create
    assert "_account_0.status = row.status" in explicit_match

    defaulted_create, defaulted_match = _split_merge_statement(by_key["A2"])
    assert "_account_0.status = row.status" in defaulted_create
    assert "_account_0.status = row.status" not in defaulted_match
