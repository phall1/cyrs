# 0004 — `SchemaProvider` adapter ergonomics for embedders

**Severity:** high (blocks stage 2 sema migration)
**Discovered:** semantic.rs survey + cyrs-sema reading

## Problem

cyrs's `SchemaProvider` trait (cyrs spec §8) is the contract between the
front-end and the embedder's catalog. The embedder's catalog is a
`store::Schema` with node kinds, relationship kinds, property type
maps, and uniqueness/index hints.

To migrate the embedder's `semantic.rs` to `cyrs-sema`, the embedder
needs:

1. A worked example of implementing `SchemaProvider` against a
   pre-existing catalog type.
2. A trait shape stable enough that the embedder's adapter doesn't need
   churn on every cyrs minor release.
3. Clarity on **what semantic checks `cyrs-sema` runs** vs. what the
   embedder still has to do (the embedder has application-specific
   checks like "MERGE on unique-keyed node requires the unique key to
   be bound").

## Proposed shape

Add to cyrs:

- `cyrs-sema/examples/embedder_adapter.rs` — toy catalog + adapter
  implementing `SchemaProvider`, sema run, error reporting.
- `docs/specs/0001-cypher-frontend.md §8` — table listing every check
  cyrs-sema performs, so embedders know what they don't need to
  duplicate.
- A trait stability guarantee: SchemaProvider methods are SemVer-locked
  separately from the rest of cyrs-sema.

## Why it matters for the embedder

Without this, stage 2 sema migration is a guess-and-test exercise.
The embedder has ~40 distinct semantic checks today; we need to know
which of those cyrs-sema covers and which we'll keep on the embedder
side.
