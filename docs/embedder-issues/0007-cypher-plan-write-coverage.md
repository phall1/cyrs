# 0007 — `cypher-plan` write-side coverage parity with the embedder

**Severity:** medium
**Discovered:** plan/builder.rs survey

## Problem

The embedder's `PlanBuilder` (`plan/builder.rs`) covers a wide
write-side surface:

- `CREATE` node, `CREATE` relationship
- `MERGE` node (with `ON CREATE` / `ON MATCH`), `MERGE` relationship
- `SET` property, `SET` label, `SET` map (`+=` and `=`)
- `REMOVE` property, `REMOVE` label
- `DELETE` node, `DELETE` relationship, `DETACH DELETE`
- `FOREACH`
- `UNWIND` driving CREATE/MERGE
- Unique-key handling on MERGE for natural-keyed nodes

cyrs's `cypher-plan` exposes `WriteOp` (spec §12.1) but the public docs
don't enumerate which of these the lowering currently covers. Stage 2
of the migration needs that table to know what the embedder still has
to build itself.

## Proposed shape

A coverage matrix in `cyrs/docs/specs/0001-cypher-frontend.md §12.1`:

| Cypher construct | `WriteOp` variant | Status |
|------------------|-------------------|--------|
| `CREATE (n:L)` | `CreateNode` | ✓ |
| `MERGE (n:L {k: $k})` | `MergeNode` | partial — TODO unique-key |
| … | … | … |

Plus golden tests for each row.

## Why it matters for the embedder

If `cypher-plan` covers, say, 60% of the write surface the embedder
needs, then stage 2 splits into "use plan IR for reads + the covered
writes" and "keep the embedder's hand-rolled write planner for the
rest" — a clear staged migration. Today we don't know.
