# cypher/docs

Design documents and specs that outlive any single PR or conversation.

## Specs

Normative architecture commitments. Numbered, dated, own-able. Read top
to bottom when a spec is relevant to the work at hand.

- [`specs/0001-cypher-frontend.md`](specs/0001-cypher-frontend.md) —
  Architecture of the standalone Rust Cypher front-end: crate graph,
  syntax/AST/HIR/sema/plan layers, schema-provider trait, dialect matrix,
  incremental DB, LSP/agent/CLI, and testing at rustc grade.
- [`specs/0002-schema-file-format.md`](specs/0002-schema-file-format.md) —
  `schema.toml` file format: TOML grammar, types, labels, rel types,
  parameters, validation rules, linter (`E3010`/`E3011`/`W6010`), and
  structural diff.
- [`specs/0003-project-manifest.md`](specs/0003-project-manifest.md) —
  `cypher-project.toml` workspace manifest: members, dialect defaults,
  lint levels, schema wiring.
- [`specs/0004-interop-surfaces.md`](specs/0004-interop-surfaces.md) —
  Interop surfaces: `cyrs-wasm` (WASM + Monaco), `cyrs-ffi` (stable C
  ABI + cbindgen), `cyrs-py` (PyO3 wheel), LSP-Web transport, and the
  tree-sitter parity contract; pins stability commitments for each.

## Conventions

- Specs are numbered sequentially (`NNNN-topic-slug.md`) and never
  renumbered.
- A spec's **Status** moves Proposed → Accepted → Implemented →
  Superseded. Do not edit an accepted spec in place to reflect
  implementation drift; write a superseding spec.
- "Deferred" sections in a spec mean intentionally omitted, not
  forgotten. Add to them freely; graduate them into the main body with
  a new spec revision when we build them.
- Reference existing specs from new specs by number (`see §3.6 of spec
  0001`) rather than by link; the number is stable.
