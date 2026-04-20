# TCK Fixture Directory — `crates/cypher-tck/tck/`

This directory holds hand-written scenario fixtures that stand in for the
full openCypher TCK corpus (spec 0001 §17.5).  Network access is restricted
in the v1 environment, so the TCK corpus is **not** downloaded; instead, a
representative slice of 40+ scenarios is vendored here inline.

## File format

Each fixture file is a TOML document containing an array of `[[scenario]]`
tables.  The current fixture is `v1.toml`.

### Scenario fields

| Field     | Type              | Required | Description |
|-----------|-------------------|----------|-------------|
| `name`    | string            | yes      | Human-readable scenario name (must be unique within the file). |
| `tags`    | string array      | yes      | One or more openCypher TCK tags (e.g. `["@MATCH", "@WHERE"]`).  The harness filters to scenarios that share at least one tag with the v1 gate set. |
| `query`   | string            | yes      | The Cypher source text to parse. |
| `outcome` | `"ok"` or `"error"` | yes    | Expected parse outcome.  `"ok"` means the parser must accept the input with no syntax errors; `"error"` means it must emit at least one. |
| `ignore`  | bool (default `false`) | no  | When `true`, the scenario is skipped rather than run.  Use this to acknowledge a known parser bug.  Always pair with a `note`. |
| `note`    | string            | no       | Free-text explanation; required when `ignore = true`.  State which parser feature is missing and suggest a bead title. |

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

## Adding new scenarios

1. Add a `[[scenario]]` block to `v1.toml` (or a new `*.toml` file if
   creating a new fixture set).
2. Choose tags from the v1 gate list in `cypher_tck::v1_gates()` (see
   `crates/cypher-tck/src/lib.rs`).
3. Set `outcome = "ok"` for positive tests (parser must accept) and
   `outcome = "error"` for negative tests (parser must reject).
4. Run `cargo test -p cypher-tck` to confirm the harness picks up the
   new scenario.

If a scenario fails due to a known parser gap, set `ignore = true` and
add a `note` pointing to the recommended bead title.

## Syncing with upstream openCypher TCK (future)

When network access is restored, the `cargo xtask tck-fetch` command (to
be implemented) will:

1. Clone/update the openCypher TCK repository.
2. Parse the Gherkin `.feature` files under `tck/features/clauses/`.
3. Emit a `tck/generated.toml` alongside the hand-written `v1.toml`.
4. The harness will load both files and de-duplicate by scenario name.

Until that lands, the hand-written `v1.toml` is the single source of truth
for TCK conformance coverage.

## v1 tag coverage

The following tags are targeted for v1 green status (all scenarios under
the tag must pass):

`@MATCH`, `@OPTIONAL-MATCH`, `@WHERE`, `@RETURN`, `@WITH`, `@UNWIND`,
`@CREATE`, `@MERGE`, `@SET`, `@REMOVE`, `@DELETE`, `@EXPRESSIONS`,
`@AGGREGATIONS`, `@STRINGS`, `@LISTS`, `@MAPS`, `@PATTERNS`, `@NULL`

The following tags are v1 red (must produce a diagnostic, not silently
parse):

`@CALL-SUBQUERY`, `@EXISTS-SUBQUERY`, `@LOAD-CSV`
