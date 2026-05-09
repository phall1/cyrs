# 0009 — Document the AST-vs-HIR consumption contract for embedders

**Severity:** high (orienting question for stage 2)
**Discovered:** module-tour reading

## Problem

cyrs has three plausible consumption layers for a downstream embedder:

1. **Typed AST** (`cypher-ast`) — rowan-backed wrappers, lossless,
   borrows from `Parse`. Embedder pays for tree walks but gets exact
   spans for diagnostics.

2. **HIR** (`cypher-hir`) — owned, name-resolved, desugared. Embedder
   gets a clean IR but loses the lossless tree (HIR keeps a HirId map
   back to syntax for diagnostics).

3. **Plan IR** (`cypher-plan`) — logical operator graph. Embedder
   skips writing its own planner.

cyrs's spec (0001) describes each layer's purpose internally but does
not state **which layer an embedder should consume**. The answer
probably depends on the embedder type:

- IDE / linter → AST (preserves trivia for refactor)
- Query engine → HIR or Plan (don't need trivia, want resolution)
- Pretty-printer → AST (fmt is CST-driven anyway)

But that's reasoning, not documentation.

## Proposed shape

A new section in cyrs's spec or README:
**"Choosing your integration depth."**

A decision table:

| Embedder kind | Recommended layer | Rationale |
|---------------|-------------------|-----------|
| Graph database | HIR + Plan | Skip parser internals; reuse plan IR if write coverage suffices |
| IDE / LSP-host | CST + AST | Lossless tree for refactors, trivia preservation |
| Static analysis tool | AST + sema | Diagnostic spans are the product |
| Query rewriter | HIR | Resolved names; emit fresh HIR after rewrite |
| Just a parser bench | Parse only | Don't pay for HIR if you don't need it |

## Why it matters for the embedder

Right now we're guessing. Stage 2 of the migration could go either way
(walk HIR, or walk Plan) and we'll cement that decision based on
whichever feels less awful at the time. A documented recommendation
from cyrs would make the call principled.
