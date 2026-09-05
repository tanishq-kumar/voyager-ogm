# voyager-pyo3

Python PyO3 bindings and Apache Arrow PyCapsule bridge for Voyager OGM.

## Overview

`voyager-pyo3` exports native Rust AST structures and dialect query compilers to Python. It implements the standard Apache Arrow C Stream interface (`__arrow_c_stream__`) for zero-copy data exchange with Polars and PyArrow.
