#!/usr/bin/env python3
"""
Voyager OGM Synthetic Benchmark Dataset Generator.

Generates scalable graph datasets (1,000 to 1,000,000+ nodes and edges)
for benchmarking AST query compilation, Arrow PyCapsule zero-copy streaming,
and Polars hydration performance SLAs.
"""

import argparse
import csv
import json
import os
import random
import time

FIRST_NAMES = [
    "James",
    "Mary",
    "Robert",
    "Patricia",
    "John",
    "Jennifer",
    "Michael",
    "Linda",
    "David",
    "Elizabeth",
    "William",
    "Barbara",
    "Richard",
    "Susan",
    "Joseph",
    "Jessica",
    "Thomas",
    "Sarah",
    "Charles",
    "Karen",
    "Christopher",
    "Nancy",
    "Daniel",
    "Lisa",
    "Matthew",
    "Betty",
    "Anthony",
    "Margaret",
    "Mark",
    "Sandra",
    "Donald",
    "Ashley",
    "Steven",
    "Kimberly",
    "Paul",
    "Emily",
    "Andrew",
    "Donna",
    "Joshua",
    "Michelle",
]

LAST_NAMES = [
    "Smith",
    "Johnson",
    "Williams",
    "Brown",
    "Jones",
    "Garcia",
    "Miller",
    "Davis",
    "Rodriguez",
    "Martinez",
    "Hernandez",
    "Lopez",
    "Gonzalez",
    "Wilson",
    "Anderson",
    "Thomas",
    "Taylor",
    "Moore",
    "Jackson",
    "Martin",
    "Lee",
    "Perez",
    "Thompson",
    "White",
    "Harris",
    "Sanchez",
    "Clark",
    "Ramirez",
    "Lewis",
    "Robinson",
    "Walker",
]

DEPARTMENTS = [
    "Engineering",
    "Product",
    "Design",
    "Marketing",
    "Sales",
    "Legal",
    "Finance",
    "Research",
]
CITIES = [
    "San Francisco",
    "New York",
    "London",
    "Tokyo",
    "Berlin",
    "Paris",
    "Seattle",
    "Toronto",
    "Sydney",
    "Singapore",
]


def generate_dataset(num_nodes: int, num_edges: int, output_dir: str):
    os.makedirs(output_dir, exist_ok=True)
    random.seed(42)

    print(
        f"[Voyager OGM] Generating {num_nodes:,} nodes and {num_edges:,} edges\n"
        f"  Target directory: '{output_dir}'..."
    )
    start_time = time.time()

    # 1. Generate Nodes (CSV & JSON-L)
    nodes_csv_path = os.path.join(output_dir, "nodes_persons.csv")
    nodes_jsonl_path = os.path.join(output_dir, "nodes_persons.jsonl")

    print("  -> Writing nodes...")
    with (
        open(nodes_csv_path, "w", newline="", encoding="utf-8") as f_csv,
        open(nodes_jsonl_path, "w", encoding="utf-8") as f_jsonl,
    ):
        csv_writer = csv.writer(f_csv)
        csv_writer.writerow(["id", "name", "age", "department", "city", "salary", "is_active"])

        for i in range(num_nodes):
            node_id = f"p_{i:07d}"
            name = f"{random.choice(FIRST_NAMES)} {random.choice(LAST_NAMES)}"
            age = random.randint(21, 68)
            dept = random.choice(DEPARTMENTS)
            city = random.choice(CITIES)
            salary = round(random.uniform(65000.0, 250000.0), 2)
            is_active = random.random() > 0.1

            csv_writer.writerow([node_id, name, age, dept, city, salary, is_active])

            node_dict = {
                "id": node_id,
                "name": name,
                "age": age,
                "department": dept,
                "city": city,
                "salary": salary,
                "is_active": is_active,
            }
            f_jsonl.write(json.dumps(node_dict) + "\n")

    # 2. Generate Edges (KNOWS / COLLABORATES)
    edges_csv_path = os.path.join(output_dir, "edges_knows.csv")
    edges_jsonl_path = os.path.join(output_dir, "edges_knows.jsonl")

    print("  -> Writing edges...")
    with (
        open(edges_csv_path, "w", newline="", encoding="utf-8") as f_csv,
        open(edges_jsonl_path, "w", encoding="utf-8") as f_jsonl,
    ):
        csv_writer = csv.writer(f_csv)
        csv_writer.writerow(["source_id", "target_id", "weight", "since_year"])

        for _ in range(num_edges):
            src_idx = random.randint(0, num_nodes - 1)
            tgt_idx = random.randint(0, num_nodes - 1)
            while tgt_idx == src_idx and num_nodes > 1:
                tgt_idx = random.randint(0, num_nodes - 1)

            source_id = f"p_{src_idx:07d}"
            target_id = f"p_{tgt_idx:07d}"
            weight = round(random.uniform(0.1, 1.0), 3)
            since_year = random.randint(2010, 2026)

            csv_writer.writerow([source_id, target_id, weight, since_year])

            edge_dict = {
                "source_id": source_id,
                "target_id": target_id,
                "weight": weight,
                "since_year": since_year,
            }
            f_jsonl.write(json.dumps(edge_dict) + "\n")

    elapsed = time.time() - start_time
    print(f"  [OK] Successfully generated test dataset in {elapsed:.2f}s:")
    print(
        f"       - Nodes: {nodes_csv_path} ({os.path.getsize(nodes_csv_path) / 1024 / 1024:.2f} MB)"
    )
    print(
        f"       - Edges: {edges_csv_path} ({os.path.getsize(edges_csv_path) / 1024 / 1024:.2f} MB)"
    )


def main():
    parser = argparse.ArgumentParser(description="Voyager OGM Benchmark Dataset Generator")
    parser.add_argument(
        "--nodes", type=int, default=10000, help="Number of nodes to generate (default: 10,000)"
    )
    parser.add_argument(
        "--edges", type=int, default=50000, help="Number of edges to generate (default: 50,000)"
    )
    parser.add_argument(
        "--output-dir", type=str, default="test_data/bench_10k", help="Destination folder"
    )
    args = parser.parse_args()

    generate_dataset(args.nodes, args.edges, args.output_dir)


if __name__ == "__main__":
    main()
