// cypher-wasm Monaco demo glue (spec 0004 §4 + §7).
//
// Two backends:
//
// * agent-wasm — in-page wasm-bindgen bundle calling the agent surface
//   (`CypherDatabase.check`) on every edit.  This is the cy-u6r mode.
//
// * lsp-wasm — a Worker running `cypher-lsp`'s wasm artifact with
//   `postMessage` as the transport (spec 0004 §7).  The main thread
//   speaks JSON-RPC: `initialize`, `textDocument/didOpen`,
//   `textDocument/didChange`.  Diagnostics arrive through
//   `publishDiagnostics` notifications and are translated into Monaco
//   markers identical to agent-wasm.
//
// Both modes must produce identical marker output on the corpus; the
// integration is asserted statically in CI and by the demo reviewer
// flipping the radio toggle.

const AGENT_PKG_URL = "../../crates/cypher-wasm/pkg/cypher_wasm.js";
const LSP_WORKER_URL = "./worker.js";
const EXPECTED_PROTO = 1;

const DEFAULT_SOURCE =
    `// Edit any Cypher below — diagnostics refresh live.\n` +
    `MATCH (p:Person)-[:KNOWS]->(f)\n` +
    `WHERE p.name = $name\n` +
    `RETURN f.name AS friend\n`;

const DEMO_URI = "inmemory:///demo.cyp";

const statusEl = document.getElementById("status");
const protoEl = document.getElementById("proto");
const modeInputs = document.querySelectorAll("#mode-toggle input[name='mode']");

/** Show a message in the status bar.  `err=true` turns the background red. */
function setStatus(msg, { err = false } = {}) {
    statusEl.textContent = msg;
    statusEl.classList.toggle("err", err);
}

/** Translate a diagnostic JSON entry into a Monaco IMarkerData.
 *  Accepts both the cypher-diag JSON shape (agent-wasm) and the LSP
 *  Diagnostic shape (lsp-wasm). */
function diagnosticToMarker(diag, model) {
    // agent-wasm shape: { range: [startOffset, endOffset], message,
    // severity: "error"|"warning"|"info", code }.
    if (Array.isArray(diag.range)) {
        const [startOffset = 0, endOffset = 0] = diag.range;
        const start = model.getPositionAt(startOffset);
        const end = model.getPositionAt(endOffset);
        return {
            severity: severityFromString(diag.severity),
            message: diag.message ?? "(no message)",
            code: diag.code ?? undefined,
            startLineNumber: start.lineNumber,
            startColumn: start.column,
            endLineNumber: end.lineNumber,
            endColumn: end.column,
        };
    }
    // LSP shape: { range: { start: { line, character }, end: {...} },
    // message, severity: 1-4, code }.
    const r = diag.range ?? {
        start: { line: 0, character: 0 },
        end: { line: 0, character: 0 },
    };
    return {
        severity: severityFromLspNumber(diag.severity ?? 1),
        message: diag.message ?? "(no message)",
        code: diag.code ?? undefined,
        startLineNumber: (r.start.line ?? 0) + 1,
        startColumn: (r.start.character ?? 0) + 1,
        endLineNumber: (r.end.line ?? 0) + 1,
        endColumn: (r.end.character ?? 0) + 1,
    };
}

function severityFromString(s) {
    const sev = String(s).toLowerCase();
    if (sev === "error") return 8;
    if (sev === "warning") return 4;
    if (sev === "info") return 2;
    return 8;
}

// LSP Diagnostic severity: 1 Error, 2 Warning, 3 Info, 4 Hint.
// Monaco MarkerSeverity: 8 Error, 4 Warning, 2 Info, 1 Hint.
function severityFromLspNumber(n) {
    switch (Number(n)) {
        case 1: return 8;
        case 2: return 4;
        case 3: return 2;
        case 4: return 1;
        default: return 8;
    }
}

require.config({
    paths: { vs: "https://unpkg.com/monaco-editor@0.52.2/min/vs" },
});

require(["vs/editor/editor.main"], async () => {
    const editor = monaco.editor.create(document.getElementById("editor"), {
        value: DEFAULT_SOURCE,
        language: "plaintext",
        theme: "vs-dark",
        automaticLayout: true,
        minimap: { enabled: false },
    });
    const model = editor.getModel();

    /** Push markers for a list of diagnostic JSON entries. */
    function applyDiagnostics(diagnostics) {
        const markers = diagnostics.map((d) => diagnosticToMarker(d, model));
        monaco.editor.setModelMarkers(model, "cypher-wasm", markers);
        setStatus(
            diagnostics.length === 0
                ? "OK — 0 diagnostics"
                : `${diagnostics.length} diagnostic(s)`,
        );
    }

    // Active backend lifecycle handle: { refresh, dispose }.
    let backend = null;

    async function switchMode(mode) {
        // Tear down the previous backend so its listeners / worker
        // exit before a new one takes over.
        if (backend?.dispose) {
            try { backend.dispose(); } catch { /* swallow */ }
        }
        monaco.editor.setModelMarkers(model, "cypher-wasm", []);
        setStatus(`loading ${mode}…`);
        if (mode === "agent-wasm") {
            backend = await initAgentMode(model, applyDiagnostics);
        } else {
            backend = await initLspMode(model, applyDiagnostics);
        }
    }

    // Trigger on every content edit.  The backend decides whether to
    // debounce server-side; we just forward the latest text.
    model.onDidChangeContent(() => {
        backend?.refresh?.();
    });

    // Radio-group toggle.
    for (const input of modeInputs) {
        input.addEventListener("change", (ev) => {
            if (ev.target.checked) {
                switchMode(ev.target.value);
            }
        });
    }

    // Initial load honours whichever radio was marked `checked` in the HTML.
    const initialMode =
        document.querySelector("#mode-toggle input[name='mode']:checked")?.value
        ?? "agent-wasm";
    await switchMode(initialMode);
});

