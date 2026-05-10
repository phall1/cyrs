//! Compiletest-style golden corpus for `cyrs-plan` (spec 0001 §17.6).
//!
//! Discovers every `.cypher` file under `tests/ui/plan/`, runs the HIR
//! pipeline then plan lowering, and compares the pretty-printed plan against
//! the companion `.stderr` sidecar.
//!
//! # Blessing
//!
//! Set `BLESS=1` (or run `cargo xtask bless -p cyrs-plan`) to regenerate
//! the `.stderr` sidecars from the actual plan output.

use std::path::Path;

use cyrs_testkit::{CorpusKind, run_ui_corpus};

#[test]
fn ui_plan_corpus() {
    run_ui_corpus(
        CorpusKind::Plan,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        "cyrs-plan",
    );
}
