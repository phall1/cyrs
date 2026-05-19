# cyrs-tck

[![crates.io](https://img.shields.io/crates/v/cyrs-tck.svg)](https://crates.io/crates/cyrs-tck)
[![docs.rs](https://img.shields.io/docsrs/cyrs-tck)](https://docs.rs/cyrs-tck)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

openCypher TCK + GQL ISO 39075 conformance harness for the
[cyrs](https://github.com/phall1/cyrs) frontend.  See spec 0001 §17.5.

Five corpora feed into the harness:

1. **`tck/v1.toml`** — a hand-written, representative slice covering
   the v1 clause + expression surface.  Every scenario is classified
   per-bead and expected to stay green on every PR.
2. **`tck/embedder-m23.toml`** — a curated subset of M23 fundamentals
   (bead cy-emb6, embedder-issue 0006).  Pre-commit gated alongside
   `v1.toml`; embedders pinning the openCypher M23 corpus extend this
   file with the scenarios their legacy parser passes.  Add-only:
   regressions are parser bugs, never silenced.
3. **`tck/full/`** — the upstream openCypher TCK vendored at tag
   `2024.3` (see [`tck/full/VENDORED.md`](tck/full/VENDORED.md) for the
   pinned commit).  220 `.feature` files, 1339 scenarios.  Runs only
   when the `full-tck` Cargo feature is enabled.
4. **`tck/gql-iso-39075/`** — a hand-authored bootstrap corpus
   exercising the GQL-distinct surface of ISO/IEC 39075:2024
   (bead cy-0hj).  Each scenario carries an inline ISO §-citation;
   compliance is reported separately from the openCypher badge in
   [`tck/gql-iso-39075/baseline.md`](tck/gql-iso-39075/baseline.md).
   Runs only when the `gql-iso` Cargo feature is enabled.
5. **`tck/opengql-samples/`** — the 14 official sample queries
   published by the OpenGQL project alongside their ANTLR4 grammar for
   ISO/IEC 39075:2024 (bead cy-qsze).  Independent of the hand-authored
   `gql-iso-39075` bootstrap — these come from the body publishing the
   grammar.  Pinned upstream commit lives in
   [`tck/opengql-samples/VENDORED.md`](tck/opengql-samples/VENDORED.md);
   rolling acceptance is recorded in
   [`tck/opengql-samples/baseline.md`](tck/opengql-samples/baseline.md).
   Runs only when the `opengql-samples` Cargo feature is enabled.

For the full story — architecture, dependency graph, and testing bar —
see the [repo-root README](https://github.com/phall1/cyrs#readme).

## Running the harness

### v1 slice (pre-commit gate)

```
cargo test -p cyrs-tck
```

Must stay green on every PR.  Runs in milliseconds.

### Full vendored corpus (measurement-only baseline)

```
cargo test -p cyrs-tck --features full-tck
# — or, equivalently —
cargo xtask tck-baseline
```

This scans every `.feature` file under `tck/full/features/`, expands
Scenario Outlines against their `Examples:` tables, runs each query
through `cyrs-db`, and writes per-area parser-acceptance counts to
[`tck/full-baseline.md`](tck/full-baseline.md).  The test **never
fails** — it is a rolling measurement used for regression tracking,
not a CI gate.  See bead `cy-p5q` (spec §17.5) for the rationale.

### What's gated vs. measured

| Invocation                                    | Role                       | Gated by pre-commit? |
| --------------------------------------------- | -------------------------- | -------------------- |
| `cargo test -p cyrs-tck`                    | v1 slice, must pass        | Yes (§17)            |
| `cargo test -p cyrs-tck --features full-tck`| full-corpus baseline write | No                   |
| `cargo test -p cyrs-tck --features gql-iso` | GQL-ISO bootstrap baseline | No                   |
| `cargo test -p cyrs-tck --features opengql-samples` | OpenGQL upstream samples baseline | No |
| `cargo xtask tck-baseline`                    | convenience wrapper        | No                   |

The full corpus is intentionally kept out of the default pre-commit
gate: ~20 % of scenarios currently fail because they cover constructs
outside the v1 spec surface (existential subqueries, `CALL { ... }`
subqueries, etc.).  Triage happens one area at a time — see `Next
steps` in the baseline file.

## Scenario classification

Each scenario carries a per-scenario [`Expected`] outcome
(bead cy-p5q, spec §17.5):

- `Supported` — parser must accept the query (zero syntax errors).
- `Error`     — parser must reject the query.
- `Ignored`   — scenario is acknowledged but skipped.

The v1.toml fixture still uses `outcome = "ok" | "error"` + an optional
`ignore = true` boolean on disk (for backward-compat with
`xtask tree-sitter-parity`); the harness maps that shape onto
`Expected` at load time.  Vendored scenarios default to `Ignored`
until a human triages them.

## Refreshing the vendored upstream

See [`tck/full/VENDORED.md`](tck/full/VENDORED.md) for step-by-step
instructions.  Re-run `cargo xtask tck-baseline` after every refresh
and commit the updated baseline alongside the corpus bump.

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.

The vendored openCypher TCK under `tck/full/` is Apache-2.0; its
upstream `LICENSE` and `NOTICE` files are preserved in that directory.
