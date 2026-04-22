# cypher-wasm Monaco demo

Minimal static page exercising the [`cypher-wasm`](../../crates/cypher-wasm/)
binding **and** the [`cypher-lsp`](../../crates/cypher-lsp/) web-lsp
build in a Monaco editor — spec 0004 §4 + §7 demo surface.

Plain HTML + ESM.  **No bundler, no build step.**  Monaco is pulled
from a pinned unpkg CDN path via AMD; the wasm wrappers are imported
directly from the per-backend `pkg` / `pkg-lsp` directories.

## Run it

```bash
# 1. agent-wasm backend — build the cypher-wasm artifact
cargo xtask wasm-build

# 2. lsp-wasm backend — build the cypher-lsp web-lsp artifact
#    (bead cy-m0d, spec 0004 §7)
cargo xtask lsp-web-build

# 3. serve the demo from any static HTTP server
npx serve demo/web
# → http://localhost:3000
```

The header carries a radio toggle:

* **agent-wasm** — in-page wasm, `CypherDatabase.check(source)` on
  every edit (spec 0004 §4).
* **lsp-wasm** — `cypher-lsp` running in a Dedicated Worker, driven
  over `postMessage` JSON-RPC (spec 0004 §7).  The page speaks
  `initialize` / `textDocument/didOpen` / `textDocument/didChange`
  and renders `publishDiagnostics` notifications as Monaco markers.

Both backends produce **identical** diagnostic output on the sample
corpus; CI asserts this on every PR.

If either bundle is absent the status bar points at the xtask command
to build it; the editor continues to function.

## What it exercises

- `CypherDatabase.protoVersion()` — surfaced in the header; mismatch
  aborts with an error (spec 0004 §4.3).
- `db.check(source)` on every keystroke (agent-wasm).
- `publishDiagnostics` round-trip over a message-channel transport
  (lsp-wasm, spec 0004 §7.2).

Other agent ops (`complete`, `hover`, `format`, `rewrite`, `plan`,
`explain`, `schemaSet`, `schemaClear`) are callable from the browser
console on `window.db` once agent-wasm has loaded — hook them up to UI
as the playground grows.

## License

Same terms as the rest of the Cyrs workspace — Apache-2.0 OR MIT.
