//! Compiletest-style golden corpus for `cypher-syntax` (spec 0001 §17.6).
//!
//! Discovers every `.cypher` file under `tests/ui/syntax/`, runs the parser,
//! and compares syntax error output against the companion `.stderr` sidecar.
//!
//! # Blessing
//!
//! Set `BLESS=1` (or run `cargo xtask bless -p cypher-syntax`) to regenerate
//! the `.stderr` sidecars from the actual pipeline output.

use std::path::Path;

use cypher_testkit::{CorpusKind, run_ui_corpus};

#[test]
fn ui_syntax_corpus() {
    run_ui_corpus(
        CorpusKind::Syntax,
        Path::new(env!("CARGO_MANIFEST_DIR")),
        "cypher-syntax",
    );
}
