// Dedicated Worker that hosts the `cypher-lsp` wasm artifact
// (spec 0004 §7, bead cy-m0d).
//
// The main thread speaks JSON-RPC over `postMessage`; this worker
// forwards the same messages to the wasm `start_lsp` entry point which
// installs its own `onmessage` listener on `self`.
//
// Build the artifact first:
//
//     cargo xtask lsp-web-build
//
// That produces `./pkg-lsp/cypher_lsp.js` + `cypher_lsp_bg.wasm`
// (wasm-bindgen `--target no-modules`).  We load the JS via
// importScripts — `type: "module"` workers do not honour
// wasm-bindgen's no-modules output, so this file is a classic worker.

const PKG_JS_URL = "./pkg-lsp/cypher_lsp.js";
const PKG_WASM_URL = "./pkg-lsp/cypher_lsp_bg.wasm";

// Buffer messages received before the wasm module finishes booting.
// `start_lsp` sets `self.onmessage` on the Rust side, which receives
// future messages directly; this pre-init handler stops at that point
// (we remove it explicitly to avoid double-dispatch).
const pending = [];

function preInitHandler(ev) {
    pending.push(ev.data);
}
self.addEventListener("message", preInitHandler);

(async function init() {
    try {
        // wasm-bindgen --target no-modules exposes a global
        // `wasm_bindgen` function on `self` after importScripts.  We
        // call it with the wasm URL to initialise the instance, then
        // invoke the exported `start_lsp()` entry point which takes
        // over `onmessage`.
        self.importScripts(PKG_JS_URL);
        // eslint-disable-next-line no-undef
        await self.wasm_bindgen(PKG_WASM_URL);
        // eslint-disable-next-line no-undef
        self.wasm_bindgen.start_lsp();
        // Rust installed its own `self.onmessage` handler.  Stop
        // buffering and replay anything that arrived during boot via a
        // synthetic MessageEvent so the Rust handler sees it.
        self.removeEventListener("message", preInitHandler);
        const rustHandler = self.onmessage;
        for (const data of pending) {
            const ev = new MessageEvent("message", { data });
            rustHandler?.call(self, ev);
        }
        pending.length = 0;
    } catch (e) {
        // Surface the failure back to the main thread as a structured
        // error — the UI shows it in the status bar.
        self.postMessage(
            JSON.stringify({
                jsonrpc: "2.0",
                method: "window/logMessage",
                params: {
                    type: 1,
                    message:
                        `lsp-wasm worker: failed to load pkg-lsp bundle.\n` +
                        `Build via: cargo xtask lsp-web-build\n` +
                        `Error: ${e.message ?? e}`,
                },
            }),
        );
    }
})();
