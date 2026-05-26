# Concept: plan

Logical plan IR. The final layer in the front-end pipeline, and the
hand-off boundary to a downstream execution engine.

**Crate:** [`cyrs-plan`](../../crates/cyrs-plan).
**Spec section:** [0001 §3.5, §7](../specs/0001-cypher-frontend.md).

## What goes in, what comes out

| In | Out |
| -- | --- |
| Type-checked HIR (post-sema) | A logical operator tree: scans, joins, projections, aggregations, writes |

The plan describes **what** to do, not **how**. A downstream database
turns the logical plan into a physical plan (index choices, join
algorithms, parallelism) and executes it against its storage. cyrs
stops here — by design (spec §1.3 N1).

## Read vs. write

The plan IR covers both read and write clauses. The current
write-clause coverage matrix lives at
[`plan-write-coverage.md`](../plan-write-coverage.md) — which write
clauses lower today and which are intentionally deferred.

## When to reach for this layer

Choose `plan` when:

- The product is a **graph database**, an analytic engine, or any
  system that actually executes queries. The plan is the contract.
- A downstream tool wants to **explain** queries to users — `cypher
  explain` renders the plan as a tree.
- A tool wants to **measure** queries (cardinality, shape) without
  running them.

Tools that need only diagnostics (IDEs, CI gates, review surfaces)
should stop at [`sema`](./sema.md).

## Stability

The plan IR shape is **pre-1.0**. Operator kinds, field names, and the
schema of write operators may change in 0.x. The stability contract
is in [`stability.md`](../stability.md). Database authors planning to
ship cyrs-based execution should track the contract closely.

## Related

- Input layer: [`sema`](./sema.md).
- Plan rendering / explain: [`cyrs-cli`](../../crates/cyrs-cli)
  (`cypher explain`, `cypher plan`).
- Write coverage matrix: [`plan-write-coverage.md`](../plan-write-coverage.md).
