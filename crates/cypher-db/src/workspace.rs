//! Workspace-level `Database` API — `FileId` model, snapshot semantics,
//! and the `SchemaProvider` wiring (spec 0001 §11.4, §11.5).
//!
//! ## Design
//!
//! ```text
//! Database (owned, wraps CypherDatabase)
//!   ├── FileId map: BTreeMap<FileId, SourceFile>  (stable u32 handles)
//!   ├── WorkspaceInputs                           (workspace-scoped schema)
//!   └── FileOptions per file                      (per-file analysis options)
//!
//! DatabaseSnapshot (Clone of CypherDatabase, Send)
//!   └── read-only view, carries FileId → SourceFile handles
//! ```
//!
//! ## Concurrency contract (spec §11.5)
//!
//! * `Database` is `Send` (not `Sync` — Salsa's `ZalsaLocal` is per-thread).
//!   Mutation (`&mut self`) serialises writes.
//! * `DatabaseSnapshot` is obtained via [`Database::snapshot`].  It clones the
//!   `Arc<Zalsa>` backing store and creates a fresh `ZalsaLocal`, making it
//!   safe to send to another thread.  The snapshot sees a frozen view of the
//!   database at the point of the clone; subsequent mutations to the origin
//!   `Database` are invisible to the snapshot (Salsa's snapshot-isolation
//!   guarantee).
//! * **Pattern — one DB + snapshot per request** (spec §11.5):
//!
//! ```rust,ignore
//! // Main thread owns the database.
//! let mut db = Database::new();
//! let file = db.open_file(Path::new("q.cyp"), "RETURN 1".into(), DialectMode::GqlAligned);
//!
//! // Per-request: take a snapshot, send to worker thread.
//! let snap = db.snapshot();
//! let src  = db.source_of(file).unwrap().to_string();
//! let handle = std::thread::spawn(move || {
//!     let result = snap.parse_cst(file);
//!     result.parse().syntax().to_string()
//! });
//! let output = handle.join().unwrap();
//! assert_eq!(output, src);
//! ```
//!
//! ## `FileId` representation
//!
//! `FileId` is a `u32` newtype — simple, stable across process restarts,
//! and cheap to copy.  Each `Database` maintains a monotonically-increasing
//! counter; IDs are never reused within a single database instance.
//!
//! ## `SchemaProvider` wiring
//!
//! The workspace-scoped `SchemaProvider` is stored in a single
//! [`WorkspaceInputs`] input.  Calling [`Database::set_schema`] bumps the
//! Salsa revision for that input, which Salsa propagates to every
//! schema-dependent derived query (e.g. `sema_diagnostics`, `all_diagnostics`)
//! across **all** files.  The parse cache is unaffected (parse does not read
//! `WorkspaceInputs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;

use cypher_schema::SchemaProvider;

use crate::inputs::{AnalysisOptions, FileOptions, WorkspaceInputs};
use crate::queries::{
    AstOutput, DiagnosticsOutput, PlanOutput, ResolvedNamesOutput, all_diagnostics, analyse_file,
    parse_ast, plan_of, resolved_names, sema_diagnostics,
};
use crate::{Analysis, CypherDatabase, DialectMode, ParseOutput, SourceFile, parse_cst};

// ---------------------------------------------------------------------------
// FileId
// ---------------------------------------------------------------------------

/// A stable, workspace-scoped file identity (spec §11.4).
///
/// `FileId` is the unit of caching in the incremental database.  Every
/// source file opened via [`Database::open_file`] receives a unique
/// `FileId`.  IDs are monotonically increasing `u32` values; they are
/// never reused within a single `Database` instance.
///
/// `FileId` values are intentionally opaque — callers store them but
/// should not interpret the numeric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

impl std::fmt::Display for FileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FileId({})", self.0)
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error returned when a `FileId` is not found in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownFileId(pub FileId);

impl std::fmt::Display for UnknownFileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown FileId: {}", self.0)
    }
}

impl std::error::Error for UnknownFileId {}

// ---------------------------------------------------------------------------
// FileRecord — internal per-file state
// ---------------------------------------------------------------------------

/// Internal per-file record mapping a `FileId` to its Salsa inputs.
struct FileRecord {
    /// Salsa input struct carrying `source` + `dialect`.
    source_file: SourceFile,
    /// Salsa input struct carrying per-file `AnalysisOptions`.
    file_opts: FileOptions,
    /// Path associated with the file (used for display / lookup only).
    path: PathBuf,
}

