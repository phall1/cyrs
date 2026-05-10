# cyrs-wasm Monaco demo

Minimal static page exercising the [`cyrs-wasm`](../../crates/cyrs-wasm/)
binding **and** the [`cyrs-lsp`](../../crates/cyrs-lsp/) web-lsp
build in a Monaco editor — spec 0004 §4 + §7 demo surface.

Plain HTML + ESM.  **No bundler, no build step.**  Monaco is pulled
from a pinned unpkg CDN path via AMD; the wasm wrappers are imported
directly from the per-backend `pkg` / `pkg-lsp` directories.

## Run it

```bash
# 1. agent-wasm backend — build the cyrs-wasm artifact
cargo xtask wasm-build

# 2. lsp-wasm backend — build the cyrs-lsp web-lsp artifact
#    (bead cy-m0d, spec 0004 §7)
cargo xtask lsp-web-build

# 3. serve the demo from any static HTTP server
npx serve demo/web
# → http://localhost:3000
```

The header carries a radio toggle:

* **agent-wasm** — in-page wasm, `CypherDatabase.check(source)` on
  every edit (spec 0004 §4).
* **lsp-wasm** — `cyrs-lsp` running in a Dedicated Worker, driven
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

## Latency contract (cy-bod / cy-od5)

The page diagnoses a 200-line file in **<100 ms p95** under
`textDocument/didChange` traffic.  Because the wasm worker hosts the
same `cyrs-lsp` server code that runs natively, we bound the
round-trip in-process rather than spinning up a browser harness:

- `cargo test -p cyrs-lsp --test demo_latency` drives the LSP server
  in-process over `Connection::memory()` against the same 200-line
  corpus and asserts a **75 ms p95** budget.  The 25 ms headroom up to
  the demo's 100 ms public number is reserved for wasm-bindgen
  marshalling + the `postMessage` Worker hop + Monaco's marker paint
  cycle (each typically ≤ 5 ms in modern browsers).
- If `demo_latency` regresses, the demo will too — fix it there first.
  If only the demo regresses, suspect the wasm bundle size, worker
  glue, or Monaco — none of which the in-process test covers.

To time round-trips live in the page, paste this in the devtools
console once `lsp-wasm` mode is selected:

```js
performance.mark('rt-start');
db.lspWorker.postMessage(/* the didChange message */);
// publishDiagnostics arrives via the worker's onmessage; mark
// 'rt-end' there and call performance.measure('rt', 'rt-start', 'rt-end').
```

## License

Same terms as the rest of the Cyrs workspace — Apache-2.0 OR MIT.
