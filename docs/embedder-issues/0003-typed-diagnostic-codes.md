# 0003 — Stable diagnostic codes need a typed enum, not `u16`

**Severity:** medium
**Discovered:** cyrs_bridge.rs error mapping

## Problem

`cypher_syntax::SyntaxError::code` is a `u16` whose values match the
`DiagCode` discriminants in `cypher-diag`. Embedders mapping errors to
their own typed errors (e.g. `embedder::error::ErrorKind`) need to
write a giant `match err.code { 1 => …, 3 => …, … }` against magic
numbers.

This is a SemVer hazard: if cyrs renumbers a code, every embedder
silently regresses to "unknown error → generic fallback."

## Proposed shape

Re-export `DiagCode` from `cypher-syntax` (or add a thin `code_enum()`
method on `SyntaxError` that returns the typed enum), so embedders
match on names:

```rust
match err.code_enum() {
    DiagCode::E0001UnexpectedToken => ErrorKind::Syntax,
    DiagCode::E0003UnterminatedString => ErrorKind::Lexer,
    …
}
```

## Why it matters for the embedder

The embedder's `error::ErrorKind` is the surface its HTTP API returns
to clients. Mapping cyrs codes to embedder codes through magic numbers
is brittle and silently rots.
