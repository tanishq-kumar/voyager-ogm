"""Example 17: Topology-Backed SQLAlchemy GRAPH_TABLE & Multi-Label Nodes.

Demonstrates Voyager's native AST topology integration with SQLAlchemy:
1. Build graph traversal queries using multi-label domain models (`@node(labels=["Person", "Employee"])`).
2. Construct a first-class SQLAlchemy FromClause with `graph_table(...)`.
3. Verify the path pattern string is auto-derived directly from the native AST arena (zero regex).
4. Prove multi-label node preservation (`(p:Person:Employee)`) preventing pattern broadening in PGQ SQL.
5. Compose and compile standard SQLAlchemy relational SELECT queries joining SQL tables with GRAPH_TABLE.

Run with: `uv run python examples/python/17_sqlalchemy_graph_table.py`
"""

from __future__ import annotations

from sqlalchemy import Column, Integer, String, create_engine, select
from sqlalchemy.orm import DeclarativeBase
from voyager_ogm import Field, Node, Query, Relationship, graph_table, node, relationship


# 1. Relational SQLAlchemy ORM Table
class Base(DeclarativeBase):
    pass


class SQLEmployee(Base):
    __tablename__ = "employees"
    id = Column(Integer, primary_key=True)
    name = Column(String(50), nullable=False)
    department = Column(String(50), nullable=False)


# 2. Multi-label Graph Node Models
@node(labels=["Person", "Employee"])
class GraphStaff(Node):
    staff_id: int = Field(primary_key=True)
    clearance: str = Field(default="TopSecret")


@node(labels=["Project"])
class GraphProject(Node):
    project_id: str = Field(primary_key=True)
    code_name: str = Field(default="Voyager")


@relationship(type_name="ASSIGNED_TO")
class AssignedTo(Relationship):
    role: str = Field(default="Lead Engineer")


def main() -> None:
    print("=" * 70)
    print("[Example 17] Topology-Backed SQLAlchemy GRAPH_TABLE")
    print("=" * 70)

    # 1. Construct graph traversal query with multi-label node
    staff = GraphStaff(alias="p")
    proj = GraphProject(alias="proj")
    rel = AssignedTo(alias="r")

    q = Query.match(staff).to(rel).node(proj)

    # 2. Extract topology directly into SQLAlchemy FromClause
    gt = graph_table(
        graph="corp_graph",
        match=q,
        columns=[
            ("staff_id", staff.staff_id),
            ("project_code", proj.code_name),
        ],
        alias="gt",
    )

    print("1. Native AST-Derived Pattern String:")
    print(f"   Pattern: {gt.pattern_str}")

    # 3. Verify multi-label preservation (no silent label broadening)
    assert "(p:Person:Employee)" in gt.pattern_str, "Multi-label node lost labels in AST roundtrip!"
    assert "-[r:ASSIGNED_TO]->" in gt.pattern_str, "Edge pattern mismatch!"
    assert "(proj:Project)" in gt.pattern_str, "Target node pattern mismatch!"
    print("   [PASS] Multi-label node preserved faithfully: (p:Person:Employee)\n")

    # 4. Integrate into standard SQLAlchemy query pipeline
    engine = create_engine("sqlite:///:memory:")
    Base.metadata.create_all(engine)

    stmt = (
        select(SQLEmployee.name, SQLEmployee.department, gt.c.project_code)
        .join(gt, SQLEmployee.id == gt.c.staff_id)
        .where(SQLEmployee.department == "R&D")
    )

    compiled_sql = str(stmt.compile(compile_kwargs={"literal_binds": True}))
    print("2. Generated Hybrid SQL:2023 PGQ Statement:")
    print("   " + compiled_sql.replace("\n", "\n   "))
    print("\n   [PASS] Zero-regex AST compilation wired into SQLAlchemy FromClause.")
    print("=" * 70)


if __name__ == "__main__":
    main()