// ---------------------------------------------------------------------------
// agent-wasm backend
// ---------------------------------------------------------------------------

async function initAgentMode(model, applyDiagnostics) {
    let mod;
    try {
        mod = await import(AGENT_PKG_URL);
        if (typeof mod.default === "function") await mod.default();
    } catch (e) {
        setStatus(
            `wasm bundle not found at ${AGENT_PKG_URL}.\n` +
            `Build it first:\n    cargo xtask wasm-build\n` +
            `Error: ${e.message ?? e}`,
            { err: true },
        );
        return { refresh() {}, dispose() {} };
    }
    const { CypherDatabase } = mod;
    const proto = CypherDatabase.protoVersion();
    protoEl.textContent = String(proto);
    if (proto !== EXPECTED_PROTO) {
        setStatus(
            `proto mismatch — wasm=${proto}, page=${EXPECTED_PROTO}. Rebuild.`,
            { err: true },
        );
        return { refresh() {}, dispose() {} };
    }
    const db = new CypherDatabase();
    function refresh() {
        const source = model.getValue();
        const result = db.check(source);
        const diagnostics = Array.isArray(result?.diagnostics) ? result.diagnostics : [];
        applyDiagnostics(diagnostics);
    }
    refresh();
    return {
        refresh,
        dispose() {},
    };
}

// ---------------------------------------------------------------------------
// lsp-wasm backend (spec 0004 §7)
// ---------------------------------------------------------------------------

async function initLspMode(model, applyDiagnostics) {
    let worker;
    try {
        worker = new Worker(LSP_WORKER_URL, { type: "module" });
    } catch (e) {
        setStatus(
            `lsp-wasm worker failed to start: ${e.message ?? e}\n` +
            `Build the LSP-Web bundle first:\n` +
            `    cargo xtask lsp-web-build`,
            { err: true },
        );
        return { refresh() {}, dispose() {} };
    }
    let nextId = 1;
    let version = 1;
    let disposed = false;

    function post(msg) {
        worker.postMessage(JSON.stringify(msg));
    }

    function sendInitialize() {
        post({
            jsonrpc: "2.0",
            id: nextId++,
            method: "initialize",
            params: {
                processId: null,
                rootUri: null,
                capabilities: {},
            },
        });
    }

    function sendDidOpen(text) {
        post({
            jsonrpc: "2.0",
            method: "textDocument/didOpen",
            params: {
                textDocument: {
                    uri: DEMO_URI,
                    languageId: "cypher",
                    version,
                    text,
                },
            },
        });
    }

    function sendDidChange(text) {
        version += 1;
        post({
            jsonrpc: "2.0",
            method: "textDocument/didChange",
            params: {
                textDocument: { uri: DEMO_URI, version },
                contentChanges: [{ text }],
            },
        });
    }

    // Collected diagnostics per URI — we only render the ones for
    // DEMO_URI but keep the map open for future multi-file demos.
    const diagByUri = new Map();
    let initialized = false;
    worker.onmessage = (ev) => {
        if (disposed) return;
        let msg;
        try {
            msg = typeof ev.data === "string" ? JSON.parse(ev.data) : ev.data;
        } catch (e) {
            console.warn("lsp-wasm: bad message from worker:", e);
            return;
        }
        // Response to initialize → send initialized + didOpen.
        if (msg.id !== undefined && msg.result !== undefined) {
            if (!initialized) {
                initialized = true;
                protoEl.textContent = "lsp";
                post({ jsonrpc: "2.0", method: "initialized", params: {} });
                sendDidOpen(model.getValue());
            }
            return;
        }
        if (msg.method === "textDocument/publishDiagnostics") {
            const p = msg.params ?? {};
            const uri = p.uri ?? DEMO_URI;
            diagByUri.set(uri, Array.isArray(p.diagnostics) ? p.diagnostics : []);
            if (uri === DEMO_URI) {
                applyDiagnostics(diagByUri.get(uri));
            }
        }
    };
    worker.onerror = (e) => {
        setStatus(`lsp-wasm worker error: ${e.message ?? e}`, { err: true });
    };

    sendInitialize();

    function refresh() {
        if (!initialized) return;
        sendDidChange(model.getValue());
    }

    return {
        refresh,
        dispose() {
            disposed = true;
            try {
                post({ jsonrpc: "2.0", id: nextId++, method: "shutdown", params: null });
                post({ jsonrpc: "2.0", method: "exit", params: null });
            } catch { /* swallow */ }
            worker.terminate();
        },
    };
}
