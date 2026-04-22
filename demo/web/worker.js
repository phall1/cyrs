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
// (wasm-bindgen `--target no-modules`).  We load the JS via importScripts
// — `type: "module"` workers do not honour wasm-bindgen's no-modules
// output, so this file is a classic worker with a dynamic init guard.

const PKG_JS_URL = "./pkg-lsp/cypher_lsp.js";
const PKG_WASM_URL = "./pkg-lsp/cypher_lsp_bg.wasm";

let ready = false;
const pending = [];

(async function init() {
    try {
        // wasm-bindgen --target no-modules exposes a global
        // `wasm_bindgen` function on `self` after importScripts.  We
        // call it with the wasm URL to initialise the instance, then
        // invoke the exported `start_lsp()` entry point which takes
        // over `onmessage`.
        // eslint-disable-next-line no-undef
        self.importScripts(PKG_JS_URL);
        // eslint-disable-next-line no-undef
        await self.wasm_bindgen(PKG_WASM_URL);
        // eslint-disable-next-line no-undef
        self.wasm_bindgen.start_lsp();
        ready = true;
        // Replay buffered messages that landed before the module
        // finished initialising.
        for (const m of pending) {
            self.postMessage.bind(self); // shape: worker forwards into itself
            self.dispatchEvent(new MessageEvent("message", { data: m }));
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

// Buffer inbound messages while the wasm module is still initialising.
// Once `start_lsp` returns, the wasm module replaces the `onmessage`
// handler, so this handler is only used pre-init.
self.addEventListener("message", (ev) => {
    if (!ready) {
        pending.push(ev.data);
    }
});
