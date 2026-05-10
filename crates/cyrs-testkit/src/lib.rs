//! `cyrs-testkit` — shared test fixtures, snapshot helpers, and
//! compiletest runner for the Cypher front-end. Dev-only; never published.
//!
//! Spec 0001 §17.6 describes a `rustc`-style golden compiletest corpus.
//! This crate provides:
//!
//! - [`fixture`] — helpers for building a populated [`cyrs_db::Database`] and
//!   loading `.cypher` input files from the `tests/ui/**` corpus.
//! - [`snapshot`] — thin wrappers around `insta` so every crate uses the
//!   same redaction set and settings file automatically.
//! - [`compiletest`] — the custom golden-file runner that pairs each
//!   `.cypher` input with its expected `.stderr`, `.ast.txt`, `.hir.txt`,
//!   and `.plan.json` outputs and fails loudly on byte-level divergence.
//!   Re-generation is `cargo xtask bless`.
//!
//! # Why a separate crate?
//!
//! Duplicating setup boilerplate across fifteen crates raises the bar for
//! writing tests and leads to drift.  Centralising it here keeps test
//! entry-points lean and ensures consistent `insta` settings, database
//! construction, and diff output across the whole workspace.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cyrs-testkit/0.0.1")]

pub mod compiletest;
pub mod fixture;
pub mod snapshot;

pub use compiletest::{CorpusKind, run_ui_corpus};
pub use fixture::{
    db_with_source, db_with_source_and_dialect, legacy_db_with_source,
    legacy_db_with_source_and_dialect,
};
