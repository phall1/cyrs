# cypher-py

[![crates.io](https://img.shields.io/crates/v/cypher-py.svg)](https://crates.io/crates/cypher-py)
[![docs.rs](https://img.shields.io/docsrs/cypher-py)](https://docs.rs/cypher-py)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Python (PyO3) bindings for the Cyrs Cypher / GQL frontend. Exposes the
agent v1 op surface (parse, check, complete, hover, format, rewrite,
schema_set, schema_clear) as methods on a single `CypherDatabase`
Python class. Thin adapter over `cypher-lang-services` + `cypher-db` +
`cypher-diag` + `cypher-fmt`; no analysis logic lives here (spec 0004
§6).

## Install (release)

```bash
pip install cypher
```

Publishing to PyPI is deferred to the release playbook in `cy-zgz` —
until then, the wheels ship as GitHub release assets. See the
[wheel matrix](#wheel-matrix) below for what lands per platform.

## Develop locally

```bash
# Create a fresh virtualenv
python3 -m venv .venv
source .venv/bin/activate

# Install maturin + pytest
pip install --upgrade pip
pip install maturin pytest

# Build + install the extension module into the venv
maturin develop -m crates/cypher-py/Cargo.toml

# Run the pytest suite
pytest crates/cypher-py/tests/
```

`maturin develop` builds a debug-mode cdylib and installs it under
`.venv/lib/python*/site-packages/cypher/`; a fresh `import cypher`
resolves to the just-built module.

For a release build (stripped, optimised), use `maturin develop
--release` or `maturin build --release`.

## Python API shape

```python
import cypher

# Check wire-protocol compatibility — see spec 0004 §9.3.
if cypher.PROTO_VERSION != 1:
    raise RuntimeError("proto version mismatch")

db = cypher.CypherDatabase()

# Parse + type-check a query.  Clean input yields [].
diagnostics = db.check("MATCH (n) RETURN n")
for d in diagnostics:
    print(d.code, d.severity, d.message, d.range)

# Format to canonical form.
formatted = db.format("match (n) return n")
assert "MATCH" in formatted  # keywords are uppercased

# Completions at a cursor.
items = db.complete("MATCH (", offset=7)

# Hover at a cursor.
info = db.hover("MATCH (n) RETURN n", offset=7)

# Apply quick-fixes by id.
out = db.rewrite("MATCH (n) RETURN n", fix_ids=["cy-fix.uppercase"])

# Schema lifecycle.
db.schema_set('[[node]]\nlabel = "Person"\n')
db.schema_clear()
```

Each method mirrors the agent op of the same name; see spec 0001 §15
for the full wire-level input / output shape.

## Type stubs

`cypher.pyi` ships with the wheel (PEP 561) under
`crates/cypher-py/python/cypher/__init__.pyi`. Every PyO3-exported
class, method, and return-shape dict appears in the stub. A CI gate
runs `mypy --strict` against the built wheel to catch drift.

Enums over Rust `#[non_exhaustive]` types (diagnostic severity,
completion kind) are exposed as plain `str`, not closed `Enum`s — this
is intentional: downstream users can read the values but should not
match exhaustively on them, so a new variant in a minor release does
not break their code (spec 0004 §6.3).

## Wheel matrix

`maturin build --release` produces one abi3 wheel per platform, covering
CPython 3.10 → 3.13 via the stable ABI (spec 0004 §6.2):

| OS       | Architecture | Wheels per release | Notes                           |
| -------- | ------------ | ------------------ | ------------------------------- |
| Linux    | x86_64       | 1 (abi3)           | manylinux2014 container         |
| Linux    | aarch64      | 1 (abi3)           | manylinux2014 container         |
| macOS    | x86_64       | 1 (abi3)           | native runner                   |
| macOS    | aarch64      | 1 (abi3)           | Apple Silicon, native runner    |
| Windows  | x86_64       | 1 (abi3)           | native runner                   |

That is 5 wheels per release; the wheel-matrix workflow
(`.github/workflows/python-wheels.yml`) runs `pytest` against each
wheel × CPython version so regressions surface even though the ABI is
shared.

Per-wheel size budget: ≤ 5 MB (spec 0004 §6 acceptance gate).

### abi3 tradeoff

`abi3-py310` means one wheel supports every CPython from 3.10 onwards,
at the cost of newer `PyO3` / `CPython` features that require
version-specific ABI hooks (tagged `Py_3_11+` on the PyO3 side). The
adapter surface only uses stable-ABI-representable types (strings,
ints, dicts, lists, opaque `#[pyclass]` handles) so the tradeoff is
free for this crate.

## Running locally without maturin

For host-only tests that don't need the PyO3 boundary, `cargo test -p
cypher-py` exercises the plain-Rust helper paths (severity mapping,
completion-kind mapping, FileId interning) directly. `cargo build -p
cypher-py` fails at link on hosts without a Python toolchain (PyO3's
`extension-module` feature expects the wheel builder to link against
the caller's Python); this is expected — the wheel build runs in CI.

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
