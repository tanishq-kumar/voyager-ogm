"""Level 2: Intermediate - Connecting the Dots (Nodes & Relationships).

Learn how to connect entities with relationships, traverse incoming & outgoing edges,
and filter on edge properties (e.g. roles, timestamps).

Run with: `uv run python examples/python/02_intermediate_relationships.py`
"""

from __future__ import annotations

from voyager_ogm import Field, Query, node, relationship


# Step 1: Define Entities
@node(label="Actor")
class Actor:
    name: str
    born: int


@node(label="Movie")
class Movie:
    title: str
    released: int
    genre: str = "Sci-Fi"


@node(label="Director")
class Director:
    name: str


# Step 2: Define Relationships (Edges) with typed properties
@relationship(type_name="ACTED_IN")
class ActedIn:
    role: str = Field(name="character_name")
    earnings: float = 0.0


@relationship(type_name="DIRECTED")
class Directed:
    year: int = 1999


def main() -> None:
    print("=" * 65)
    print("[Level 2: Intermediate] Nodes, Edges & Multi-Hop Paths")
    print("=" * 65)

    # Graph Shape We Are Querying:
    # (Actor) -[ACTED_IN]-> (Movie) <-[DIRECTED]- (Director)
    actor = Actor("a")
    movie = Movie("m")
    director = Director("d")
    acted_in = ActedIn("rel_act")
    directed = Directed("rel_dir")

    query = (
        Query.match(actor)
        .to(acted_in)  # Outgoing edge -[rel_act:ACTED_IN]->
        .node(movie)  # Target node   (m:Movie)
        .from_(directed)  # Incoming edge <-[rel_dir:DIRECTED]-
        .node(director)  # Origin node   (d:Director)
        .where(
            actor.born > 1960,
            movie.released >= 1999,
            director.name.contains("Wachowski"),
        )
        .return_(
            actor.name,
            movie.title,
            director_name=director.name,
            character=acted_in.role,
        )
        .order_by(movie.released)
        .limit(10)
    )

    compiled = query.compile("cypher")

    print("\n1. Compiled Multi-Hop Traversal:")
    print(f"   {compiled.statement}\n")
    print(f"2. Extracted Literal Parameters: {compiled.parameters}\n")
    print("   Graph Topology Explored:")
    print("   (a:Actor)-[rel_act:ACTED_IN]->(m:Movie)<-[rel_dir:DIRECTED]-(d:Director)")
    print("=" * 65)


if __name__ == "__main__":
    main()
