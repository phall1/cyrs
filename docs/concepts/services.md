# Concept: services

Everything above the pipeline that turns the layered front-end into a
**usable surface** for editors, agents, and CLIs. Three crates share a
single engine layer; no analysis logic is duplicated.

**Crates:** [`cyrs-db`](../../crates/cyrs-db),
[`cyrs-lang-services`](../../crates/cyrs-lang-services),
[`cyrs-lsp`](../../crates/cyrs-lsp),
[`cyrs-agent`](../../crates/cyrs-agent),
[`cyrs-cli`](../../crates/cyrs-cli).
**Spec section:** [0001 §3.6, §3.7](../specs/0001-cypher-frontend.md).

## The shared engine

`cyrs-db` is a [Salsa](https://salsa-rs.github.io/salsa/)-backed
incremental analysis database. It memoises every layer of the
pipeline keyed by source revision, so an editor session that re-runs
analysis on every keystroke pays only for the affected queries.

`cyrs-lang-services` sits above `cyrs-db` and exposes the
editor-shaped operations: completion, hover, code actions, signature
help, semantic tokens, rewrite engines. The LSP server and the agent
binary both call into this layer; neither one re-implements analysis.

## The surfaces

| Surface | Crate | Transport | Audience |
| ------- | ----- | --------- | -------- |
| Language server | `cyrs-lsp` | JSON-RPC over stdio (LSP) | IDEs (VS Code, Neovim, Helix, …) |
| Agent API | `cyrs-agent` | One JSON request per stdin line | AI agents, scripted automation |
| CLI | `cyrs-cli` | `cypher {parse,check,fmt,explain,plan,schema …}` | Humans, CI scripts |
| WASM / FFI / Python | `cyrs-wasm`, `cyrs-ffi`, `cyrs-py` | Per-language bindings | Embeddings outside Rust |

Interop bindings (WASM, C ABI, PyO3) are normative in
[spec 0004](../specs/0004-interop-surfaces.md).

## When to reach for which surface

- **An editor or IDE** consumes the LSP surface. Diagnostics,
  completion, format-on-save, and code actions are all wired through
  `cyrs-lsp`.
- **An AI agent** consumes the JSON agent API. One JSON-shaped request
  per line of stdin keeps the surface scriptable and sandboxable.
- **A CI gate or a human at a terminal** uses the CLI.
- **A non-Rust embedder** uses the WASM / FFI / Python binding that
  matches its host language.

## Incremental analysis in practice

A Salsa query in `cyrs-db` is a memoised function from inputs (source
text, schema revision) to outputs (CST, HIR, diagnostics, plan).
Editing one query in a multi-query file invalidates only the touched
inputs; everything upstream is reused. This is why an LSP session
stays responsive at the keystroke scale.

## Related

- Underlying pipeline: [`syntax`](./syntax.md) → [`hir`](./hir.md) →
  [`sema`](./sema.md) → [`plan`](./plan.md).
- Interop surfaces: [spec 0004](../specs/0004-interop-surfaces.md).
- Stability contract for the agent wire protocol: [`stability.md`](../stability.md).
