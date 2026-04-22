# 05-wasm-monaco — browser front-end, minimal

Two ways to drive `cypher-wasm` in a page:

1. **Pointer.** The full Monaco + dual-backend playground lives at
   [`../../demo/web/`](../../demo/web/). Follow its README — one
   xtask, one static server, one editor with live diagnostics.
2. **Minimal.** The `index.html` + `main.js` in this directory: a
   textarea, a button, a `<pre>` panel. No editor, no Monaco, no
   build step beyond the wasm artifact. Useful as a starting point
   when Monaco is overkill.

The minimal page exercises exactly one thing: wire a string through
`CypherDatabase.check(source)` and render the diagnostic JSON.

## Pre-reqs

Build the wasm artifact from the repo root:

```sh
cargo xtask wasm-build
```

That populates `crates/cypher-wasm/pkg/` with the `.wasm` + JS
wrapper. The demo loads them directly; no bundler.

## Run the minimal page

From the repo root:

```sh
npx serve .
# then browse http://localhost:3000/examples/05-wasm-monaco/
```

Type into the textarea, click **Check**. Diagnostics for the current
source render as JSON.

## What the minimal page does

```js
import init, { CypherDatabase } from
    "../../crates/cypher-wasm/pkg/cypher_wasm.js";

await init();
const db = new CypherDatabase();
const result = db.check(source);  // { diagnostics: [ {code, severity, …} ] }
```

Every agent op is a method on `CypherDatabase`. See
`crates/cypher-wasm/README.md` for the full list and the proto-version
handshake (spec 0004 §4.3).

## Where to go next

- Full Monaco integration — see `demo/web/main.js` for how to convert
  diagnostic byte-ranges into Monaco markers.
- Worker-based LSP — `demo/web/worker.js` hosts `cypher-lsp` over
  `postMessage` JSON-RPC. Same wire shape as stdio.
