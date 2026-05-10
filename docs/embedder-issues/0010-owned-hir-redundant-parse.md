# 0010 — `lower_statement(&str)` re-parses internally — redundant work

**Severity:** low (subset of 0001 but worth its own ticket if 0001 lands as a sugar-only fix)
**Discovered:** stage 0 cyrs_bridge implementation

## Problem

`cyrs_hir::lower::lower_statement(src: &str) -> Statement` takes a
`&str` and re-runs the syntax parse internally. Embedders that already
have a `Parse` (because they extracted syntax errors from it first) end
up paying the parse cost twice.

This is a special case of issue 0001 but lives separately because the
fix is independent — even without a `parse_to_hir` convenience, the
`&str`-taking lower function should be re-implementable as a thin
wrapper over a `lower_parse(&Parse)` primitive.

## Proposed shape

```rust
pub fn lower_parse(parse: &Parse) -> Statement;
pub fn lower_statement(src: &str) -> Statement {
    lower_parse(&cyrs_syntax::parse(src))
}
```

## Why it matters

Hot path in the embedder. Closing 0001 closes this; closing this
without 0001 still helps.
