# Overview

cyrs is a **compiler front-end** for Cypher and GQL — the parts of a
query-language toolchain that come *before* execution: lex, parse,
resolve names, type-check, lint, format, and lower the query into a
logical plan. Execution against storage is delegated to the consumer.

The shape mirrors a traditional language compiler (rustc, clang, swiftc):
source text moves through progressively richer intermediate
representations, each layer adding information and discarding text-level
noise. A database author embeds the layers it needs and ignores the rest.

## Pipeline

```
  Cypher / GQL source text
        │
        ▼
  cyrs-syntax        lexer + recovering parser → lossless CST
        │
        ▼
  cyrs-ast           typed wrappers over the CST
        │
        ▼
  cyrs-hir           lowered HIR + name resolution + scope graph
        │
        ▼
  cyrs-sema          type system, schema-aware analysis, lints
        │
        ▼
  cyrs-plan          logical read / write plan IR
        │
        ▼
  consumer           executes the plan against its own storage
```

Each arrow is a one-way data transformation. The full crate graph and
allowed-edges list is normative in
[`specs/0001-cypher-frontend.md`](./specs/0001-cypher-frontend.md) §3.

## What each layer is for

Each layer has its own concept page; the summaries below are the
30-second version.

- **[`syntax`](./concepts/syntax.md)** — the lossless tree. Every byte of
  the source survives, including whitespace and comments. Bad tokens
  produce error nodes rather than cascading failures. Reach for it when
  the source text matters (editors, refactors, format-on-save).
- **[`hir`](./concepts/hir.md)** — names resolved, scopes built. Each
  variable knows where it was bound; `WITH`, `UNWIND`, and aggregations
  produce scope edges. Reach for it when you want to reason about a
  query semantically without paying for full type-checking.
- **[`sema`](./concepts/sema.md)** — schema-aware type system and
  analysis. Type errors, undefined labels, lint warnings, fix hints.
  Reach for it when you need diagnostics or static guarantees against a
  schema.
- **[`plan`](./concepts/plan.md)** — logical plan IR for execution.
  Operator trees (scans, joins, projections, writes) that a downstream
  database turns into a physical plan. Reach for it when you are
  building a database.
- **[`services`](./concepts/services.md)** — `cyrs-db` (Salsa-backed
  incremental analysis), `cyrs-lsp` (language server), `cyrs-agent`
  (JSON-over-stdio). Shared engine layer that batches everything above
  into IDE-ready and agent-ready surfaces.

## Glossary

Compiler terms used throughout the docs and the code:

| Term | Plain-words gloss |
| ---- | ----------------- |
| **CST** | Concrete Syntax Tree — every character of the source, including whitespace and trivia. |
| **AST** | Abstract Syntax Tree — typed wrappers over the CST that strip layout but preserve structure. |
| **HIR** | High-level Intermediate Representation — after name resolution and desugaring; closer to meaning than to text. |
| **Sema** | Semantic analysis — type checking, schema validation, diagnostics. |
| **Plan IR** | Logical operator tree (the plan a database turns into a physical execution plan). |
| **TCK** | Technology Compatibility Kit — the upstream openCypher conformance test suite; analogously the bootstrap corpus for GQL. |
| **Lossless / round-trip** | "Lossless" means no input bytes are dropped; "round-trip" means the output can be re-parsed and yields the same tree. |
| **Recovering parser** | A parser that synthesises error nodes on bad input and keeps going, so one typo does not blank the whole tree. |
| **Schema-aware** | Behaviour conditioned on a schema description (labels, types, properties) supplied by the embedder via the `SchemaProvider` trait. |

## Dialect routing

The parser emits the same CST for Cypher and GQL. Dialect selection
happens at the analysis layer through the
[`DialectMode`](../crates/cyrs-db/src/lib.rs) selector, which gates
GQL-only and Cypher-only constructs behind `E4xxx` diagnostics. The
mapping table lives at
[`crates/cyrs-sema/src/dialect.rs`](../crates/cyrs-sema/src/dialect.rs).

## Diagnostic codes

Diagnostics carry stable codes that never change meaning once assigned:

- `E0xxx` — parse errors
- `E1xxx` … `E3xxx` — semantic and schema errors
- `E4xxx` — dialect gating
- `W6xxx` — lints (warning severity, opt-in)
- `N8xxx` — notes and fix hints

Full registry: [`specs/0001-cypher-frontend.md`](./specs/0001-cypher-frontend.md) §10.

## Non-goals

cyrs is deliberately narrow. The following live outside the workspace by
design (spec 0001 §1.3, §9.3):

- **No execution engine, runtime, or storage.** The plan IR is the
  hand-off boundary.
- **No domain concepts.** The workspace contains no application
  vocabulary; CI greps to enforce this.
- **No overlay crate host.** Domain extensions plug in via the
  `SchemaProvider` trait and live in the consumer's repository.
- **No `Neo4jCurrent`-specific surface.** No APOC, `EXISTS {}`
  subqueries, `CALL { … }`, `LOAD CSV`, `SHOW`, or `CYPHER` prefixes.

## Where to go next

- Choosing a layer to depend on: [`integration-depth.md`](./integration-depth.md).
- Conformance numbers and what they mean: [`coverage.md`](./coverage.md).
- Lint catalogue: [`lints.md`](./lints.md).
- Crate-by-crate index: [`crates.md`](./crates.md).
- Normative architecture: [`specs/0001-cypher-frontend.md`](./specs/0001-cypher-frontend.md).
