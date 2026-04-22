// cypher-wasm Monaco demo glue (spec 0004 §4).
//
// - Loads the wasm-bindgen generated JS wrapper from
//   ../../crates/cypher-wasm/pkg/cypher_wasm.js.
// - If the .wasm asset has not been built yet, surfaces a clear
//   "build via `cargo xtask wasm-build` first" message in the status
//   bar instead of crashing.
// - Wires Monaco's `onDidChangeModelContent` to `db.check(source)` and
//   translates the resulting `diagnostics` into `IMarkerData` entries
//   that drive Monaco's red / yellow squiggles.

const PKG_URL = "../../crates/cypher-wasm/pkg/cypher_wasm.js";
const EXPECTED_PROTO = 1;

const DEFAULT_SOURCE =
    `// Edit any Cypher below — diagnostics refresh live.\n` +
    `MATCH (p:Person)-[:KNOWS]->(f)\n` +
    `WHERE p.name = $name\n` +
    `RETURN f.name AS friend\n`;

const statusEl = document.getElementById("status");
const protoEl = document.getElementById("proto");

/** Show a message in the status bar.  `err=true` turns the background red. */
function setStatus(msg, { err = false } = {}) {
    statusEl.textContent = msg;
    statusEl.classList.toggle("err", err);
}

/** Load the wasm-bindgen wrapper.  Returns `null` if the bundle is absent. */
async function loadWasmModule() {
    try {
        const mod = await import(PKG_URL);
        // wasm-bindgen --target web exports init() by default; call it
        // so the underlying .wasm fetches once.
        if (typeof mod.default === "function") {
            await mod.default();
        }
        return mod;
    } catch (e) {
        const msg =
            `wasm bundle not found at ${PKG_URL}.\n` +
            `Build it first:\n` +
            `    cargo xtask wasm-build       # or see crates/cypher-wasm/README.md\n` +
            `Error: ${e.message ?? e}`;
        setStatus(msg, { err: true });
        return null;
    }
}

/** Translate a cypher-wasm diagnostic JSON entry into a Monaco IMarkerData. */
function diagnosticToMarker(diag, model) {
    // Diagnostics come through cypher-diag's json::to_json helper with
    // a `range: [start, end]` byte pair plus `message`, `severity`,
    // `code` fields.  Monaco wants 1-based lines/cols via
    // model.getPositionAt — which accepts UTF-16 code unit offsets.
    const [startOffset = 0, endOffset = 0] = diag.range ?? [0, 0];
    const start = model.getPositionAt(startOffset);
    const end = model.getPositionAt(endOffset);

    // cypher-diag severity strings: "error" | "warning" | "info".
    const severity = {
        error: 8,    // monaco.MarkerSeverity.Error
        warning: 4,  // monaco.MarkerSeverity.Warning
        info: 2,     // monaco.MarkerSeverity.Info
    }[String(diag.severity).toLowerCase()] ?? 8;

    return {
        severity,
        message: diag.message ?? "(no message)",
        code: diag.code ?? undefined,
        startLineNumber: start.lineNumber,
        startColumn: start.column,
        endLineNumber: end.lineNumber,
        endColumn: end.column,
    };
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

    const wasm = await loadWasmModule();
    if (!wasm) {
        return;  // status bar already carries the "build me" hint.
    }

    const { CypherDatabase } = wasm;
    const proto = CypherDatabase.protoVersion();
    protoEl.textContent = String(proto);
    if (proto !== EXPECTED_PROTO) {
        setStatus(
            `proto mismatch — wasm=${proto}, page=${EXPECTED_PROTO}. Rebuild.`,
            { err: true },
        );
        return;
    }

    const db = new CypherDatabase();
    const model = editor.getModel();

    /** Re-run check() and push markers into Monaco. */
    function refresh() {
        const source = model.getValue();
        const result = db.check(source);
        const diagnostics = Array.isArray(result?.diagnostics) ? result.diagnostics : [];
        const markers = diagnostics.map((d) => diagnosticToMarker(d, model));
        monaco.editor.setModelMarkers(model, "cypher-wasm", markers);
        setStatus(
            diagnostics.length === 0
                ? "OK — 0 diagnostics"
                : `${diagnostics.length} diagnostic(s)`,
        );
    }

    model.onDidChangeContent(refresh);
    refresh();
});
