# Embedder issues — feedback from an external embedder

These issues were discovered while migrating an external embedder off
its bespoke Cypher parser and onto cyrs. They are concrete papercuts
and missing pieces that an external embedder hits when integrating
cyrs as a Cypher front-end.

Each issue is a candidate for a real GitHub issue against this repo. The
file form keeps them grouped and reviewable as a set rather than as a
flurry of bug reports. The numeric IDs (`NNNN`) are stable and referenced
from `TODO(cyrs-issue-NNNN)` comments in the embedder codebase — keep the
IDs even if titles drift.

> **See also:** `../../feat-request.md` collects a *second* embedder's
> asks — pgGraph, a PostgreSQL extension integrating cyrs for a
> `graph.cypher()` function. Those use a separate `§N.M` numbering (a
> stable contract with pgGraph's own docs), not the `NNNN` scheme here.
> All 13 of them shipped in cyrs 0.1.0; a few overlap issues
> 0001/0003/0004/0005/0007/0008 and cross-link to them.

## Index

| ID | Severity | Title |
|----|----------|-------|
| 0001 | medium | `cyrs-hir` should expose a single-call `parse_to_hir` |
| 0002 | high | HIR walk traits matching the legacy AST visitor surface |
| 0003 | medium | Stable diagnostic codes need a typed enum, not `u16` |
| 0004 | high | `SchemaProvider` adapter for the embedder's catalog |
| 0005 | low | Crate naming inconsistency (`cyrs-*` package, `cypher_*` lib) |
| 0006 | medium | TCK `Expected` classification needs an embedder M23 subset |
| 0007 | medium | `cyrs-plan` write-side coverage parity with the embedder |
| 0008 | low | Span vs `TextRange` ergonomics for embedders |
| 0009 | high | Document the AST-vs-HIR consumption contract for embedders |
| 0010 | low | Owned HIR statement re-parses internally — redundant work |

Severity is from the **embedder's** perspective. For the cyrs project
itself, prioritisation may differ.
