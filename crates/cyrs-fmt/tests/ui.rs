//! Compiletest-style golden corpus for `cyrs-fmt` (spec 0001 §17.6).
//!
//! Discovers every `.cypher` file under `tests/ui/fmt/`, runs the formatter,
//! and compares the output against the companion `.formatted.cypher` sidecar.
//!
//! # Blessing
//!
//! Set `BLESS=1` (or run `cargo xtask bless -p cyrs-fmt`) to regenerate
//! the `.formatted.cypher` sidecars from the actual formatter output.

use std::path::Path;

use cyrs_testkit::{CorpusKind, run_ui_corpus};

#[test]
fn ui_fmt_corpus() {
    run_ui_corpus(
        CorpusKind::Fmt,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        "cyrs-fmt",
    );
}