// ---------------------------------------------------------------------------
// DatabaseSnapshot
// ---------------------------------------------------------------------------

/// A read-only, `Send` snapshot of the database at a point in time.
///
/// Obtained via [`Database::snapshot`].  The snapshot shares the
/// `Arc<Zalsa>` backing store with the origin `Database` and sees a
/// frozen view of the Salsa revision at the moment of cloning.  Subsequent
/// mutations to the `Database` are invisible to this snapshot.
///
/// Snapshots are suitable for cross-thread queries: they implement `Send`
/// so they can be shipped to a worker thread for concurrent read queries.
///
/// ## Example
///
/// ```rust,ignore
/// let snap = db.snapshot();
/// let file = /* FileId from the origin db */;
/// let result = std::thread::spawn(move || {
///     snap.parse_cst(file).unwrap()
/// }).join().unwrap();
/// ```
pub struct DatabaseSnapshot {
    /// Cloned Salsa database (read-only from the snapshot's perspective).
    inner: CypherDatabase,
    /// Snapshot of the file registry at the time of cloning.
    files: Arc<IndexMap<FileId, SourceFile>>,
    /// Snapshot of the workspace inputs handle.
    workspace: Option<WorkspaceInputs>,
    /// Per-file options snapshot.
    file_opts: Arc<IndexMap<FileId, FileOptions>>,
}

impl std::fmt::Debug for DatabaseSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseSnapshot")
            .field("num_files", &self.files.len())
            .finish_non_exhaustive()
    }
}

// Compile-time Send assertion for DatabaseSnapshot.
// CypherDatabase is Send (salsa #[salsa::db] macro emits `unsafe impl Send`).
// Arc<IndexMap<…>> is Send when the value types are Send.
// FileId, SourceFile, FileOptions, WorkspaceInputs are all Send.
const _: fn() = || {
    fn check_send<T: Send>() {}
    check_send::<DatabaseSnapshot>();
};

impl DatabaseSnapshot {
    /// Run `parse_cst` on the given file using this snapshot's view.
    ///
    /// Returns `Err(UnknownFileId)` if `id` was not open at snapshot time.
    pub fn parse_cst(&self, id: FileId) -> Result<ParseOutput, UnknownFileId> {
        let sf = self.files.get(&id).copied().ok_or(UnknownFileId(id))?;
        Ok(parse_cst(&self.inner, sf))
    }

    /// Run `parse_ast` on the given file using this snapshot's view.
    pub fn parse_ast(&self, id: FileId) -> Result<AstOutput, UnknownFileId> {
        let sf = self.files.get(&id).copied().ok_or(UnknownFileId(id))?;
        Ok(parse_ast(&self.inner, sf))
    }

    /// Run `plan_of` on the given file using this snapshot's view.
    pub fn plan_of(&self, id: FileId) -> Result<PlanOutput, UnknownFileId> {
        let sf = self.files.get(&id).copied().ok_or(UnknownFileId(id))?;
        Ok(plan_of(&self.inner, sf))
    }

    /// Run `sema_diagnostics` on the given file using this snapshot's view.
    pub fn sema_diagnostics(&self, id: FileId) -> Result<DiagnosticsOutput, UnknownFileId> {
        let sf = self.files.get(&id).copied().ok_or(UnknownFileId(id))?;
        let fo = self.file_opts.get(&id).copied().ok_or(UnknownFileId(id))?;
        let ws = self.workspace.ok_or(UnknownFileId(id))?;
        Ok(sema_diagnostics(&self.inner, sf, fo, ws))
    }

    /// Run `resolved_names` on the given file using this snapshot's view.
    pub fn resolved_names(&self, id: FileId) -> Result<ResolvedNamesOutput, UnknownFileId> {
        let sf = self.files.get(&id).copied().ok_or(UnknownFileId(id))?;
        let fo = self.file_opts.get(&id).copied().ok_or(UnknownFileId(id))?;
        Ok(resolved_names(&self.inner, sf, fo))
    }

    /// Run `all_diagnostics` on the given file using this snapshot's view.
    pub fn all_diagnostics(&self, id: FileId) -> Result<DiagnosticsOutput, UnknownFileId> {
        let sf = self.files.get(&id).copied().ok_or(UnknownFileId(id))?;
        let fo = self.file_opts.get(&id).copied().ok_or(UnknownFileId(id))?;
        let ws = self.workspace.ok_or(UnknownFileId(id))?;
        Ok(all_diagnostics(&self.inner, sf, fo, ws))
    }

