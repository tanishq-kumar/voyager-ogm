# voyager-pyo3

> **Internal crate.** This crate is not published to crates.io (`publish = false`). It ships as the Python package **`voyager-ogm`** on PyPI.

Python PyO3 bindings and Apache Arrow PyCapsule bridge for Voyager OGM.

## Overview

`voyager-pyo3` exports the native Rust AST structures and dialect query compilers to Python. It implements the standard Apache Arrow C Stream interface (`__arrow_c_stream__`) for zero-copy data exchange with Polars and PyArrow.

## Install the Python package

```bash
pip install voyager-ogm
```

The PyPI project page is the [Python package README](https://github.com/tanishq-kumar/voyager/blob/main/packages/python/README.md); the main repository README covers the whole project.

## Development

Build the extension into the active virtual environment with maturin:

```bash
uv sync --all-extras
uv run --no-sync maturin develop
uv run --no-sync pytest -m "not live"
```

`just dev` rebuilds the extension (`maturin develop`) and `just test-python` runs the Python suite.

## License

MIT OR Apache-2.0. See [LICENSE-MIT](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE-MIT) and [LICENSE-APACHE](https://github.com/tanishq-kumar/voyager/blob/main/LICENSE-APACHE).
