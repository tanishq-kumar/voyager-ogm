"""Comprehensive tests for Tier-1 Release-Gating Features:
1. spec_version: Query Spec Interchange Protocol v1 enforcement.
2. ErrorCode: Machine-readable ErrorCode enum and VoyagerError(ValueError) with .code.
3. MockBridge fallback: Loud failure on unrecognized URI schemes, explicit fallback='mock' opt-in.
"""

from __future__ import annotations

import pytest
from voyager_ogm import (
    SPEC_VERSION,
    AsyncSession,
    MockBridge,
    Session,
    VoyagerError,
    col,
    create_bridge,
)
from voyager_ogm._voyager_rs import compile_query_from_spec

# ---------------------------------------------------------------------------
# 1. spec_version Protocol Verification
# ---------------------------------------------------------------------------


def test_spec_version_constant_exported() -> None:
    """SPEC_VERSION must be exported at the package root and equal 1."""
    assert SPEC_VERSION == 1


def test_expression_to_spec_dict_emits_spec_version() -> None:
    """Expression.to_spec_dict() must emit {"spec_version": 1, "expr": ...}."""
    expr = col("score") > 10
    spec_dict = expr.to_spec_dict()
    assert spec_dict["spec_version"] == 1
    assert "expr" in spec_dict
    assert spec_dict["expr"][0] == "bin"
    assert spec_dict["expr"][1] == ">"


def test_compile_query_from_spec_rejects_unversioned_spec() -> None:
    """Unversioned specs must be rejected with INVALID_SPEC VoyagerError."""
    spec = {
        "matches": [
            {
                "paths": [[("node", "p", ["Person"], None)]],
            }
        ],
    }
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "cypher")

    assert exc_info.value.code == "INVALID_SPEC"
    assert "Missing required 'spec_version'" in str(exc_info.value)
    # Subclass verification: also catches as ValueError
    assert isinstance(exc_info.value, ValueError)


def test_compile_query_from_spec_rejects_unsupported_version() -> None:
    """Unsupported spec versions (e.g. 2) must be rejected with INVALID_SPEC."""
    spec = {
        "spec_version": 2,
        "matches": [
            {
                "paths": [[("node", "p", ["Person"], None)]],
            }
        ],
    }
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "cypher")

    assert exc_info.value.code == "INVALID_SPEC"
    assert "Unsupported query spec_version: 2" in str(exc_info.value)


def test_compile_query_from_spec_rejects_invalid_clause_version() -> None:
    """Inner match clauses with unsupported spec_version must be rejected."""
    spec = {
        "spec_version": 1,
        "matches": [
            {
                "spec_version": 99,
                "paths": [[("node", "p", ["Person"], None)]],
            }
        ],
    }
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "cypher")

    assert exc_info.value.code == "INVALID_SPEC"
    assert "Unsupported match clause spec_version: 99" in str(exc_info.value)


def test_compile_query_from_spec_valid_spec_succeeds() -> None:
    """Valid v1 spec dictionary compiles cleanly to Cypher."""
    spec = {
        "spec_version": 1,
        "matches": [
            {
                "spec_version": 1,
                "paths": [[("node", "p", ["Person"], None)]],
            }
        ],
        "projections": [("field", "p", "name", None)],
    }
    res = compile_query_from_spec(spec, "cypher")
    assert res["statement"] == "MATCH (p:Person) RETURN p.name"


# ---------------------------------------------------------------------------
# 2. ErrorCode and VoyagerError Verification
# ---------------------------------------------------------------------------


def test_voyager_error_subclasses_value_error() -> None:
    """VoyagerError must inherit from built-in ValueError for non-breaking catch sites."""
    assert issubclass(VoyagerError, ValueError)
    err = VoyagerError("test error", code="TEST_CODE")
    assert isinstance(err, ValueError)
    assert isinstance(err, VoyagerError)
    assert err.code == "TEST_CODE"
    assert str(err) == "test error"


def test_voyager_error_code_on_invalid_clause() -> None:
    """Malformed projection tuple must trigger INVALID_CLAUSE error code."""
    spec = {
        "spec_version": 1,
        "projections": [("unknown_tag", "p", "name")],
    }
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "cypher")

    assert exc_info.value.code == "INVALID_CLAUSE"
    # Legacy code catching ValueError continues to work
    assert isinstance(exc_info.value, ValueError)


def test_voyager_error_code_on_unsupported_dialect() -> None:
    """Invalid dialect name must trigger UNSUPPORTED_DIALECT error code."""
    spec = {
        "spec_version": 1,
        "matches": [
            {
                "paths": [[("node", "p", ["Person"], None)]],
            }
        ],
    }
    with pytest.raises(VoyagerError) as exc_info:
        compile_query_from_spec(spec, "non_existent_dialect")

    assert exc_info.value.code == "UNSUPPORTED_DIALECT"


# ---------------------------------------------------------------------------
# 3. MockBridge Fallback and Loud URI Resolution
# ---------------------------------------------------------------------------


def test_create_bridge_unrecognized_scheme_fails_loudly() -> None:
    """Typo'd or unrecognized database URI schemes must fail loudly with diagnostic message."""
    typo_uri = "postgress://user:pass@localhost:5432/db"
    with pytest.raises(ValueError) as exc_info:
        create_bridge(typo_uri)

    err_msg = str(exc_info.value)
    assert "No bridge available for URI" in err_msg
    assert "unrecognized scheme: 'postgress'" in err_msg
    assert "bolt, duckdb, postgres, falkordb" in err_msg
    assert "fallback='mock'" in err_msg


def test_create_bridge_none_fails_loudly_without_fallback() -> None:
    """create_bridge(None) without explicit fallback must raise ValueError."""
    with pytest.raises(ValueError, match="No database driver, connection, or URI provided"):
        create_bridge(None)

    with pytest.raises(ValueError, match="No database driver, connection, or URI provided"):
        create_bridge(None, is_async=True)


def test_create_bridge_explicit_mock_opt_in() -> None:
    """create_bridge with fallback='mock' or mock:// must return MockBridge."""
    # Explicit fallback='mock' on typo URI
    b1 = create_bridge("postgress://localhost:5432/db", fallback="mock")
    assert isinstance(b1, MockBridge)

    b1_async = create_bridge("postgress://localhost:5432/db", is_async=True, fallback="mock")
    assert b1_async.__class__.__name__ == "AsyncMockBridge"

    # Explicit fallback='mock' on None
    b2 = create_bridge(None, fallback="mock")
    assert isinstance(b2, MockBridge)

    # Explicit mock:// URI scheme
    b3 = create_bridge("mock://test_env")
    assert isinstance(b3, MockBridge)

    b3_async = create_bridge("mock://test_env", is_async=True)
    assert b3_async.__class__.__name__ == "AsyncMockBridge"


def test_session_typo_uri_fails_loudly() -> None:
    """Session initialized with a typo URI must fail loudly unless fallback='mock' is passed."""
    typo_uri = "postgress://localhost:5432/db"
    with pytest.raises(ValueError, match="No bridge available for URI"):
        Session(bridge=typo_uri)

    with pytest.raises(ValueError, match="No bridge available for URI"):
        AsyncSession(bridge=typo_uri)

    # With explicit fallback='mock', succeeds with MockBridge
    sess = Session(bridge=typo_uri, fallback="mock")
    assert isinstance(sess.bridge, MockBridge)

    async_sess = AsyncSession(bridge=typo_uri, fallback="mock")
    assert async_sess.bridge.__class__.__name__ == "AsyncMockBridge"
