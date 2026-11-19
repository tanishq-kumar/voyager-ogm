"""Live tests reproducing Apache AGE issue #179 with configurable graph_name.

Reproduction scenario:
1. Create graph "production_graph".
2. CREATE a node through a Session with graph_name="production_graph".
3. Read it back through that session.
4. Assert that the node is NOT visible in "age_graph".
5. Perform a write within session.transaction() targeting "production_graph",
   and assert it is visible in "production_graph" but NOT in "age_graph".
6. Drop "production_graph" during cleanup.
"""

import socket

import pytest
from voyager_ogm import Field, Node, Query, Session, create_bridge


def _is_age_reachable(host: str = "127.0.0.1", port: int = 5455) -> bool:
    """Checks whether the Apache AGE PostgreSQL port is reachable."""
    try:
        with socket.create_connection((host, port), timeout=1.0):
            return True
    except OSError:
        return False


class LivePerson(Node):
    __label__ = "LivePerson"
    id: int = Field(primary_key=True)
    name: str
    role: str = "Member"


@pytest.mark.live
def test_live_age_custom_graph_name_isolation() -> None:
    """Live test verifying that queries with custom graph_name target the intended graph."""
    if not _is_age_reachable():
        pytest.skip("Apache AGE container not reachable on port 5455")

    try:
        import psycopg
    except ImportError:
        pytest.skip("psycopg is required for live Apache AGE tests")

    uri = "postgresql://postgres:voyagerpass123@127.0.0.1:5455/voyager_graph"

    try:
        raw_conn = psycopg.connect(uri, autocommit=True, connect_timeout=2)
    except Exception as exc:
        pytest.skip(f"Failed to connect to Apache AGE: {exc}")

    created_production_graph = False
    try:
        with raw_conn.cursor() as cur:
            cur.execute("LOAD 'age';")
            cur.execute('SET search_path = ag_catalog, "$user", public;')

            # Ensure default age_graph exists and is clean
            cur.execute("SELECT count(*) FROM ag_catalog.ag_graph WHERE name = 'age_graph';")
            if cur.fetchone()[0] == 0:
                cur.execute("SELECT ag_catalog.create_graph('age_graph');")
            else:
                cur.execute(
                    "SELECT * FROM ag_catalog.cypher('age_graph', $$ MATCH (n:LivePerson) DETACH DELETE n $$) AS (res agtype);"
                )

            # Ensure production_graph exists and is clean
            cur.execute("SELECT count(*) FROM ag_catalog.ag_graph WHERE name = 'production_graph';")
            if cur.fetchone()[0] == 0:
                cur.execute("SELECT ag_catalog.create_graph('production_graph');")
                created_production_graph = True
            else:
                cur.execute(
                    "SELECT * FROM ag_catalog.cypher('production_graph', $$ MATCH (n:LivePerson) DETACH DELETE n $$) AS (res agtype);"
                )

        bridge_prod = create_bridge(uri)
        session_prod = Session(bridge=bridge_prod, dialect="age", graph_name="production_graph")

        bridge_default = create_bridge(uri)
        session_default = Session(bridge=bridge_default, dialect="age")  # defaults to "age_graph"

        assert session_prod.graph_name == "production_graph"
        assert session_default.graph_name == "age_graph"

        # 1. CREATE a node through session_prod
        p = LivePerson("p")
        q_create = Query.create(LivePerson("p", id=101, name="Alice", role="Admin")).return_(p.name)
        res_create = session_prod.execute(q_create)
        assert len(res_create) == 1
        assert "Alice" in str(res_create[0]["name"])

        # 2. Read it back through session_prod (must be found)
        q_read_alice = (
            Query.match(LivePerson("p"))
            .where(LivePerson("p").name == "Alice")
            .return_(LivePerson("p").name, LivePerson("p").role)
        )
        res_prod = session_prod.execute(q_read_alice)
        assert len(res_prod) == 1
        assert "Alice" in str(res_prod[0]["name"])

        # 3. Assert Alice is NOT visible in age_graph
        res_default = session_default.execute(q_read_alice)
        assert len(res_default) == 0, "Node created in production_graph leaked into age_graph!"

        # 4. Include a session.transaction() write on production_graph
        with session_prod.transaction():
            q_tx_create = Query.create(
                LivePerson("p", id=102, name="Bob", role="Engineer")
            ).return_(p.name)
            res_tx = session_prod.execute(q_tx_create)
            assert len(res_tx) == 1

        # Read Bob back from production_graph (must be found)
        q_read_bob = (
            Query.match(LivePerson("p"))
            .where(LivePerson("p").name == "Bob")
            .return_(LivePerson("p").name)
        )
        res_bob_prod = session_prod.execute(q_read_bob)
        assert len(res_bob_prod) == 1
        assert "Bob" in str(res_bob_prod[0]["name"])

        # Read Bob back from age_graph (must NOT be found)
        res_bob_default = session_default.execute(q_read_bob)
        assert len(res_bob_default) == 0, "Transaction write leaked into age_graph!"

        session_prod.close()
        session_default.close()

    finally:
        # Clean up: drop production_graph only if this test created it
        try:
            if created_production_graph:
                with raw_conn.cursor() as cur:
                    cur.execute("SELECT ag_catalog.drop_graph('production_graph', true);")
            raw_conn.close()
        except Exception:
            pass
