//! `cypher-testkit` — shared test fixtures (dev only).
//!
//! Spec 0001 §17.6 describes a compiletest-style golden-file runner. This
//! crate is where that runner will live, alongside helper setup for the
//! incremental database and insta snapshot plumbing shared across crates.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-testkit/0.0.1")]

use cypher_db::Database;

/// Build a fresh database populated with the given source text. Returns
/// the `Database` and the allocated file id. Used by snapshot and
/// compiletest suites to keep setup uniform.
#[must_use]
pub fn db_with_source(src: impl Into<String>) -> (Database, cypher_db::FileId) {
    let db = Database::new();
    let id = db.allocate_file();
    db.set_source(id, src.into());
    (db, id)
}
