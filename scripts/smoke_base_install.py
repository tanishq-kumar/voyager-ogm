#!/usr/bin/env python3
"""Smoke test a base-only (``polars`` + ``pyarrow``) install of ``voyager-ogm``.

Run this inside a fresh virtual environment that has only the built wheel
installed (no extras), for example::

    python -I scripts/smoke_base_install.py

It verifies that importing the package and every public submodule works, that
queries compile for all supported dialects, and that optional integrations fail
with an error naming the extra to install.
"""

from __future__ import annotations

import importlib
import importlib.util
from collections.abc import Callable
from typing import Any

# Optional packages that must not be present when this script runs. If one is
# installed, the environment is not a base-only install and the checks below
# would give a false sense of safety.
_OPTIONAL_MODULES = (
    "anywidget",
    "duckdb",
    "marimo",
    "neo4j",
    "networkx",
    "psycopg",
    "relate",
    "sqlalchemy",
    "traitlets",
)

# Public submodules a user may import directly. None of them may require an
# optional extra at import time.
_PUBLIC_SUBMODULES = (
    "voyager_ogm.analytics",
    "voyager_ogm.bridge",
    "voyager_ogm.config",
    "voyager_ogm.expressions",
    "voyager_ogm.fn",
    "voyager_ogm.hybrid",
    "voyager_ogm.ingestion",
    "voyager_ogm.models",
    "voyager_ogm.query",
    "voyager_ogm.schema",
    "voyager_ogm.session",
    "voyager_ogm.sqlalchemy",
    "voyager_ogm.streaming",
    "voyager_ogm.transaction",
    "voyager_ogm.types",
    "voyager_ogm.unit_of_work",
    "voyager_ogm.viewer",
    "voyager_ogm.viewer.extractor",
    "voyager_ogm.viewer.widget",
)


def _expect_import_error(
    call: Callable[[], Any],
    extra: str,
    label: str,
) -> None:
    """Asserts that ``call`` raises ImportError mentioning ``extra``."""
    try:
        call()
    except ImportError as exc:
        message = str(exc)
        if extra not in message:
            raise AssertionError(
                f"{label}: expected the error to name {extra!r}, got {message!r}"
            ) from None
        print(f"ok: {label} -> {message}")
        return
    raise AssertionError(f"{label}: expected ImportError naming {extra!r}")


def _check_missing_optional_modules() -> None:
    """Fails unless every optional dependency is absent from this environment."""
    installed = [name for name in _OPTIONAL_MODULES if importlib.util.find_spec(name) is not None]
    if installed:
        raise SystemExit(
            "This smoke test must run in a fresh, base-only environment; "
            f"found optional module(s): {', '.join(installed)}"
        )
    print(f"ok: {len(_OPTIONAL_MODULES)} optional modules are absent")


def _check_imports() -> None:
    """Imports the package and every public submodule."""
    import voyager_ogm

    print(f"ok: import voyager_ogm {voyager_ogm.__version__} from {voyager_ogm.__file__}")
    for module_name in _PUBLIC_SUBMODULES:
        importlib.import_module(module_name)
    print(f"ok: imported {len(_PUBLIC_SUBMODULES)} public submodules")


def _check_query_compilation() -> None:
    """Defines a model and compiles a query for every supported dialect."""
    from voyager_ogm import Field, Node, Query, node

    @node(label="Person")
    class Person(Node):
        name: str = Field(primary_key=True)
        age: int = Field(default=0)

    person = Person(alias="p")
    query = Query.match(person).where(person.age >= 18).return_(name=person.name, age=person.age)

    compiled = {
        "cypher": query.compile("cypher").statement,
        "iso_gql": query.compile("iso_gql").statement,
        "age": query.compile("age", graph_name="social").statement,
        "sql_pgq": query.compile("sql_pgq", graph_name="social").statement,
    }
    for dialect, statement in compiled.items():
        if not statement.strip():
            raise AssertionError(f"{dialect}: compiled an empty statement")
        print(f"ok: {dialect}: {statement}")


def _check_sqlalchemy_errors() -> None:
    """The SQLAlchemy integration must fail clearly when its extra is missing."""

    def import_graph_table_clause() -> None:
        from voyager_ogm import GraphTableClause  # noqa: F401

    _expect_import_error(
        import_graph_table_clause,
        "voyager-ogm[sqlalchemy]",
        "from voyager_ogm import GraphTableClause",
    )

    def import_submodule_clause() -> None:
        from voyager_ogm import sqlalchemy

        _ = sqlalchemy.GraphTableClause

    _expect_import_error(
        import_submodule_clause,
        "voyager-ogm[sqlalchemy]",
        "voyager_ogm.sqlalchemy.GraphTableClause",
    )

    def call_graph_table() -> None:
        from voyager_ogm import graph_table

        graph_table("social", "MATCH (p:Person)", [("name", "p.name")])

    _expect_import_error(
        call_graph_table,
        "voyager-ogm[sqlalchemy]",
        "graph_table()",
    )

    def call_as_cte() -> None:
        from voyager_ogm import as_cte

        as_cte(object(), object(), object())

    _expect_import_error(call_as_cte, "voyager-ogm[sqlalchemy]", "as_cte()")


def _check_other_integration_errors() -> None:
    """Other optional integrations must fail clearly when their extra is missing."""
    from voyager_ogm import create_bridge, to_networkx

    def call_to_networkx() -> None:
        to_networkx([{"id": "a"}])

    _expect_import_error(call_to_networkx, "voyager-ogm[analytics]", "to_networkx()")

    for uri, extra in (
        ("neo4j://localhost:7687", "voyager-ogm[neo4j]"),
        ("duckdb://:memory:", "voyager-ogm[duckdb]"),
        ("age://localhost:5432/example", "voyager-ogm[age]"),
        ("relate://:memory:", "voyager-ogm[relate]"),
    ):
        _expect_import_error(
            lambda uri=uri: create_bridge(uri),
            extra,
            f"create_bridge({uri!r})",
        )


def main() -> int:
    """Runs every base-install smoke check."""
    _check_missing_optional_modules()
    _check_imports()
    _check_query_compilation()
    _check_sqlalchemy_errors()
    _check_other_integration_errors()
    print("PASS: base-install smoke test completed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
