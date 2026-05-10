# 0005 — Crate naming inconsistency: `cyrs-*` package, `cypher_*` lib

**Severity:** low (cosmetic, but annoying)
**Discovered:** stage 0 path-dep wiring

## Problem

Every crate in cyrs has split names:

| Directory | Package name | Lib name |
|-----------|-------------|----------|
| `crates/cypher-syntax/` | `cyrs-syntax` | `cypher_syntax` |
| `crates/cypher-ast/` | `cyrs-ast` | `cypher_ast` |
| `crates/cypher-hir/` | `cyrs-hir` | `cypher_hir` |
| … | … | … |
| `crates/cypher/` | `cyrs-lang` | `cypher` |

Embedders have to write:

```toml
cypher-syntax = { package = "cyrs-syntax", path = "..." }
```

…in every Cargo.toml that consumes cyrs. The `package = "..."` rename
form works but is an unforced friction point.

## Cause

Likely a crates.io squat: `cypher-*` namespace probably wasn't
available, so the project chose `cyrs-*` for publishing while keeping
the more pleasant `cypher_*` lib idents for code.

## Options

1. **Pick one and align.** Either rename packages to `cypher-*` (if the
   namespace is now obtainable, or via a `cypher-*` org/scope), or
   rename lib targets to `cyrs_*`. The first is preferable from a brand
   standpoint; the second eliminates the rename ceremony.

2. **Document the rename pattern.** Add an "Embedding cyrs" section to
   the cyrs README showing the `package = "..."` incantation so
   embedders don't have to re-derive it.

## Why it matters for the embedder

Cosmetic only — once written, the workspace deps work fine. Filed for
future maintainers when sorting out 1.0 publishing.
