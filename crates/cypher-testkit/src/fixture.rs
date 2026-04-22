//! Test fixture helpers. Spec 0001 §17.6.
//!
//! Every test that touches the analysis pipeline starts from a [`Database`]
//! with at least one file loaded.  The functions here provide a uniform
//! construction path so individual crate test suites stay minimal.

use std::path::Path;

use cypher_db::{Database, DialectMode, FileId, LegacyDatabase, LegacyFileId};

/// Build a fresh [`Database`] (new workspace API, spec §11.4) with a single
/// file pre-loaded.
///
/// The dialect defaults to [`DialectMode::GqlAligned`].  Use
/// [`db_with_source_and_dialect`] when the test exercises dialect-specific
/// behaviour (spec §9).
///
/// # Example
/// ```
/// use cypher_testkit::fixture::db_with_source;
/// let (db, fid) = db_with_source("MATCH (n) RETURN n");
/// let parse = db.parse_cst(fid).unwrap();
/// assert!(parse.parse().errors().is_empty());
/// ```
#[must_use]
pub fn db_with_source(src: impl Into<String>) -> (Database, FileId) {
    db_with_source_and_dialect(src, DialectMode::GqlAligned)
}

/// Build a fresh [`Database`] (new workspace API, spec §11.4) with a single
/// file pre-loaded at the given dialect mode.
///
/// # Example
/// ```
/// use cypher_db::DialectMode;
/// use cypher_testkit::fixture::db_with_source_and_dialect;
/// let (db, fid) = db_with_source_and_dialect(
///     "MATCH (n) RETURN n",
///     DialectMode::OpenCypherV9,
/// );
/// let parse = db.parse_cst(fid).unwrap();
/// assert!(parse.parse().errors().is_empty());
/// ```
#[must_use]
pub fn db_with_source_and_dialect(
    src: impl Into<String>,
    dialect: DialectMode,
) -> (Database, FileId) {
    let mut db = Database::new();
    let id = db.open_file(Path::new("fixture.cyp"), src.into(), dialect);
    (db, id)
}

/// Build a fresh [`LegacyDatabase`] with a single file pre-loaded.
///
/// Retained for tests that have not yet migrated to the new workspace API.
///
/// # Example
/// ```
/// use cypher_testkit::fixture::legacy_db_with_source;
/// let (db, fid) = legacy_db_with_source("MATCH (n) RETURN n");
/// let parse = db.parse(fid);
/// assert!(parse.errors().is_empty());
/// ```
#[must_use]
pub fn legacy_db_with_source(src: impl Into<String>) -> (LegacyDatabase, LegacyFileId) {
    legacy_db_with_source_and_dialect(src, DialectMode::GqlAligned)
}

/// Build a fresh [`LegacyDatabase`] with a single file pre-loaded at the
/// given dialect mode.
#[must_use]
pub fn legacy_db_with_source_and_dialect(
    src: impl Into<String>,
    dialect: DialectMode,
) -> (LegacyDatabase, LegacyFileId) {
    let db = LegacyDatabase::new();
    let id = db.allocate_file();
    let s: String = src.into();
    db.set_source(id, s.as_str());
    db.set_dialect(id, dialect);
    (db, id)
}
