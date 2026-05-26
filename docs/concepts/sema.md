# Concept: sema

Semantic analysis. The layer that turns a name-resolved HIR into
**diagnostics**: type errors, schema violations, dialect-gating errors,
and lint warnings.

**Crates:** [`cyrs-sema`](../../crates/cyrs-sema),
[`cyrs-schema`](../../crates/cyrs-schema),
[`cyrs-diag`](../../crates/cyrs-diag).
**Spec section:** [0001 §3.4, §6, §10](../specs/0001-cypher-frontend.md).

## What goes in, what comes out

| In | Out |
| -- | --- |
| HIR tree + a `SchemaProvider` implementation | A list of `Diagnostic`s (errors + lints + notes), each carrying a stable code, a primary span, and optional fix hints |

## Schema is a trait, not a file

cyrs has no hard-coded schema. The `SchemaProvider` trait
([`cyrs-schema`](../../crates/cyrs-schema)) is the boundary: a consumer
implements it against whatever schema source it has (a `schema.toml`,
a live database catalog, a JSON blob). Sema queries the trait for
labels, relationship types, property keys, parameter types, and
function signatures.

`schema.toml` is the canonical *file format* for the trait — spec
[0002](../specs/0002-schema-file-format.md) defines its grammar, types,
validation rules, and structural diff. It is one possible
`SchemaProvider`; a consumer can ship its own.

## Diagnostic codes

Every diagnostic carries a stable code. Codes are **SemVer**: once
assigned, meaning does not change. The catalogue is in
[0001 §10](../specs/0001-cypher-frontend.md); the family ranges:

- `E0xxx` — parse errors (emitted by `syntax`, surfaced here).
- `E1xxx` … `E3xxx` — semantic / schema errors.
- `E4xxx` — dialect-gating (Cypher-only vs. GQL-only constructs).
- `W6xxx` — lints (warning severity, opt-in).
- `N8xxx` — notes and fix hints (attached to a parent diagnostic).

## Lints

Beyond hard semantic checks, sema ships a **clippy-equivalent lint pack**
— warning-severity diagnostics for queries that pass type-checking but
look stylistically poor or smell like a bug. Lints are opt-in and never
change the exit code of `cypher check`. Catalogue:
[`lints.md`](../lints.md).

Lints live in
[`crates/cyrs-sema/src/lints/`](../../crates/cyrs-sema/src/lints).

## When to reach for this layer

Choose `sema` when:

- The product is **diagnostics** — IDEs, CI gates, query review tools.
- An embedder needs to validate queries against its schema before
  storing or running them.
- A downstream system wants to refuse known-bad queries early and
  cheaply.

Embedders that only need to *execute* a query (and trust validation
done upstream) can skip sema and consume [`plan`](./plan.md) directly,
though the more common pattern is to run sema as a guardrail before
planning.

## Related

- Input layer: [`hir`](./hir.md).
- Next layer down: [`plan`](./plan.md).
- Diagnostic rendering backends (terminal, JSON, LSP): [`cyrs-diag`](../../crates/cyrs-diag).
- Schema file format: [spec 0002](../specs/0002-schema-file-format.md).
