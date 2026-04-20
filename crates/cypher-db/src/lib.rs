//! `cypher-db` — incremental analysis database (spec 0001 §11).
//!
//! This bead (cy-zx6) installs the Salsa 2022-style skeleton alongside the
//! existing thin facade.  The facade remains fully functional so that binary
//! crates (`cypher-cli`, `cypher-agent`) and `cypher-testkit` continue to
//! compile unchanged; their migration to the Salsa API lands in the pipeline
//! beads (cy-nk7, cy-i6a, cy-amr).
//!
//! ## Salsa API (new in cy-zx6)
//!
//! - [`SourceFile`] — Salsa `#[input]` for a single source unit.
//! - [`CypherDatabase`] — the concrete `salsa::Database` impl.
//! - [`CypherDb`] — the database trait that all concrete DBs implement.
//! - [`ParseOutput`] — memoised result of parsing a [`SourceFile`].
//! - [`parse_cst`] — first derived query: lossless CST, re-evaluated only
//!   when `source` changes.
//!
//! ## Legacy facade API (preserved for backward compat)
//!
//! [`Database`] / [`FileId`] remain.  They will be retired once all
//! downstream callers migrate to [`CypherDatabase`].
//!
//! ## Send + Sync / snapshot semantics (spec §11.5)
//!
//! `CypherDatabase` is `Clone`.  Cloning shares the `Arc<Zalsa>` backing
//! store and creates a fresh `ZalsaLocal`, producing a snapshot.  The clone
//! can be sent to another thread for concurrent read queries (`Send`).
//! Writes require `&mut self` and block until all clones are dropped.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-db/0.0.1")]

use std::sync::Arc;

use cypher_syntax::{Parse, parse};
use salsa::Setter as _;

// ---------------------------------------------------------------------------
// Dialect mode (spec §9)
// ---------------------------------------------------------------------------

/// Dialect mode selected at parse time.  Spec §9.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DialectMode {
    /// GQL-aligned parsing (default).
    #[default]
    GqlAligned,
    /// openCypher 9 compatibility mode.
    OpenCypherV9,
}

// ---------------------------------------------------------------------------
// Compile-time Send / Sync assertion helpers (no unsafe)
// ---------------------------------------------------------------------------

macro_rules! assert_send {
    ($T:ty) => {
        const _: () = {
            fn _check()
            where
                $T: Send,
            {
            }
        };
    };
}

macro_rules! assert_sync {
    ($T:ty) => {
        const _: () = {
            fn _check()
            where
                $T: Sync,
            {
            }
        };
    };
}

// ---------------------------------------------------------------------------
// ParseOutput — newtype wrapping Arc<Parse> with pointer-equality Eq
//
// Salsa tracked functions require Output: Eq so that it can detect whether
// re-execution produced the same value (§11.3).  `Parse` itself does not
// implement `Eq`, so we wrap it in an `Arc` and compare by pointer identity.
// Two re-executions of an unchanged source return the cached Arc, so they
// compare equal.  A mutation produces a fresh `Arc`, so they compare unequal.
// ---------------------------------------------------------------------------

/// Memoised result of parsing a [`SourceFile`].
///
/// Wraps an `Arc<Parse>`; equality is by pointer identity so that Salsa
/// can detect when re-parsing produced the same tree.
#[derive(Debug, Clone)]
pub struct ParseOutput(Arc<Parse>);

impl ParseOutput {
    fn new(p: Parse) -> Self {
        Self(Arc::new(p))
    }

    /// Access the underlying [`Parse`].
    #[must_use]
    pub fn parse(&self) -> &Parse {
        &self.0
    }
}

impl PartialEq for ParseOutput {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ParseOutput {}

// Salsa requires Output: Send + Sync.
// Arc<Parse> is Send+Sync because GreenNode (Arc-backed) and Vec<SyntaxError>
// (all fields Send+Sync) are Send+Sync.
assert_send!(ParseOutput);
assert_sync!(ParseOutput);

// ---------------------------------------------------------------------------
// Salsa: input structs
// ---------------------------------------------------------------------------

/// A single source file tracked by the incremental database.
///
/// Fields:
/// - `source` — raw UTF-8 source text.
/// - `dialect` — parsing dialect (spec §9).
/// - `options_digest` — hash of analysis options.  Full shape deferred to
///   bead cy-nk7; zero is a valid "no options" value.
#[salsa::input]
pub struct SourceFile {
    /// Raw UTF-8 source text for this file.
    #[returns(ref)]
    pub source: String,

    /// Dialect used when parsing this file.
    pub dialect: DialectMode,

