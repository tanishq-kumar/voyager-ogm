# voyager-cli

[![crates.io](https://img.shields.io/crates/v/voyager-cli.svg)](https://crates.io/crates/voyager-cli)
[![crates.io downloads](https://img.shields.io/crates/d/voyager-cli.svg)](https://crates.io/crates/voyager-cli)
[![CI](https://github.com/tanishq-kumar/voyager/actions/workflows/ci.yml/badge.svg)](https://github.com/tanishq-kumar/voyager/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.87-blue.svg)](https://github.com/tanishq-kumar/voyager/blob/main/Cargo.toml)

The command-line interface for Voyager OGM. It installs the `voyager` binary (planned short alias: `voy`), which currently provides environment diagnostics and placeholders for query compilation and benchmarking.

## Install

```bash
cargo install voyager-cli
```

Check the installed version:

```bash
voyager --version
```

The binary is `voyager`. The short alias `voy` is planned but is not defined in this version.

## Usage

### `voyager doctor`

Prints the engine version, arena and dialect support, and an environment check:

```text
$ voyager doctor
󰓾 Voyager OGM v0.4.0-alpha.8
  Engine Core: voyager-ogm v0.4.0-alpha.8
  Memory Arena: Active (32-bit handles)
  Supported Dialects: openCypher 9, Cypher 25, SQL:2023 PGQ, ISO GQL 2024, DuckPGQ
  Environment: All diagnostics OK [PASS]
```

### `voyager compile` and `voyager bench`

These subcommands are early placeholders in `0.4.0-alpha`: they accept their arguments and echo a message, but do not compile or benchmark anything yet.

```bash
voyager compile --dialect cypher   # prints: Compiling query for dialect: cypher
voyager bench --nodes 100000       # prints: Running Voyager OGM benchmark for 100000 nodes...
```

For real compilation, use the [`voyager-ogm`](https://crates.io/crates/voyager-ogm) library API (or the Python package). The Rust examples under [`crates/voyager-ogm/examples`](https://github.com/tanishq-kumar/voyager/tree/main/crates/voyager-ogm/examples) show how to build and emit a query.

## MSRV

Rust **1.87.0** (the workspace `rust-version`).

## License

MIT OR Apache-2.0. See [LICENSE-MIT](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE-MIT) and [LICENSE-APACHE](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE-APACHE).

## Links

- Repository: <https://github.com/tanishq-kumar/voyager>
- Changelog: <https://github.com/tanishq-kumar/voyager/blob/main/CHANGELOG.md>
