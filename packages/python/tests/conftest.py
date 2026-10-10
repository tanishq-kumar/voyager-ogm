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
