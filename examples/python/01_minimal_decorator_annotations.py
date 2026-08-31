"""Approach 1: Minimal Decorator & Pure Type Annotations (No Field() needed).

Run with: `uv run python examples/python/01_minimal_decorator_annotations.py`
"""

from __future__ import annotations

from voyager_ogm import Query, node, relationship


# 1. Define graph entities using standard Python type annotations
@node
class Developer:
    name: str
    age: int
    skills: list[str]
    level: str = "Senior"


@node
class Project:
    title: str
    stars: int = 0
    license: str = "MIT"


@relationship
class ContributedTo:
    commits: int
    role: str = "Maintainer"


def main() -> None:
    print("=== Voyager OGM (Python) - Approach 1: Minimal Decorators & Type Hints ===\n")

    # 2. Instantiate entities (auto-aliased: _developer_0, _project_0, _contributedto_0)
    dev = Developer()
    proj = Project()
    contrib = ContributedTo()

    print(f"Auto-generated alias for dev:  {dev.alias}")
    print(f"Auto-generated alias for proj: {proj.alias}")
    print(f"Auto-generated alias for rel:  {contrib.alias}\n")

    # 3. Fluent query construction with natural operators
    query = (
        Query.match(dev)
        .to(contrib)
        .node(proj)
        .where(
            dev.age >= 21,
            proj.stars > 100,
            dev.name.contains("Linus"),
        )
        .return_(dev.name, proj.title, commits=contrib.commits)
        .order_by(proj.stars)
        .limit(10)
    )

    compiled = query.compile("cypher")
    print(f"Generated Cypher:\n  {compiled.statement}\n")
    print(f"Extracted Parameters:\n  {compiled.parameters}\n")


if __name__ == "__main__":
    main()
