# Concept: HIR

High-level Intermediate Representation. The first layer where the query
is described in terms of its *meaning* rather than its *text*. Names
are resolved, scopes are explicit, and desugaring has fired.

**Crate:** [`cyrs-hir`](../../crates/cyrs-hir).
**Spec section:** [0001 §3.3, §5](../specs/0001-cypher-frontend.md).

## What goes in, what comes out

| In | Out |
| -- | --- |
| Typed AST from [`cyrs-ast`](../../crates/cyrs-ast) | Resolved HIR tree + scope graph; each variable use is linked to its binding site |

## What HIR adds over AST

- **Name resolution.** Every variable reference resolves to the binding
  introduced by `MATCH`, `WITH`, `UNWIND`, pattern parts, list
  comprehensions, or function parameters. Unresolved names surface as
  diagnostics.
- **Scope graph.** Cypher and GQL have several scope-introducing
  constructs that are not lexical (`WITH` is a re-projection, `UNWIND`
  flattens, aggregations capture pre-projection names). HIR represents
  these as edges so downstream code does not re-derive them.
- **Desugaring.** Forms that mean the same thing reduce to one canonical
  shape (label-expression normalisation, pattern flattening, anonymous
  binders). Sema and Plan consume the canonical forms.

## When to reach for this layer

Choose `hir` when:

- You are writing a **query rewriter** (e.g. predicate push-down,
  variable inlining) and want resolved names without committing to a
  full type system.
- You are running **structural analysis** that needs scopes — for
  example, "find every variable that escapes its `WITH` boundary."
- You are building a downstream IR and want a stable, name-resolved
  input that you do not have to re-resolve.

Reach for [`sema`](./sema.md) when you also need types or schema
validation; reach for [`syntax`](./syntax.md) when you need to preserve
exact source layout.

## Stability

The HIR shape is **pre-1.0**. Schema for the tree (node kinds, scope
edge labels) may change in 0.x. The stability contract for HIR is
listed in [`stability.md`](../stability.md). Consumers that pin to a
0.x version are not surprised; consumers that want HIR as a contract
surface should wait for 1.0 or vendor a snapshot.

## Related

- Source-text layer: [`syntax`](./syntax.md).
- Next layer down (uses HIR as input): [`sema`](./sema.md).
- Lowering target after sema: [`plan`](./plan.md).
