# cypher-wasm Monaco demo

Minimal static page exercising the [`cypher-wasm`](../../crates/cypher-wasm/)
binding in a Monaco editor — spec 0004 §4 demo surface.

Plain HTML + ESM.  **No bundler, no build step.**  Monaco is pulled
from a pinned unpkg CDN path via AMD; the wasm wrapper is imported
directly from `../../crates/cypher-wasm/pkg/cypher_wasm.js` (populated
by `cargo xtask wasm-build` — see
[the crate README](../../crates/cypher-wasm/README.md) for the full
build pipeline).

## Run it

```bash
# 1. build the wasm artifact (first time only / after a lib change)
cargo xtask wasm-build

# 2. serve the demo from any static HTTP server
npx serve demo/web
# → http://localhost:3000
```

If the `cypher_wasm.js` wrapper is absent the page loads with a clear
"build via `cargo xtask wasm-build` first" notice in the status bar —
editing still works, it just stops surfacing diagnostics.

## What it exercises

- `CypherDatabase.protoVersion()` — surfaced in the header; mismatch
  aborts with an error (spec 0004 §4.3).
- `db.check(source)` on every keystroke — translates cypher-diag JSON
  diagnostics into Monaco `IMarkerData` red / yellow / blue squiggles.

Other agent ops (`complete`, `hover`, `format`, `rewrite`, `plan`,
`explain`, `schemaSet`, `schemaClear`) are callable from the browser
console on `window.db` once the demo has loaded — hook them up to UI
as the playground grows.

## License

Same terms as the rest of the Cyrs workspace — Apache-2.0 OR MIT.
