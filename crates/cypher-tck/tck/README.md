# TCK Fixture Directory — `crates/cypher-tck/tck/`

This directory holds the openCypher TCK inputs the `cypher-tck`
integration tests run against.  See spec 0001 §17.5.

## Layout

| Path                | Purpose                                                    |
| ------------------- | ---------------------------------------------------------- |
| `v1.toml`           | Hand-written v1 slice — pre-commit gate input.             |
| `v1-baseline.md`    | Descriptive snapshot of the v1 slice pass state.           |
| `embedder-m23.toml` | Curated M23 fundamentals — pre-commit gate input (cy-emb6).|
| `full/`             | Vendored upstream openCypher TCK (opt-in via `full-tck`).  |
| `full/VENDORED.md`  | Upstream pin + refresh procedure for `full/`.              |
| `full-baseline.md`  | Auto-generated pass-rate snapshot for the full corpus.     |

The on-disk shape of `embedder-m23.toml` is identical to `v1.toml`;
see the file's header comment for the add-only ratchet policy.

## v1 slice format (`v1.toml`)

Each fixture file is a TOML document containing an array of `[[scenario]]`
tables.

### Scenario fields

| Field     | Type              | Required | Description |
|-----------|-------------------|----------|-------------|
| `name`    | string            | yes      | Human-readable scenario name (must be unique within the file). |
| `tags`    | string array      | yes      | One or more openCypher TCK tags (e.g. `["@MATCH", "@WHERE"]`).  The harness filters to scenarios that share at least one tag with `cypher_tck::v1_tags()`. |
| `query`   | string            | yes      | The Cypher source text to parse. |
| `outcome` | `"ok"` or `"error"` | yes    | Expected parse outcome.  Mapped at load time to `Expected::Supported` or `Expected::Error`. |
| `ignore`  | bool (default `false`) | no  | When `true`, the scenario is skipped (maps to `Expected::Ignored`).  Always pair with a `note`. |
| `note`    | string            | no       | Free-text explanation; required when `ignore = true`.  State which parser feature is missing and suggest a bead title. |

The on-disk `outcome`/`ignore` shape is kept stable for
backward-compat with `xtask tree-sitter-parity`; harness code works in
terms of the richer `Expected` enum (bead cy-p5q).

### Example

```toml
[[scenario]]
name    = "MATCH simple node pattern"
tags    = ["@MATCH"]
query   = "MATCH (n) RETURN n"
outcome = "ok"

[[scenario]]
name    = "MATCH unclosed paren is a parse error"
tags    = ["@MATCH"]
query   = "MATCH (n RETURN n"
outcome = "error"

[[scenario]]
name    = "WITH clause (ignored — parser gap)"
tags    = ["@WITH"]
query   = "MATCH (n) WITH n RETURN n"
outcome = "ok"
ignore  = true
note    = "Parser does not yet parse WITH clause; file bead: 'WITH clause grammar rule missing'"
```

## Adding new v1 scenarios

1. Add a `[[scenario]]` block to `v1.toml`.
2. Choose tags from `cypher_tck::v1_tags()` (see
   `crates/cypher-tck/src/lib.rs`).
3. Set `outcome = "ok"` for positive tests (parser must accept) and
   `outcome = "error"` for negative tests (parser must reject).
4. Run `cargo test -p cypher-tck` to confirm the harness picks up the
   new scenario.

If a scenario fails due to a known parser gap, set `ignore = true` and
add a `note` pointing to the recommended bead title.

## Full vendored upstream (`full/`)

See `full/VENDORED.md` for the pinned upstream commit and the
refresh procedure.  Do not hand-edit files under `full/` — upstream
changes should go through a TCK PR first.

### v1 tag coverage

The following tags are targeted for v1 (all scenarios under the tag
must pass):

`@MATCH`, `@OPTIONAL-MATCH`, `@WHERE`, `@RETURN`, `@WITH`, `@UNWIND`,
`@CREATE`, `@MERGE`, `@SET`, `@REMOVE`, `@DELETE`, `@EXPRESSIONS`,
`@AGGREGATIONS`, `@STRINGS`, `@LISTS`, `@MAPS`, `@PATTERNS`, `@NULL`

These tags must produce a parse error (not silently accept):

`@CALL-SUBQUERY`, `@EXISTS-SUBQUERY`, `@LOAD-CSV`
