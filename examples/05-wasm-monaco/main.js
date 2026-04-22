// Minimal cypher-wasm glue. One call: db.check(source).
// Run `cargo xtask wasm-build` first to populate ../../crates/cypher-wasm/pkg/.

import init, { CypherDatabase } from
    "../../crates/cypher-wasm/pkg/cypher_wasm.js";

const out = document.getElementById("out");
const src = document.getElementById("src");
const go = document.getElementById("go");

try {
    await init();
} catch (e) {
    out.textContent =
        "wasm init failed — run `cargo xtask wasm-build` from the repo root.\n\n" +
        String(e);
    throw e;
}

if (CypherDatabase.protoVersion() !== 1) {
    out.textContent = "proto mismatch: expected 1, got " + CypherDatabase.protoVersion();
    throw new Error("proto mismatch");
}

const db = new CypherDatabase();

function run() {
    try {
        const result = db.check(src.value);
        out.textContent = JSON.stringify(result, null, 2);
    } catch (e) {
        out.textContent = "check threw: " + String(e);
    }
}

go.addEventListener("click", run);
run();
