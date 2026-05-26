# docs

Design documents and specs that outlive any single PR or conversation.
Organised by altitude: top entries are the briefest, deeper entries
the most normative.

## Entry points

- [`overview.md`](./overview.md) — what cyrs is and what each pipeline
  layer does, in plain words. Start here.
- [`integration-depth.md`](./integration-depth.md) — decision table
  for embedders choosing which layer to consume.
- [`crates.md`](./crates.md) — crate-by-crate index.

## Concept guides

Mid-altitude explanations of each pipeline layer. Each page covers
purpose, inputs and outputs, when to reach for the layer, and links
to the relevant spec section and crate.

- [`concepts/syntax.md`](./concepts/syntax.md) — lossless CST + recovering parser.
- [`concepts/hir.md`](./concepts/hir.md) — name-resolved IR.
- [`concepts/sema.md`](./concepts/sema.md) — schema-aware analysis + diagnostics.
- [`concepts/plan.md`](./concepts/plan.md) — logical plan IR.
- [`concepts/services.md`](./concepts/services.md) — incremental DB, LSP, agent API, CLI.

## Reference

- [`coverage.md`](./coverage.md) — TCK acceptance numbers and what they mean.
- [`lints.md`](./lints.md) — clippy-equivalent lint catalogue.
- [`stability.md`](./stability.md) — surface-by-surface stability contract.
- [`sema-checks.md`](./sema-checks.md) — semantic-check catalogue.
- [`plan-write-coverage.md`](./plan-write-coverage.md) — write-clause lowering matrix.
- [`tree-sitter.md`](./tree-sitter.md) — editor grammar setup + parity contract.

## Operations

- [`development.md`](./development.md) — local dev, tests, hook install.
- [`release-playbook.md`](./release-playbook.md) — release workflow.
- [`fuzz-runbook.md`](./fuzz-runbook.md) — fuzzing setup and corpus.
- [`bead-seed.md`](./bead-seed.md) — bead-system conventions.
- [`embedder-issues/`](./embedder-issues) — open-issue tracker for embedder concerns.

## Specs

Normative architecture commitments. Numbered, dated, owned. A spec's
**Status** moves Proposed → Accepted → Implemented → Superseded; an
accepted spec is not edited in place to reflect implementation drift,
a superseding spec is written instead.

- [`specs/0001-cypher-frontend.md`](./specs/0001-cypher-frontend.md) —
  architecture of the standalone Rust Cypher front-end: crate graph,
  syntax / AST / HIR / sema / plan layers, schema-provider trait,
  dialect matrix, incremental DB, LSP / agent / CLI, testing bar.
- [`specs/0002-schema-file-format.md`](./specs/0002-schema-file-format.md) —
  `schema.toml` file format: grammar, types, labels, rel types,
  parameters, validation (`E3010` / `E3011` / `W6010`), structural diff.
- [`specs/0003-project-manifest.md`](./specs/0003-project-manifest.md) —
  `cypher-project.toml` workspace manifest: members, dialect defaults,
  lint levels, schema wiring.
- [`specs/0004-interop-surfaces.md`](./specs/0004-interop-surfaces.md) —
  Interop: `cyrs-wasm` (WASM + Monaco), `cyrs-ffi` (stable C ABI +
  cbindgen), `cyrs-py` (PyO3 wheel), LSP-Web transport, tree-sitter
  parity.

## Conventions

- Specs are numbered sequentially (`NNNN-topic-slug.md`) and never
  renumbered.
- "Deferred" sections inside a spec are intentionally omitted, not
  forgotten. They graduate into the main body through a new spec
  revision when the work lands.
- Cross-references from one spec to another use the spec number, not
  the link path (e.g. `see §3.6 of spec 0001`): the number is stable.
