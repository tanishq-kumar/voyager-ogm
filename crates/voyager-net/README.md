# voyager-net

**Native Asynchronous Network Engine, Connection Pooling & Wire Protocols for Voyager OGM**

`voyager-net` is a pure safe Rust crate providing an asynchronous network engine built on top of [Tokio](https://tokio.rs). It manages connection pools, protocol handshakes, and direct wire-to-Arrow record batch streaming.

## Features

- **Asynchronous Connection Pooling:** Thread-safe checkout, RAII connection leasing, heartbeat liveness probing, and idle eviction.
- **Wire Protocols:**
  - Bolt Protocol v4.4, v5.0, v5.4+ (Neo4j & Memgraph)
  - PostgreSQL Frontend/Backend Protocol v3.0 (Apache AGE & PostgreSQL 19)
  - Redis RESP2 / RESP3 (FalkorDB)
  - In-process C-ABI (DuckDB / DuckPGQ)
- **Direct Wire-to-Arrow Streaming:** Parses binary wire socket packets directly into Apache Arrow columnar record batches with zero intermediate allocations.
- **Zero Python GIL Contention:** Releases the Python GIL completely during network socket I/O.
