"""Pytest runner for the SQL:2023 PGQ & DuckPGQ conformance corpus (test_data/tck/sql_pgq/*.test).

Parses sqllogictest (.test) files, compiles corresponding graph queries using the
Voyager Python SDK, and tests execution against DuckDB/DuckPGQ with expected output comparison.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

import pytest

try:
    import duckdb
except ImportError:
    duckdb = None

from voyager_ogm import (
    Field,
    Node,
    Query,
    Relationship,
    node,
    relationship,
    reset_alias_counters,
)

REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent
PGQ_TCK_DIR = REPO_ROOT / "test_data" / "tck" / "sql_pgq"


def _check_duckpgq() -> bool:
    if duckdb is None:
        return False
    try:
        con = duckdb.connect(":memory:", config={"allow_unsigned_extensions": "true"})
        con.execute("LOAD duckpgq;")
        return True
    except Exception:
        return False


HAS_DUCKPGQ = _check_duckpgq()


def translate_pgq_to_duckpgq(sql: str) -> str:
    """Translates standard ISO SQL:2023 PGQ DDL and queries into DuckPGQ dialect.

    DuckPGQ dialect differences from ISO SQL:2023 PGQ:
    1. In CREATE PROPERTY GRAPH:
       - VERTEX TABLES: rejects KEY (id) and PROPERTIES (...) clauses.
         DuckPGQ automatically exposes all columns as properties.
       - EDGE TABLES: requires SOURCE KEY (...) REFERENCES ... and
         DESTINATION KEY (...) REFERENCES ..., but rejects PROPERTIES (...) clauses.
    2. In GRAPH_TABLE queries:
       - Correlation alias for GRAPH_TABLE does not support 'AS' (i.e. ') gt' not ') AS gt').
    """
    if "CREATE PROPERTY GRAPH" in sql.upper():
        cleaned = re.sub(r"\bPROPERTIES\s*\([^)]*\)", "", sql, flags=re.IGNORECASE)

        def _strip_vertex_key(m: re.Match[str]) -> str:
            if m.group(1):
                return m.group(1)
            return ""

        cleaned = re.sub(
            r"(\b(?:SOURCE|DESTINATION)\s+KEY\s*\([^)]*\))|\bKEY\s*\([^)]*\)",
            _strip_vertex_key,
            cleaned,
            flags=re.IGNORECASE,
        )
        cleaned = re.sub(r"[ \t]+,", ",", cleaned)
        cleaned = re.sub(r"[ \t]+\n", "\n", cleaned)
        return cleaned

    return re.sub(r"\)\s+AS\s+([a-zA-Z_][a-zA-Z0-9_]*)", r") \1", sql, flags=re.IGNORECASE)


@dataclass
class PgqTestCase:
    file_name: str
    query_idx: int
    query_type: str
    statements: list[str] = field(default_factory=list)
    sql_query: str = ""
    expected_rows: list[list[str]] = field(default_factory=list)


def parse_pgq_test_file(fpath: Path) -> list[PgqTestCase]:
    """Parses a single DuckPGQ sqllogictest file into PgqTestCase instances."""
    fname = fpath.name
    content = fpath.read_text(encoding="utf-8")

    # Extract all setup statements
    statements: list[str] = []
    statement_blocks = content.split("statement ok")
    for block in statement_blocks[1:]:
        stmt_part = block.split("\nquery")[0].strip()
        # Clean lines
        cleaned_lines = [ln for ln in stmt_part.splitlines() if not ln.strip().startswith("#")]
        stmt = "\n".join(cleaned_lines).strip()
        if stmt:
            statements.append(stmt)

    # Extract query blocks
    cases: list[PgqTestCase] = []
    lines = content.splitlines()
    i = 0
    q_idx = 0
    while i < len(lines):
        line = lines[i].strip()
        if line.startswith("query"):
            q_idx += 1
            parts = line.split()
            q_type = parts[1] if len(parts) > 1 else ""
            i += 1
            query_lines: list[str] = []
            while i < len(lines) and not lines[i].strip().startswith("----"):
                if not lines[i].strip().startswith("#"):
                    query_lines.append(lines[i])
                i += 1

            sql_query = "\n".join(query_lines).strip().rstrip(";")

            expected_rows: list[list[str]] = []
            if i < len(lines) and lines[i].strip().startswith("----"):
                i += 1
                while (
                    i < len(lines)
                    and lines[i].strip()
                    and not lines[i].strip().startswith("#")
                    and not lines[i].strip().startswith("query")
                ):
                    expected_rows.append(lines[i].strip().split("\t"))
                    i += 1

            cases.append(
                PgqTestCase(
                    file_name=fname,
                    query_idx=q_idx,
                    query_type=q_type,
                    statements=list(statements),
                    sql_query=sql_query,
                    expected_rows=expected_rows,
                )
            )
        else:
            i += 1

    return cases


def load_all_pgq_test_cases() -> list[PgqTestCase]:
    cases: list[PgqTestCase] = []
    for f in sorted(PGQ_TCK_DIR.glob("*.test")):
        cases.extend(parse_pgq_test_file(f))
    return cases


ALL_CASES = load_all_pgq_test_cases()


@pytest.fixture(autouse=True)
def _reset():
    reset_alias_counters()


# Schema definitions for SDK compilation
@node(label="Student")
class Student(Node):
    id = Field()
    name = Field()
    age = Field()


@node(label="School")
class School(Node):
    id = Field()
    name = Field()
    city = Field()


@relationship(type_name="enrolled_in")
class EnrolledIn(Relationship):
    year = Field()


@node(label="Person")
class Person(Node):
    id = Field()
    name = Field()


@relationship(type_name="knows")
class Knows(Relationship):
    pass


@node(label="Employee")
class Employee(Node):
    id = Field()
    name = Field()
    department = Field()
    salary = Field()


def test_pgq_corpus_inventory():
    """Asserts that all 3 sqllogictest files in the corpus are found and parsed."""
    assert len(ALL_CASES) == 4
    files = {c.file_name for c in ALL_CASES}
    assert "graph_table_matching.test" in files
    assert "variable_length_paths.test" in files
    assert "aggregations_grouping.test" in files


def test_compile_graph_table_matching_query1():
    """Compiles graph_table_matching.test Query 1 using the Voyager Python SDK."""
    s = Student(alias="s")
    sch = School(alias="sch")
    q = (
        Query.match(s)
        .where(s.age >= 20)
        .to(EnrolledIn, "e")
        .node(sch)
        .return_(s_name=s.name, sch_name=sch.name, enroll_year=EnrolledIn("e").year)
        .order_by(s.name)
    )
    compiled = q.compile("sql_pgq", graph_name="university_pg")
    stmt = compiled.statement

    assert "FROM GRAPH_TABLE (university_pg MATCH" in stmt
    assert "(s IS Student) -[e IS enrolled_in]-> (sch IS School)" in stmt
    assert "WHERE s.age >= $p0" in stmt
    assert "COLUMNS (s.name AS s_name, sch.name AS sch_name, e.year AS enroll_year)" in stmt
    assert "ORDER BY s_name ASC" in stmt
    assert compiled.parameters == {"p0": 20}


def test_compile_graph_table_matching_query2():
    """Compiles graph_table_matching.test Query 2 using the Voyager Python SDK."""
    s = Student(alias="s")
    sch = School(alias="sch")
    q = (
        Query.match(s)
        .to(EnrolledIn, "e")
        .node(sch)
        .return_(sch_name=sch.name, student_count=s.id.count())
    )
    compiled = q.compile("sql_pgq", graph_name="university_pg")
    stmt = compiled.statement

    assert "SELECT sch_name, COUNT(id) AS student_count FROM GRAPH_TABLE (" in stmt
    assert "(s IS Student) -[e IS enrolled_in]-> (sch IS School)" in stmt
    assert "GROUP BY sch_name" in stmt


def test_compile_variable_length_paths_query():
    """Compiles variable_length_paths.test Query using the Voyager Python SDK."""
    p1 = Person(alias="p1")
    p2 = Person(alias="p2")
    q = (
        Query.match(p1)
        .where(p1.name == "Alice")
        .to(Knows, "k")
        .hops(1, 3)
        .node(p2)
        .return_(start_person=p1.name, reachable_person=p2.name)
        .order_by(p2.name)
    )
    compiled = q.compile("sql_pgq", graph_name="social_net_pg")
    stmt = compiled.statement

    assert "FROM GRAPH_TABLE (social_net_pg MATCH" in stmt
    assert "(p1 IS Person) -[k IS knows]->{1,3} (p2 IS Person)" in stmt
    assert "WHERE p1.name = $p0" in stmt
    assert "COLUMNS (p1.name AS start_person, p2.name AS reachable_person)" in stmt
    assert "ORDER BY reachable_person ASC" in stmt
    assert compiled.parameters == {"p0": "Alice"}


def test_compile_aggregations_grouping_query():
    """Compiles aggregations_grouping.test Query using the Voyager Python SDK."""
    e = Employee(alias="e")
    q = (
        Query.match(e)
        .return_(dept=e.department, emp_count=e.id.count(), avg_salary=e.salary.avg())
        .order_by(e.department)
    )
    compiled = q.compile("sql_pgq", graph_name="corp_pg")
    stmt = compiled.statement

    assert (
        "SELECT dept, COUNT(id) AS emp_count, AVG(salary) AS avg_salary FROM GRAPH_TABLE (" in stmt
    )
    assert "FROM GRAPH_TABLE (corp_pg MATCH (e IS Employee)" in stmt
    assert "GROUP BY dept" in stmt
    assert "ORDER BY dept ASC" in stmt


def test_translate_pgq_to_duckpgq_ddl():
    """Verifies that ISO SQL:2023 PGQ DDL is translated to DuckPGQ dialect."""
    iso_ddl = """
    CREATE PROPERTY GRAPH university_pg
      VERTEX TABLES (
        Student KEY (id) LABEL Student PROPERTIES (id, name, age),
        School KEY (id) LABEL School PROPERTIES (id, name, city)
      )
      EDGE TABLES (
        enrolled_in
          SOURCE KEY (student_id) REFERENCES Student (id)
          DESTINATION KEY (school_id) REFERENCES School (id)
          LABEL enrolled_in
          PROPERTIES (year)
      );
    """
    translated = translate_pgq_to_duckpgq(iso_ddl)
    assert "KEY (id)" not in translated
    assert "PROPERTIES" not in translated
    assert "SOURCE KEY (student_id) REFERENCES Student (id)" in translated
    assert "DESTINATION KEY (school_id) REFERENCES School (id)" in translated
    assert "Student  LABEL Student" in translated
    assert "School  LABEL School" in translated


def test_translate_pgq_to_duckpgq_query():
    """Verifies that GRAPH_TABLE query alias syntax is translated for DuckPGQ."""
    iso_query = """
    SELECT s_name, sch_name, enroll_year
    FROM GRAPH_TABLE (
      university_pg
      MATCH (s:Student) -[e:enrolled_in]-> (sch:School)
      COLUMNS (s.name AS s_name, sch.name AS sch_name, e.year AS enroll_year)
    ) AS gt
    ORDER BY s_name ASC;
    """
    translated = translate_pgq_to_duckpgq(iso_query)
    assert ") gt" in translated
    assert ") AS gt" not in translated


def test_translate_pgq_to_duckpgq_passthrough():
    """Verifies that non-PGQ DDL statements are preserved untouched."""
    stmt = "CREATE TABLE Student (id BIGINT PRIMARY KEY, name VARCHAR, age INT);"
    assert translate_pgq_to_duckpgq(stmt) == stmt


@pytest.mark.live
@pytest.mark.parametrize("case", ALL_CASES, ids=lambda c: f"{c.file_name}::query_{c.query_idx}")
@pytest.mark.xfail(
    not HAS_DUCKPGQ,
    reason="Live SQL/PGQ execution requires the duckpgq DuckDB extension binary, which is not available in the current environment",
    strict=True,
)
def test_live_execution_against_duckpgq(case: PgqTestCase):
    """Executes the sqllogictest case against DuckDB + DuckPGQ and compares against expected output.

    Marked with @pytest.mark.live and xfail(strict=True) when duckpgq extension is not present.
    """
    if duckdb is None:
        raise RuntimeError("duckdb is not installed")

    con = duckdb.connect(":memory:", config={"allow_unsigned_extensions": "true"})
    con.execute("LOAD duckpgq;")

    # Execute all setup DDL/DML statements
    for stmt in case.statements:
        con.execute(translate_pgq_to_duckpgq(stmt))

    # Execute query
    res = con.execute(translate_pgq_to_duckpgq(case.sql_query)).fetchall()

    # Compare against expected rows
    assert len(res) == len(case.expected_rows)
    for actual_row, expected_row in zip(res, case.expected_rows, strict=True):
        assert len(actual_row) == len(expected_row)
        for actual_val, expected_val in zip(actual_row, expected_row, strict=True):
            assert str(actual_val) == str(expected_val)