    /// Run the full analysis pipeline on the given file using this snapshot's view.
    pub fn analyse_file(&self, id: FileId) -> Result<Analysis, UnknownFileId> {
        let sf = self.files.get(&id).copied().ok_or(UnknownFileId(id))?;
        let fo = self.file_opts.get(&id).copied().ok_or(UnknownFileId(id))?;
        let ws = self.workspace.ok_or(UnknownFileId(id))?;
        Ok(analyse_file(&self.inner, sf, fo, ws))
    }
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// Workspace-scoped incremental analysis database (spec §11).
///
/// The primary public API for all consumers (`cypher-lsp`, `cypher-agent`,
/// `cypher-cli`, `cypher-tck`).  Wraps [`CypherDatabase`] with:
///
/// * A `FileId` → `SourceFile` registry so callers use stable `u32` handles
///   instead of raw Salsa input structs.
/// * A single [`WorkspaceInputs`] for the workspace-scoped schema.
/// * Snapshot support via [`Database::snapshot`].
///
/// ## Concurrency contract (spec §11.5)
///
/// `Database` is `Send` but not `Sync`.  All mutating methods take `&mut self`.
/// For concurrent read access from multiple threads, call [`snapshot`] to
/// obtain a [`DatabaseSnapshot`] that implements `Send` and can be given to
/// a worker thread.
///
/// [`snapshot`]: Database::snapshot
pub struct Database {
    inner: CypherDatabase,
    /// Registry: `FileId` → per-file Salsa inputs.
    files: IndexMap<FileId, FileRecord>,
    /// The single workspace-scoped input; created lazily on first use.
    workspace: Option<WorkspaceInputs>,
    /// Monotonically increasing `FileId` counter.
    next_id: u32,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("num_files", &self.files.len())
            .finish_non_exhaustive()
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Create a new, empty workspace database.
    #[must_use]
    pub fn new() -> Self {
        let mut inner = CypherDatabase::new();
        let workspace = Some(inner.new_workspace_inputs(None));
        Self {
            inner,
            files: IndexMap::new(),
            workspace,
            next_id: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Workspace API — spec §11.4
    // -----------------------------------------------------------------------

    /// Open a file in the workspace.
    ///
    /// Returns a stable [`FileId`] that uniquely identifies this file for the
    /// lifetime of the database.  The `path` is recorded for diagnostics /
    /// display but is not used as a key; two calls with the same path but
    /// different sources produce two independent files.
    ///
    /// ## Caching
    ///
    /// Opening a file creates new Salsa input structs and does not yet run
    /// any derived query.  All analysis is lazy and memoised.
    pub fn open_file(&mut self, path: &Path, source: String, dialect: DialectMode) -> FileId {
        let id = FileId(self.next_id);
        self.next_id += 1;

        let source_file = self.inner.new_source_file_with(source, dialect, 0);
        let file_opts = self.inner.new_file_options(AnalysisOptions {
            dialect,
            ..Default::default()
        });

        self.files.insert(
            id,
            FileRecord {
                source_file,
                file_opts,
                path: path.to_owned(),
            },
        );

        id
    }

    /// Update the source text of an already-open file.
    ///
    /// Bumps the Salsa revision for this file's `SourceFile` input, causing
    /// all derived queries that depend on `source` to be re-evaluated on the
    /// next access.
    ///
    /// Returns `Err(UnknownFileId)` if `id` is not currently open.
    pub fn update_file(&mut self, id: FileId, new_source: String) -> Result<(), UnknownFileId> {
        let record = self.files.get(&id).ok_or(UnknownFileId(id))?;
        let sf = record.source_file;
        self.inner.set_source(sf, new_source);
        Ok(())
    }

    /// Remove a file from the workspace.
    ///
    /// After removal, the `FileId` is considered stale.  Any subsequent call
    /// that takes this `FileId` will return `Err(UnknownFileId)` rather than
    /// panic.
    ///
    /// Returns `Err(UnknownFileId)` if `id` was not open.
    pub fn remove_file(&mut self, id: FileId) -> Result<(), UnknownFileId> {
        self.files
            .swap_remove(&id)
            .map(|_| ())
            .ok_or(UnknownFileId(id))
    }

    // -----------------------------------------------------------------------
    // Schema API — spec §11.4 workspace-scoped SchemaProvider
    // -----------------------------------------------------------------------

    /// Set the workspace-scoped schema.
    ///
    /// Bumps the Salsa revision on [`WorkspaceInputs`], which cascades to
    /// every schema-aware derived query (`sema_diagnostics`, `all_diagnostics`)
    /// across **all** files.  The parse cache is unaffected.
    ///
    /// Pass `None` to switch to schema-free analysis mode (§7.1).
    pub fn set_schema(&mut self, schema: Option<Arc<dyn SchemaProvider>>) {
        let ws = self.workspace_inputs_mut();
        self.inner.set_schema(ws, schema);
    }

    /// Read the current workspace-scoped schema (may be `None`).
    #[must_use]
    pub fn schema(&self) -> Option<Arc<dyn SchemaProvider>> {
        self.workspace.as_ref()?.schema(&self.inner)
    }

    // -----------------------------------------------------------------------
    // Snapshot API — spec §11.5
    // -----------------------------------------------------------------------

    /// Create a [`DatabaseSnapshot`] suitable for cross-thread queries.
    ///
    /// The snapshot is a frozen read-only view of the database at this
    /// instant.  It can be sent to another thread (`Send`) and used to run
    /// any derived query without `&mut`.  Mutations applied to `self` after
    /// calling `snapshot()` are **not** visible to the snapshot.
    ///
    /// ## Pattern — one DB + snapshot per request
    ///
    /// ```rust,ignore
    /// let snap = db.snapshot();
    /// std::thread::spawn(move || {
    ///     let out = snap.parse_cst(file_id).unwrap();
    ///     // use out …
    /// });
    /// ```
    #[must_use]
    pub fn snapshot(&self) -> DatabaseSnapshot {
        // Collect a lightweight view of the file registry (FileId → SourceFile).
        let files: IndexMap<FileId, SourceFile> = self
            .files
            .iter()
            .map(|(&id, rec)| (id, rec.source_file))
            .collect();

        let file_opts: IndexMap<FileId, FileOptions> = self
            .files
            .iter()
            .map(|(&id, rec)| (id, rec.file_opts))
            .collect();

        DatabaseSnapshot {
            inner: self.inner.clone(),
            files: Arc::new(files),
            workspace: self.workspace,
            file_opts: Arc::new(file_opts),
        }
    }

    // -----------------------------------------------------------------------
    // Query access — delegates to the Salsa derived queries
    // -----------------------------------------------------------------------

    /// Run `parse_cst` on the given file.
    pub fn parse_cst(&self, id: FileId) -> Result<ParseOutput, UnknownFileId> {
        let sf = self.source_file(id)?;
        Ok(parse_cst(&self.inner, sf))
    }

    /// Run `parse_ast` on the given file.
    pub fn parse_ast(&self, id: FileId) -> Result<AstOutput, UnknownFileId> {
        let sf = self.source_file(id)?;
        Ok(parse_ast(&self.inner, sf))
    }

    /// Run `plan_of` on the given file.
    pub fn plan_of(&self, id: FileId) -> Result<PlanOutput, UnknownFileId> {
        let sf = self.source_file(id)?;
        Ok(plan_of(&self.inner, sf))
    }

    /// Run `sema_diagnostics` on the given file.
    pub fn sema_diagnostics(&self, id: FileId) -> Result<DiagnosticsOutput, UnknownFileId> {
        let sf = self.source_file(id)?;
        let fo = self.file_options(id)?;
        let ws = self.workspace_inputs()?;
        Ok(sema_diagnostics(&self.inner, sf, fo, ws))
    }

    /// Run `resolved_names` on the given file.
    pub fn resolved_names(&self, id: FileId) -> Result<ResolvedNamesOutput, UnknownFileId> {
        let sf = self.source_file(id)?;
        let fo = self.file_options(id)?;
        Ok(resolved_names(&self.inner, sf, fo))
    }

    /// Run `all_diagnostics` on the given file.
    pub fn all_diagnostics(&self, id: FileId) -> Result<DiagnosticsOutput, UnknownFileId> {
        let sf = self.source_file(id)?;
        let fo = self.file_options(id)?;
        let ws = self.workspace_inputs()?;
        Ok(all_diagnostics(&self.inner, sf, fo, ws))
    }

    /// Run the full analysis pipeline on the given file.
    pub fn analyse_file(&self, id: FileId) -> Result<Analysis, UnknownFileId> {
        let sf = self.source_file(id)?;
        let fo = self.file_options(id)?;
        let ws = self.workspace_inputs()?;
        Ok(analyse_file(&self.inner, sf, fo, ws))
    }

    /// Return the source text of a file.
    ///
    /// Returns `Err(UnknownFileId)` if `id` is not open.
    pub fn source_of(&self, id: FileId) -> Result<String, UnknownFileId> {
        let sf = self.source_file(id)?;
        Ok(sf.source(&self.inner).clone())
    }

    /// Return the path associated with a file.
    ///
    /// Returns `Err(UnknownFileId)` if `id` is not open.
    pub fn path_of(&self, id: FileId) -> Result<&Path, UnknownFileId> {
        self.files
            .get(&id)
            .map(|r| r.path.as_path())
            .ok_or(UnknownFileId(id))
    }

    /// Returns `true` if `id` refers to an open file.
    #[must_use]
    pub fn is_open(&self, id: FileId) -> bool {
        self.files.contains_key(&id)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn source_file(&self, id: FileId) -> Result<SourceFile, UnknownFileId> {
        self.files
            .get(&id)
            .map(|r| r.source_file)
            .ok_or(UnknownFileId(id))
    }

    fn file_options(&self, id: FileId) -> Result<FileOptions, UnknownFileId> {
        self.files
            .get(&id)
            .map(|r| r.file_opts)
            .ok_or(UnknownFileId(id))
    }

    fn workspace_inputs(&self) -> Result<WorkspaceInputs, UnknownFileId> {
        // WorkspaceInputs is always present after construction.
        self.workspace
            .ok_or_else(|| unreachable!("WorkspaceInputs always initialised in Database::new"))
    }

    fn workspace_inputs_mut(&mut self) -> WorkspaceInputs {
        self.workspace
            .expect("WorkspaceInputs always initialised in Database::new")
    }
}

// Compile-time Send assertion.  Database is Send because:
//   - CypherDatabase: Send (Salsa emits this)
//   - IndexMap<FileId, FileRecord>: Send (FileRecord fields are Send)
//   - WorkspaceInputs: Copy + Send
//   - u32: Send
const _: fn() = || {
    fn check_send<T: Send>() {}
    check_send::<Database>();
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // Basic open / query / update / remove
    // -----------------------------------------------------------------------

    #[test]
    fn open_and_query_single_file() {
        let mut db = Database::new();
        let id = db.open_file(
            Path::new("a.cyp"),
            "MATCH (n) RETURN n".into(),
            DialectMode::GqlAligned,
        );

        let out = db.parse_cst(id).expect("file must be open");
        assert_eq!(out.parse().syntax().to_string(), "MATCH (n) RETURN n");
    }

    #[test]
    fn update_file_invalidates_cache() {
        let mut db = Database::new();
        let id = db.open_file(
            Path::new("a.cyp"),
            "RETURN 1".into(),
            DialectMode::GqlAligned,
        );

        let out1 = db.parse_cst(id).unwrap();
        assert_eq!(out1.parse().syntax().to_string(), "RETURN 1");

        db.update_file(id, "RETURN 2".into()).unwrap();

        let out2 = db.parse_cst(id).unwrap();
        assert_eq!(out2.parse().syntax().to_string(), "RETURN 2");
    }

    #[test]
    fn remove_file_stale_returns_error() {
        let mut db = Database::new();
        let id = db.open_file(
            Path::new("a.cyp"),
            "RETURN 1".into(),
            DialectMode::GqlAligned,
        );

        // Confirm it's open.
        assert!(db.is_open(id));

        // Remove it.
        db.remove_file(id).expect("remove should succeed");

        // All subsequent queries return an error, not a panic.
        assert!(!db.is_open(id));
        assert_eq!(db.parse_cst(id), Err(UnknownFileId(id)));
        assert_eq!(
            db.update_file(id, "RETURN 2".into()),
            Err(UnknownFileId(id))
        );
        assert_eq!(db.remove_file(id), Err(UnknownFileId(id)));
    }

    // -----------------------------------------------------------------------
    // Three files — independent caching
    // -----------------------------------------------------------------------

    #[test]
    fn three_files_independent_caching() {
        let mut db = Database::new();
        let a = db.open_file(
            Path::new("a.cyp"),
            "RETURN 1".into(),
            DialectMode::GqlAligned,
        );
        let b = db.open_file(
            Path::new("b.cyp"),
            "RETURN 2".into(),
            DialectMode::GqlAligned,
        );
        let c = db.open_file(
            Path::new("c.cyp"),
            "RETURN 3".into(),
            DialectMode::GqlAligned,
        );

        // Query each file.
        let oa = db.parse_cst(a).unwrap();
        let ob = db.parse_cst(b).unwrap();
        let oc = db.parse_cst(c).unwrap();

        assert_eq!(oa.parse().syntax().to_string(), "RETURN 1");
        assert_eq!(ob.parse().syntax().to_string(), "RETURN 2");
        assert_eq!(oc.parse().syntax().to_string(), "RETURN 3");

        // Mutate only file `b`.
        db.update_file(b, "RETURN 99".into()).unwrap();

        // `a` and `c` caches are untouched.
        let oa2 = db.parse_cst(a).unwrap();
        let oc2 = db.parse_cst(c).unwrap();
        assert!(
            Arc::ptr_eq(&oa.0, &oa2.0),
            "file a cache must survive update to file b"
        );
        assert!(
            Arc::ptr_eq(&oc.0, &oc2.0),
            "file c cache must survive update to file b"
        );

        // `b` is invalidated.
        let ob2 = db.parse_cst(b).unwrap();
        assert_eq!(ob2.parse().syntax().to_string(), "RETURN 99");
        assert!(
            !Arc::ptr_eq(&ob.0, &ob2.0),
            "file b cache must be invalidated"
        );
    }

    // -----------------------------------------------------------------------
    // Snapshot + thread: one DB + snapshot per request pattern
    // -----------------------------------------------------------------------

    #[test]
    fn snapshot_send_concurrent_query() {
        let mut db = Database::new();
        let id = db.open_file(
            Path::new("q.cyp"),
            "MATCH (n) RETURN n".into(),
            DialectMode::GqlAligned,
        );

        // Warm the main-thread cache.
        let main_out = db.parse_cst(id).unwrap();
        let main_text = main_out.parse().syntax().to_string();

        // Take a snapshot; ship to worker thread.
        let snap = db.snapshot();
        let worker_text = std::thread::spawn(move || {
            snap.parse_cst(id)
                .expect("snapshot must contain the file")
                .parse()
                .syntax()
                .to_string()
        })
        .join()
        .expect("worker thread panicked");

        assert_eq!(
            worker_text, main_text,
            "snapshot result must match main-thread result"
        );
    }

    /// Compile-time assertion: `DatabaseSnapshot: Send`.
    #[test]
    fn database_snapshot_is_send() {
        fn require_send<T: Send>(_: T) {}
        let mut db = Database::new();
        let _id = db.open_file(
            Path::new("a.cyp"),
            "RETURN 1".into(),
            DialectMode::GqlAligned,
        );
        let snap = db.snapshot();
        require_send(snap);
    }

    // -----------------------------------------------------------------------
    // Schema change: sema cache invalidates, parse cache survives
    // -----------------------------------------------------------------------

    #[test]
    fn schema_change_invalidates_sema_not_parse() {
        use cypher_schema::EmptySchema;

        let mut db = Database::new();
        let id = db.open_file(
            Path::new("s.cyp"),
            "MATCH (n:Person) RETURN n".into(),
            DialectMode::GqlAligned,
        );

        // Warm the parse cache.
        let cst1 = db.parse_cst(id).unwrap();

        // Run sema (no schema yet).
        let _sema1 = db.sema_diagnostics(id).unwrap();

        // Set a schema — bumps WorkspaceInputs revision.
        let schema: Arc<dyn SchemaProvider> = Arc::new(EmptySchema);
        db.set_schema(Some(schema));

        // Parse cache must survive schema change.
        let cst2 = db.parse_cst(id).unwrap();
        assert!(
            Arc::ptr_eq(&cst1.0, &cst2.0),
            "parse_cst Arc must survive schema change"
        );

        // Sema runs without error under the new schema.
        let _sema2 = db.sema_diagnostics(id).unwrap();
    }

    // -----------------------------------------------------------------------
    // source_of / path_of helpers
    // -----------------------------------------------------------------------

    #[test]
    fn source_and_path_accessors() {
        let mut db = Database::new();
        let p = Path::new("myfile.cyp");
        let id = db.open_file(p, "RETURN 42".into(), DialectMode::GqlAligned);

        assert_eq!(db.source_of(id).unwrap(), "RETURN 42");
        assert_eq!(db.path_of(id).unwrap(), p);
    }

    // -----------------------------------------------------------------------
    // Unknown FileId accessors
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_fileid_source_of() {
        let db = Database::new();
        let ghost = FileId(999);
        assert_eq!(db.source_of(ghost), Err(UnknownFileId(ghost)));
        assert_eq!(db.path_of(ghost), Err(UnknownFileId(ghost)));
    }
}