    /// Hash of options that affect derived queries.
    /// Shape is stabilised in cy-nk7; zero is a valid "no options" value.
    pub options_digest: u64,
}

// ---------------------------------------------------------------------------
// Salsa: derived queries
// ---------------------------------------------------------------------------

/// Parse a [`SourceFile`] into a lossless CST.
///
/// The result is memoised; it is re-evaluated only when `source` or `dialect`
/// changes.  See [`ParseOutput`] for equality semantics.
#[salsa::tracked]
pub fn parse_cst(db: &dyn CypherDb, file: SourceFile) -> ParseOutput {
    let src = file.source(db);
    ParseOutput::new(parse(src))
}

// ---------------------------------------------------------------------------
// Salsa: database trait + concrete struct
// ---------------------------------------------------------------------------

/// Trait that all concrete databases in this workspace implement.
///
/// Using a trait lets downstream crates write `&dyn CypherDb` functions
/// that are testable against mock databases.
#[salsa::db]
pub trait CypherDb: salsa::Database {}

/// The concrete incremental database (Salsa 2022-style, spec §11.1).
///
/// ## Send + Sync via snapshots (spec §11.5)
///
/// `CypherDatabase` is `Send` but not `Sync` — `ZalsaLocal` contains
/// per-thread `UnsafeCell` state by design.  Thread-safety is achieved via
/// snapshots: `clone()` shares the `Arc<Zalsa>` backing store and produces a
/// fresh `ZalsaLocal`, allowing the clone to be sent to another thread and
/// queried concurrently.  The LSP server clones once per request; the CLI
/// never needs to.
#[salsa::db]
#[derive(Clone, Default)]
pub struct CypherDatabase {
    storage: salsa::Storage<Self>,
}

impl std::fmt::Debug for CypherDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CypherDatabase").finish_non_exhaustive()
    }
}

impl CypherDatabase {
    /// Construct a new, empty database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new [`SourceFile`] input with the given source text,
    /// using the default dialect and a zero options digest.
    pub fn new_source_file(&mut self, source: impl Into<String>) -> SourceFile {
        SourceFile::new(self, source.into(), DialectMode::default(), 0)
    }

    /// Create a new [`SourceFile`] input with explicit dialect and digest.
    pub fn new_source_file_with(
        &mut self,
        source: impl Into<String>,
        dialect: DialectMode,
        options_digest: u64,
    ) -> SourceFile {
        SourceFile::new(self, source.into(), dialect, options_digest)
    }

    /// Update the source text of an existing [`SourceFile`], bumping the
    /// Salsa revision so that derived queries are invalidated.
    pub fn set_source(&mut self, file: SourceFile, source: impl Into<String>) {
        file.set_source(self).to(source.into());
    }

    /// Update the dialect of an existing [`SourceFile`].
    pub fn set_dialect(&mut self, file: SourceFile, dialect: DialectMode) {
        file.set_dialect(self).to(dialect);
    }
}

#[salsa::db]
impl salsa::Database for CypherDatabase {}

#[salsa::db]
impl CypherDb for CypherDatabase {}

// CypherDatabase is Send (salsa adds `unsafe impl Send` via the #[salsa::db]
// macro) but not Sync.
assert_send!(CypherDatabase);

// ---------------------------------------------------------------------------
// Legacy thin facade — preserved for backward compatibility.
//
// Binary crates (cypher-cli, cypher-agent) and cypher-testkit depend on this
// API.  They will migrate to CypherDatabase in subsequent beads.
// ---------------------------------------------------------------------------

use std::sync::Mutex as StdMutex;

use cypher_diag::{Diagnostic, DiagnosticsSink};
use cypher_fmt::{FormatOptions, format_with as fmt_format_with};
use cypher_schema::{EmptySchema, SchemaProvider};
use cypher_sema::SemaOptions;
use indexmap::IndexMap;
use smol_str::SmolStr;

/// File identity within the legacy [`Database`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

#[derive(Default)]
struct LegacyInner {
    sources: IndexMap<FileId, Arc<str>>,
    dialects: IndexMap<FileId, DialectMode>,
    #[allow(dead_code)] // consumed when sema passes are wired in
    sema_opts: SemaOptions,
    next_file_id: u32,
}

