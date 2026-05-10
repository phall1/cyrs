// Node.js smoke test for the cyrs-ffi C ABI.  Spec 0004 §10.2.
//
// Uses `koffi` (a pure-JS, no-native-build FFI loader) to open
// libcyrs_ffi.{dylib,so} and invoke `cypher_check` on a malformed
// query.  Picks koffi over ffi-napi because the latter requires a
// native build step (and re-breaks every Node major); koffi is a
// single npm dep.
//
// Run:
//
//   cargo build -p cyrs-ffi --release     (from workspace root)
//   cd demo/ffi/node && npm install && npm start
//
// The script exits 0 when at least one diagnostic is reported for
// `MATCH (n RETURN n` (unclosed paren), and non-zero otherwise.

import koffi from "koffi";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(here, "..", "..", "..");

// Resolve the platform-specific dylib name.  macOS → .dylib, Linux → .so,
// Windows → .dll.  Matches cargo's artifact layout.
function dylibPath() {
    const base = path.join(workspaceRoot, "target", "release");
    switch (process.platform) {
        case "darwin":
            return path.join(base, "libcyrs_ffi.dylib");
        case "linux":
            return path.join(base, "libcyrs_ffi.so");
        case "win32":
            return path.join(base, "cyrs_ffi.dll");
        default:
            throw new Error(`unsupported platform ${process.platform}`);
    }
}

const libPath = dylibPath();
const lib = koffi.load(libPath);

// Opaque handle pointers — koffi treats every unknown name as an
// opaque pointer, so we only need to declare types for the primitive
// return values.
const cypher_proto_version = lib.func("cypher_proto_version", "uint32", []);
const cypher_last_error = lib.func("cypher_last_error", "str", []);
const cypher_database_new = lib.func("cypher_database_new", "void*", []);
const cypher_database_free = lib.func("cypher_database_free", "void", ["void*"]);
const cypher_check = lib.func("cypher_check", "void*", ["void*", "str", "size_t"]);
const cypher_diagnostic_list_free = lib.func(
    "cypher_diagnostic_list_free",
    "void",
    ["void*"]
);
const cypher_diagnostic_list_len = lib.func(
    "cypher_diagnostic_list_len",
    "size_t",
    ["void*"]
);
const cypher_diagnostic_code = lib.func("cypher_diagnostic_code", "str", [
    "void*",
    "size_t",
]);
const cypher_diagnostic_severity = lib.func("cypher_diagnostic_severity", "str", [
    "void*",
    "size_t",
]);
const cypher_diagnostic_message = lib.func("cypher_diagnostic_message", "str", [
    "void*",
    "size_t",
]);
const cypher_diagnostic_start_line = lib.func(
    "cypher_diagnostic_start_line",
    "uint32",
    ["void*", "size_t"]
);
const cypher_diagnostic_start_col = lib.func(
    "cypher_diagnostic_start_col",
    "uint32",
    ["void*", "size_t"]
);
const cypher_diagnostic_end_line = lib.func(
    "cypher_diagnostic_end_line",
    "uint32",
    ["void*", "size_t"]
);
const cypher_diagnostic_end_col = lib.func(
    "cypher_diagnostic_end_col",
    "uint32",
    ["void*", "size_t"]
);

const proto = cypher_proto_version();
if (proto !== 1) {
    console.error(`unexpected proto_version: ${proto} (want 1)`);
    process.exit(2);
}
console.log(`cypher_proto_version() = ${proto}`);

const db = cypher_database_new();
if (!db) {
    console.error("cypher_database_new returned NULL");
    process.exit(2);
}

const src = "MATCH (n RETURN n";
const list = cypher_check(db, src, src.length);
if (!list) {
    const err = cypher_last_error();
    console.error(`cypher_check returned NULL: ${err}`);
    cypher_database_free(db);
    process.exit(2);
}

const n = Number(cypher_diagnostic_list_len(list));
console.log(`${n} diagnostic(s) for ${JSON.stringify(src)}:`);
for (let i = 0; i < n; i++) {
    const code = cypher_diagnostic_code(list, i);
    const severity = cypher_diagnostic_severity(list, i);
    const msg = cypher_diagnostic_message(list, i);
    const sl = cypher_diagnostic_start_line(list, i);
    const sc = cypher_diagnostic_start_col(list, i);
    const el = cypher_diagnostic_end_line(list, i);
    const ec = cypher_diagnostic_end_col(list, i);
    console.log(`  [${code}] ${severity} ${sl}:${sc}-${el}:${ec} — ${msg}`);
}

cypher_diagnostic_list_free(list);
cypher_database_free(db);

if (n < 1) {
    console.error("expected at least one diagnostic; got zero");
    process.exit(1);
}
console.log("OK");
