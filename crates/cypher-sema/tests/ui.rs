//! Compiletest-style golden corpus for `cypher-sema` (spec 0001 §17.6).
//!
//! Delegates to `cypher_testkit::compiletest::run_ui_corpus` which discovers
//! every `.cypher` file under `tests/ui/sema/`, runs the full sema pipeline,
//! renders all diagnostics, and compares against the companion `.stderr`
//! sidecar byte-exactly.
//!
//! # Blessing
//!
//! Set `BLESS=1` (or run `cargo xtask bless -p cypher-sema`) to regenerate
//! the `.stderr` sidecars from the actual pipeline output.

use std::path::Path;

use cypher_testkit::{CorpusKind, run_ui_corpus};

#[test]
fn ui_sema_corpus() {
    run_ui_corpus(
        CorpusKind::Sema,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        "cypher-sema",
    );
}
