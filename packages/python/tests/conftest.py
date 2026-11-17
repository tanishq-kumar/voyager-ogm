"""Shared pytest configuration, fixtures, and networking test utilities."""

from __future__ import annotations

import socket


def is_port_open(host: str, port: int, timeout: float = 0.5) -> bool:
    """Fast socket probe to detect whether a database or container port is actively listening.

    Args:
        host: Hostname or IP address (e.g. '127.0.0.1').
        port: TCP port number.
        timeout: Socket connect timeout in seconds.

    Returns:
        True if the port is open and accepting connections; False otherwise.
    """
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def neo4j_edition(driver) -> str:
    """Return the edition ('community' or 'enterprise') of the Neo4j server behind a driver.

    The default dev container is Neo4j Community; Enterprise is opt-in (see containers/compose.yaml).
    """
    with driver.session() as s:
        record = s.run("CALL dbms.components() YIELD edition RETURN edition").single()
    return str(record["edition"]).lower() if record else "unknown"


def community_compatible_ddl(statements: list[str]) -> list[str]:
    """Drop property-existence constraints (``REQUIRE ... IS NOT NULL``), which Neo4j Community rejects.

    Unique constraints and indexes are kept, so the rest of a schema test still runs on Community.
    """
    return [stmt for stmt in statements if "IS NOT NULL" not in stmt.upper()]
