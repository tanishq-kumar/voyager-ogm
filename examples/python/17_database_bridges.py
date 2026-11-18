"""Voyager OGM: Database Bridges & create_bridge() URI Dispatch.

Teaches:
    - Connecting to graph and relational backends using `create_bridge()`
    - URI schemes: `bolt://`, `duckdb://`, `postgres://`, `falkordb://`, `relate://`, `mock://`
    - Graceful offline fallback with `fallback="mock"`
    - In-memory execution using embedded DuckDB and Relate engines
    - Unified database bridge interface (`execute()`, `ping()`, `close()`)

Prerequisites:
    - Engine: None (runs offline using in-memory DuckDB, Relate, and mock bridges)
    - Extras: None

Run with:
    `uv run python examples/python/17_database_bridges.py`
"""

from __future__ import annotations

from voyager_ogm import (
    DuckDbBridge,
    Field,
    MockBridge,
    Node,
    Query,
    RelateBridge,
    Session,
    create_bridge,
    node,
)


@node(label="Person")
class Person(Node):
    name: str = Field()
    age: int = Field()


@node(label="User")
class User(Node):
    name: str = Field()


def main() -> None:
    print("=== Voyager OGM: Database Bridges & create_bridge() ===\n")

    # -------------------------------------------------------------------------
    # 1. Embedded In-Memory DuckDB Bridge (duckdb://)
    # -------------------------------------------------------------------------
    print("--- [1] Embedded DuckDB Bridge (duckdb://:memory:) ---")
    duck_bridge = create_bridge("duckdb://:memory:")
    assert isinstance(duck_bridge, DuckDbBridge)
    print(f"Bridge Type: {type(duck_bridge).__name__}")
    print(f"Ping Liveness: {duck_bridge.ping()}")

    # Advanced: raw query — DuckDB relational SQL table setup and query (escape hatch)
    # Relational tabular DDL/DML on embedded DuckDB when not mapping through PGQ GRAPH_TABLE
    duck_bridge.execute("CREATE TABLE users (id INT, name VARCHAR, role VARCHAR);")
    duck_bridge.execute(
        "INSERT INTO users VALUES (?, ?, ?);",
        [1, "Alice", "Admin"],
    )
    duck_bridge.execute(
        "INSERT INTO users VALUES (?, ?, ?);",
        [2, "Bob", "Engineer"],
    )
    res_duck = duck_bridge.execute("SELECT * FROM users ORDER BY id;")
    print(f"Executed Query Results: {res_duck}\n")
    duck_bridge.close()

    # -------------------------------------------------------------------------
    # 2. Embedded In-Memory Relate Bridge (relate://)
    # -------------------------------------------------------------------------
    print("--- [2] Embedded Relate Bridge (relate://:memory:) ---")
    try:
        relate_bridge = create_bridge("relate://:memory:")
        assert isinstance(relate_bridge, RelateBridge)
        print(f"Bridge Type: {type(relate_bridge).__name__}")
        print(f"Ping Liveness: {relate_bridge.ping()}")

        # Execute through Voyager Session and OGM Query builder
        session_relate = Session(bridge=relate_bridge, dialect="cypher")
        charlie = Person(alias="p", name="Charlie", age=32)
        session_relate.execute(Query.create(charlie))

        p_query = Person(alias="p")
        res_relate = session_relate.execute(
            Query.match(p_query).return_(name=p_query.name, age=p_query.age)
        )
        print(f"Executed Cypher OGM Query Results: {res_relate.mappings().all()}\n")
        session_relate.close()
    except ImportError:
        print("[SKIP] relate-db package not installed.\n")

    # -------------------------------------------------------------------------
    # 3. Offline Testing & Fallback with fallback="mock"
    # -------------------------------------------------------------------------
    print("--- [3] Offline Testing with fallback='mock' and mock:// ---")

    # Native mock URI
    direct_mock = create_bridge("mock://")
    print(f"create_bridge('mock://') -> {type(direct_mock).__name__}")

    # Fallback when URI scheme has no driver installed or unparseable URI
    fallback_mock = create_bridge("unsupported_backend", fallback="mock")
    print(
        f"create_bridge('unsupported_backend', fallback='mock') -> {type(fallback_mock).__name__}"
    )

    # Bolt driver instantiation (lazy connection)
    bolt_bridge = create_bridge("bolt://127.0.0.1:7687", fallback="mock")
    print(f"create_bridge('bolt://127.0.0.1:7687') -> {type(bolt_bridge).__name__}")

    # Remote engines (FalkorDB, Postgres): instantiate or fall back cleanly if offline
    try:
        falkor_bridge = create_bridge("falkordb://127.0.0.1:6379", fallback="mock")
        print(f"create_bridge('falkordb://127.0.0.1:6379') -> {type(falkor_bridge).__name__}")
    except Exception as e:
        print(
            f"create_bridge('falkordb://...') offline: {e.__class__.__name__} (handled gracefully)"
        )

    try:
        pg_bridge = create_bridge("postgresql://postgres:pass@127.0.0.1:5432/db", fallback="mock")
        print(f"create_bridge('postgresql://...') -> {type(pg_bridge).__name__}")
    except Exception as e:
        print(
            f"create_bridge('postgresql://...') offline: {e.__class__.__name__} (handled gracefully)"
        )

    print()

    # -------------------------------------------------------------------------
    # 4. Using Bridge with Voyager Session
    # -------------------------------------------------------------------------
    print("--- [4] High-Level Session Integration ---")
    mock = MockBridge()
    session = Session(bridge=mock, dialect="cypher")
    u = User(alias="u")
    session.execute(Query.match(u).return_(u.name))
    print(f"Session executed query recorded in mock history: {mock.executed_queries[-1][0]}")

    print("\n[SUCCESS] Database bridge demonstration completed successfully.")


if __name__ == "__main__":
    main()
