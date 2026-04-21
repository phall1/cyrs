# TCK v1 Green-Tag Conformance Baseline

*Captured: 2026-04-21 against commit `9843c82` (cy-zjv).*

This file records the **current** conformance of the cyrs parser
against the vendored v1 openCypher TCK fixtures
(`crates/cypher-tck/tck/v1.toml`).  Future PRs that regress any of
the currently-passing scenarios will fail `cargo test -p cypher-tck`;
that test is the CI gate.  No separate `xtask check-tck` is needed —
the Rust test harness already enforces the invariant.

## Summary

| Metric | Value |
|---|---|
| Scenarios matching ≥1 v1 tag | **40** |
| Passing | **25** (63 %) |
| Ignored (known parser gaps) | **15** (38 %) |
| Failing | **0** |

Harness: `crates/cypher-tck/tests/harness.rs::tck_v1_scenarios`.
Run: `cargo test -p cypher-tck --test harness -- --nocapture`.

## What passes today

The parser handles the green-tag subset end-to-end for these clauses
and expressions (quick sample of scenarios that pass):

- `@MATCH` — simple node, labelled, property-match, relationship
  pattern, `OPTIONAL MATCH`; unclosed-paren negative case.
- `@RETURN` — literal int, arithmetic, multi-projection,
  `DISTINCT`, `ORDER BY SKIP LIMIT`; missing-expression negative.
- `@WHERE` — equality, `AND`, `OR`, `NOT`, `IS NULL`, `IS NOT NULL`.
- `@STRINGS` — literal, `STARTS WITH`, `ENDS WITH`, `CONTAINS`.
- `@EXPRESSIONS` — string, boolean, `null`.
- `@CALL-SUBQUERY` and `@LOAD-CSV` — v1 red; rejected with a parse
  error as required.

## Parser gaps (15 ignored scenarios)

Each ignored scenario in `v1.toml` carries an inline `note` pointing
at the missing grammar.  Consolidated list (one row per feature):

| Feature (tag) | Ignored scenarios | Parser status |
|---|---:|---|
| `@WITH` clause | 3 | not parsed |
| `@CREATE` clause | 1 | not parsed |
| `@MERGE` clause | 1 | not parsed |
| `@SET` clause | 1 | not parsed |
| `@REMOVE` clause | 1 | not parsed |
| `@DELETE` clause | 1 | not parsed |
| `@UNWIND` clause | 1 | not parsed |
| `@LISTS` — list literal expression | 1 | not parsed |
| `@MAPS` — map literal expression | 1 | not parsed |
| `@AGGREGATIONS` / function calls | 2 | not parsed (count/sum/etc) |
| `@PATTERNS` — variable-length paths (`*1..3`) | 1 | not parsed |
| `@PATTERNS` — named paths (`p = (…)`) | 1 | not parsed |

Each row is roughly "one grammar production + its AST wrappers +
lowering + sema dialect gate + snapshot tests."  Closing all of them
is the remaining parser work to call the v1 TCK conformant.  Tracked
together under **cy-3xz** (see beads).

## Re-capturing this baseline

Run the harness, eyeball the printed `TCK v1: X/Y` line, and update
the summary above.  The baseline is descriptive — the Rust test is
the authoritative gate.

```
cargo test -p cypher-tck --test harness -- --nocapture | grep "TCK v1:"
```