/// Legacy analysis database (non-incremental).  Preserved for backward
/// compatibility while binary crates migrate to [`CypherDatabase`].
///
/// `Send + Sync` via `Mutex`-guarded interior.  The incremental replacement
/// is [`CypherDatabase`].
pub struct Database {
    inner: StdMutex<LegacyInner>,
    schema: StdMutex<Arc<dyn SchemaProvider>>,
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: StdMutex::new(LegacyInner::default()),
            schema: StdMutex::new(Arc::new(EmptySchema)),
        }
    }

    pub fn set_schema(&self, schema: Arc<dyn SchemaProvider>) {
        *self.schema.lock().expect("db mutex") = schema;
    }

    pub fn allocate_file(&self) -> FileId {
        let mut i = self.inner.lock().expect("db mutex");
        let id = FileId(i.next_file_id);
        i.next_file_id += 1;
        id
    }

    pub fn set_source(&self, file: FileId, src: impl Into<Arc<str>>) {
        let mut i = self.inner.lock().expect("db mutex");
        i.sources.insert(file, src.into());
    }

    pub fn set_dialect(&self, file: FileId, d: DialectMode) {
        let mut i = self.inner.lock().expect("db mutex");
        i.dialects.insert(file, d);
    }

    fn source_of(&self, file: FileId) -> Arc<str> {
        let i = self.inner.lock().expect("db mutex");
        i.sources
            .get(&file)
            .cloned()
            .unwrap_or_else(|| Arc::from(""))
    }

    #[must_use]
    pub fn parse(&self, file: FileId) -> Parse {
        let src = self.source_of(file);
        parse(&src)
    }

    #[must_use]
    pub fn diagnostics(&self, file: FileId) -> Vec<Diagnostic> {
        let _parse = self.parse(file);
        let sink = DiagnosticsSink::new();
        sink.into_sorted()
    }

    #[must_use]
    pub fn formatted(&self, file: FileId, opts: &FormatOptions) -> SmolStr {
        let src = self.source_of(file);
        fmt_format_with(&src, opts)
            .expect("formatter is infallible")
            .into()
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Salsa / CypherDatabase tests ---

    /// Basic construction: create a DB, create a [`SourceFile`], run the
    /// derived query, check the CST round-trips the source.
    #[test]
    fn parse_cst_basic() {
        let mut db = CypherDatabase::new();
        let file = db.new_source_file("MATCH (n) RETURN n");
        let out = parse_cst(&db, file);
        assert_eq!(
            out.parse().syntax().to_string(),
            "MATCH (n) RETURN n",
            "lossless CST round-trip"
        );
    }

    /// Calling `parse_cst` twice returns the same `Arc` (pointer equality),
    /// proving Salsa returned the cached result.
    #[test]
    fn parse_cst_cached() {
        let mut db = CypherDatabase::new();
        let file = db.new_source_file("RETURN 1");
        let out1 = parse_cst(&db, file);
        let out2 = parse_cst(&db, file);
        // Same Arc pointer → cached, not re-executed.
        assert!(
            Arc::ptr_eq(&out1.0, &out2.0),
            "second call should return cached ParseOutput"
        );
    }

    /// Modifying the source bumps the revision and causes `parse_cst` to
    /// re-execute, producing a new `Arc`.
    #[test]
    fn parse_cst_invalidates_on_source_change() {
        let mut db = CypherDatabase::new();
        let file = db.new_source_file("MATCH (n) RETURN n");

        let out1 = parse_cst(&db, file);
        assert_eq!(out1.parse().syntax().to_string(), "MATCH (n) RETURN n");

        // Mutate the source → revision bump.
        db.set_source(file, "RETURN 42");

        let out2 = parse_cst(&db, file);
        assert_eq!(out2.parse().syntax().to_string(), "RETURN 42");

        // Different Arc pointer → query was re-executed.
        assert!(
            !Arc::ptr_eq(&out1.0, &out2.0),
            "parse_cst should re-execute after source change"
        );
    }

    /// Cloning the DB creates a snapshot that can be sent to another thread.
    /// Both clones share `Arc<Zalsa>` so they see the same revision; each has
    /// its own `ZalsaLocal` allowing concurrent reads without `&mut`.
    #[test]
    fn snapshot_is_send_and_readable() {
        let mut db = CypherDatabase::new();
        let file = db.new_source_file("RETURN 1");

        // Parse once to populate the memo cache.
        let out1 = parse_cst(&db, file);
        assert_eq!(out1.parse().syntax().to_string(), "RETURN 1");

        // Clone = snapshot that can be sent to another thread.
        let snapshot = db.clone();

        // The snapshot can run queries (no &mut needed).
        let out_snap = parse_cst(&snapshot, file);
        assert_eq!(
            out_snap.parse().syntax().to_string(),
            "RETURN 1",
            "snapshot sees the same state"
        );

        // The snapshot can be sent across thread boundaries.
        let out_thread =
            std::thread::spawn(move || parse_cst(&snapshot, file).parse().syntax().to_string())
                .join()
                .expect("thread panicked");
        assert_eq!(out_thread, "RETURN 1");
    }

    /// `CypherDatabase` is `Send`; `ParseOutput` is `Send + Sync`.
    #[test]
    fn send_sync_properties() {
        fn require_send<T: Send>(_: T) {}
        fn require_send_sync<T: Send + Sync>(_: T) {}

        let db = CypherDatabase::new();
        require_send(db);

        let mut db2 = CypherDatabase::new();
        let file = db2.new_source_file("RETURN 1");
        let out = parse_cst(&db2, file);
        require_send_sync(out);
    }

    /// Empty source produces a valid (empty) CST and no parse errors.
    #[test]
    fn empty_source_ok() {
        let mut db = CypherDatabase::new();
        let file = db.new_source_file("");
        let out = parse_cst(&db, file);
        assert_eq!(out.parse().syntax().to_string(), "");
        assert!(out.parse().errors().is_empty());
    }

    // --- Legacy Database tests (backward compat) ---

    #[test]
    fn legacy_parse_through_db() {
        let db = Database::new();
        let f = db.allocate_file();
        db.set_source(f, "MATCH (n) RETURN n");
        let p = db.parse(f);
        assert_eq!(p.syntax().to_string(), "MATCH (n) RETURN n");
    }

    #[test]
    fn legacy_empty_source_is_ok() {
        let db = Database::new();
        let f = db.allocate_file();
        assert_eq!(db.parse(f).syntax().to_string(), "");
        assert!(db.diagnostics(f).is_empty());
    }
}
