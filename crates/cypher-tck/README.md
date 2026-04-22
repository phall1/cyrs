# cypher-tck

openCypher TCK conformance harness for the
[cyrs](https://github.com/phall1/cyrs) frontend.  See spec 0001 §17.5.

Two corpora feed into the harness:

1. **`tck/v1.toml`** — a hand-written, representative slice covering
   the v1 clause + expression surface.  Every scenario is classified
   per-bead and expected to stay green on every PR.
2. **`tck/full/`** — the upstream openCypher TCK vendored at tag
   `2024.3` (see [`tck/full/VENDORED.md`](tck/full/VENDORED.md) for the
   pinned commit).  220 `.feature` files, 1339 scenarios.  Runs only
   when the `full-tck` Cargo feature is enabled.

For the full story — architecture, dependency graph, and testing bar —
see the [repo-root README](https://github.com/phall1/cyrs#readme).

## Running the harness

### v1 slice (pre-commit gate)

```
cargo test -p cypher-tck
```

Must stay green on every PR.  Runs in milliseconds.

### Full vendored corpus (measurement-only baseline)

```
cargo test -p cypher-tck --features full-tck
# — or, equivalently —
cargo xtask tck-baseline
```

This scans every `.feature` file under `tck/full/features/`, expands
Scenario Outlines against their `Examples:` tables, runs each query
through `cypher-db`, and writes per-area parser-acceptance counts to
[`tck/full-baseline.md`](tck/full-baseline.md).  The test **never
fails** — it is a rolling measurement used for regression tracking,
not a CI gate.  See bead `cy-p5q` (spec §17.5) for the rationale.

### What's gated vs. measured

| Invocation                                    | Role                       | Gated by pre-commit? |
| --------------------------------------------- | -------------------------- | -------------------- |
| `cargo test -p cypher-tck`                    | v1 slice, must pass        | Yes (§17)            |
| `cargo test -p cypher-tck --features full-tck`| full-corpus baseline write | No                   |
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
