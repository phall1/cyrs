//! `cargo xtask wasm-smoke` — end-to-end round-trip across the
//! wasm-bindgen boundary.  Spec 0004 §10.1.
//!
//! Runs under `wasm-pack test --headless --chrome`.  The whole file is
//! gated behind the `wasm-smoke` Cargo feature so CI runners without
//! the wasm toolchain do not see an extra dev-dep / failing test.  A
//! native `cargo test` compiles this as an empty file.

#![cfg(all(feature = "wasm-smoke", target_arch = "wasm32"))]

use cypher_wasm::CypherDatabase;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// proto version surfaces as the pinned u32 const.
#[wasm_bindgen_test]
fn proto_version_is_one() {
    assert_eq!(CypherDatabase::proto_version_static(), 1);
}

/// A well-formed query produces zero diagnostics.  Exercises the full
/// parse + sema pipeline through the wasm boundary — the bead's
/// "assert no diagnostics" acceptance criterion.
#[wasm_bindgen_test]
fn check_of_well_formed_query_reports_no_diagnostics() {
    let mut db = CypherDatabase::new();
    let result = db.check("MATCH (n) RETURN n".into(), None);
    let value: serde_json::Value =
        serde_wasm_bindgen::from_value(result).expect("check result is JSON-compatible");
    let diagnostics = value
        .get("diagnostics")
        .and_then(|d| d.as_array())
        .expect("diagnostics array");
    assert!(
        diagnostics.is_empty(),
        "well-formed query must not emit diagnostics: {diagnostics:?}"
    );
}
