"""Tests for Issue #54: Query profiling and plan explanation (.explain() and .profile())."""

import copy

from voyager_ogm import (
    CompiledQuery,
    Field,
    MockBridge,
    Node,
    Query,
    Session,
    explain,
    node,
    profile,
)


@node("Person")
class Person(Node):
    name: str = Field()
    age: int = Field()


class TestExplainAndProfile:
    """Test suite for query plan explanation and live execution profiling."""

    def test_fluent_chaining_explain(self) -> None:
        p = Person("p")
        query = Query.match(p).where(p.age > 30).return_(p.name).explain()

        assert query.execution_mode == "explain"

        compiled_cypher = query.compile("cypher")
        assert compiled_cypher.statement.startswith("EXPLAIN MATCH (p:Person)")
        assert compiled_cypher.execution_mode == "explain"

        compiled_gql = query.compile("iso_gql")
        assert compiled_gql.statement.startswith("EXPLAIN MATCH (p:Person)")
        assert compiled_gql.execution_mode == "explain"

        compiled_pgq = query.compile("sql_pgq")
        assert compiled_pgq.statement.startswith("EXPLAIN SELECT * FROM GRAPH_TABLE (")
        assert compiled_pgq.execution_mode == "explain"

        compiled_age = query.compile("age")
        assert compiled_age.statement.startswith("EXPLAIN SELECT * FROM cypher(")
        assert "$$ EXPLAIN" not in compiled_age.statement
        assert compiled_age.execution_mode == "explain"

    def test_fluent_chaining_profile(self) -> None:
        p = Person("p")
        query = Query.match(p).where(p.age > 30).return_(p.name).profile()

        assert query.execution_mode == "profile"

        compiled_cypher = query.compile("cypher")
        assert compiled_cypher.statement.startswith("PROFILE MATCH (p:Person)")
        assert compiled_cypher.execution_mode == "profile"

        compiled_gql = query.compile("iso_gql")
        assert compiled_gql.statement.startswith("PROFILE MATCH (p:Person)")
        assert compiled_gql.execution_mode == "profile"

        compiled_pgq = query.compile("sql_pgq")
        assert compiled_pgq.statement.startswith("EXPLAIN ANALYZE SELECT * FROM GRAPH_TABLE (")
        assert compiled_pgq.execution_mode == "profile"

        compiled_age = query.compile("age")
        assert compiled_age.statement.startswith("EXPLAIN ANALYZE SELECT * FROM cypher(")
        assert "$$ PROFILE" not in compiled_age.statement
        assert compiled_age.execution_mode == "profile"

    def test_fluent_chaining_both_composability(self) -> None:
        p = Person("p")

        # Explain then Profile
        q1 = Query.match(p).where(p.age > 30).return_(p.name).explain().profile()
        assert q1.execution_mode == "explain_and_profile"

        c1 = q1.compile("cypher")
        assert c1.statement.startswith("PROFILE MATCH (p:Person)")
        assert c1.execution_mode == "explain_and_profile"

        pgq1 = q1.compile("sql_pgq")
        assert pgq1.statement.startswith("EXPLAIN ANALYZE SELECT * FROM GRAPH_TABLE (")
        assert pgq1.execution_mode == "explain_and_profile"

        age1 = q1.compile("age")
        assert age1.statement.startswith("EXPLAIN ANALYZE SELECT * FROM cypher(")
        assert age1.execution_mode == "explain_and_profile"

        # Profile then Explain
        q2 = Query.match(p).where(p.age > 30).return_(p.name).profile().explain()
        assert q2.execution_mode == "explain_and_profile"

        c2 = q2.compile("cypher")
        assert c2.statement.startswith("PROFILE MATCH (p:Person)")
        assert c2.execution_mode == "explain_and_profile"

    def test_function_style_explain_and_profile(self) -> None:
        p = Person("p")
        base_query = Query.match(p).where(p.age > 30).return_(p.name)

        assert base_query.execution_mode == "normal"

        # Individual functions
        explained_query = explain(base_query)
        profiled_query = profile(base_query)

        # Immutability check: base_query must remain untouched!
        assert base_query.execution_mode == "normal"
        assert not base_query.compile("cypher").statement.startswith("EXPLAIN")
        assert not base_query.compile("cypher").statement.startswith("PROFILE")

        assert explained_query.execution_mode == "explain"
        assert explained_query.compile("cypher").statement.startswith("EXPLAIN MATCH (p:Person)")

        assert profiled_query.execution_mode == "profile"
        assert profiled_query.compile("cypher").statement.startswith("PROFILE MATCH (p:Person)")

        # Composability: explain can take a profiled query (and vice versa)
        both_query1 = explain(profiled_query)
        assert both_query1.execution_mode == "explain_and_profile"
        assert both_query1.compile("cypher").statement.startswith("PROFILE MATCH (p:Person)")

        both_query2 = profile(explained_query)
        assert both_query2.execution_mode == "explain_and_profile"
        assert both_query2.compile("cypher").statement.startswith("PROFILE MATCH (p:Person)")

        both_query3 = explain(profile(base_query))
        assert both_query3.execution_mode == "explain_and_profile"
        assert both_query3.compile("cypher").statement.startswith("PROFILE MATCH (p:Person)")

    def test_clone_copy_and_deepcopy(self) -> None:
        p = Person("p")
        q = Query.match(p).where(p.age > 30).return_(p.name)

        q_cloned = q.clone()
        q_cloned.explain()

        assert q.execution_mode == "normal"
        assert q_cloned.execution_mode == "explain"

        q_copied = copy.copy(q)
        q_copied.profile()

        assert q.execution_mode == "normal"
        assert q_copied.execution_mode == "profile"

        q_deep = copy.deepcopy(q)
        q_deep.explain().profile()

        assert q.execution_mode == "normal"
        assert q_deep.execution_mode == "explain_and_profile"

    def test_compiled_query_dataclass_defaults(self) -> None:
        cq = CompiledQuery(statement="MATCH (n) RETURN n", parameters={})
        assert cq.execution_mode == "normal"

        cq_explain = CompiledQuery(
            statement="EXPLAIN MATCH (n) RETURN n", parameters={}, execution_mode="explain"
        )
        assert cq_explain.execution_mode == "explain"

    def test_session_execute_integration(self) -> None:
        bridge = MockBridge()
        session = Session(bridge=bridge, dialect="cypher")

        p = Person("p")
        q = Query.match(p).where(p.age > 25).return_(p.name).explain()

        session.execute(q)

        assert len(bridge.executed_queries) == 1
        stmt, _params = bridge.executed_queries[0]
        assert stmt.startswith("EXPLAIN MATCH (p:Person)")
