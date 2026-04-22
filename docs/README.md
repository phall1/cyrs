# cypher / docs

Everything above the source tree. Start here after the top-level
[`README.md`](../README.md).

## Getting started

- [`getting-started.md`](./getting-started.md) — 5-minute tour: install,
  first query, project mode, LSP setup, agent JSON.
- [`interop.md`](./interop.md) — WASM, C FFI, Python, tree-sitter, LSP-Web.
  Which binding fits which language.

## Architecture

- [`architecture.md`](./architecture.md) — crate graph, layer map,
  incrementality model, invariants.
- [`diagnostics.md`](./diagnostics.md) — reference for all 120 registered
  diagnostic codes.
- [`performance.md`](./performance.md) — benchmarks, methodology,
  regression policy.
- [`stability.md`](./stability.md) — surface-by-surface SemVer contract.

## Operations

- [`release-playbook.md`](./release-playbook.md) — how to cut a release.
- [`fuzz-runbook.md`](./fuzz-runbook.md) — fuzz triage + OSS-Fuzz
  scaffolding.
- [`bead-seed.md`](./bead-seed.md) — new-bead template.

## Specs

Normative architecture commitments. Numbered, dated, own-able. Read
top to bottom when a spec is relevant.

- [`specs/0001-cypher-frontend.md`](specs/0001-cypher-frontend.md) —
  Architecture of the standalone Rust Cypher front-end: crate graph,
  syntax/AST/HIR/sema/plan layers, schema-provider trait, dialect
  matrix, incremental DB, LSP/agent/CLI, and testing at rustc grade.
- [`specs/0002-schema-file-format.md`](specs/0002-schema-file-format.md) —
  `schema.toml` file format: TOML grammar, types, labels, rel types,
  parameters, validation rules, linter (`E3010` / `E3011` / `W6010`),
  and structural diff.
- [`specs/0003-project-manifest.md`](specs/0003-project-manifest.md) —
  `cypher-project.toml` workspace manifest: members, dialect defaults,
  lint levels, schema wiring.
- [`specs/0004-interop-surfaces.md`](specs/0004-interop-surfaces.md) —
  Interop surfaces: `cypher-wasm` (WASM + Monaco), `cypher-ffi`
  (stable C ABI + cbindgen), `cypher-py` (PyO3 wheel), LSP-Web
  transport, and the tree-sitter parity contract; stability
  commitments per surface.

### Spec conventions

- Specs are numbered sequentially (`NNNN-topic-slug.md`) and never
  renumbered.
- A spec's **Status** moves Proposed → Accepted → Implemented →
  Superseded. Do not edit an accepted spec in place to reflect
  implementation drift; write a superseding spec.
- "Deferred" sections in a spec mean intentionally omitted, not
  forgotten. Add to them freely; graduate them into the main body
  with a new spec revision when we build them.
- Reference existing specs from new specs by number (`see §3.6 of
  spec 0001`) rather than by link; the number is stable.

## Planning

In-flight expansion notes for multi-bead epics. These are working
documents, not commitments — a plan here may change before it becomes
a bead.

- [`planning/cy-7s6-expansion.md`](planning/cy-7s6-expansion.md) —
  openCypher TCK long-tail child beads.
- [`planning/cy-od5-expansion.md`](planning/cy-od5-expansion.md) —
  Interop surface child beads (inputs to spec 0004).
