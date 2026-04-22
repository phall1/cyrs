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
use cypher_syntax::{TextEdit, incremental_reparse};

use crate::inputs::{AnalysisOptions, FileOptions, WorkspaceInputs};
use crate::options::DatabaseOptions;
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
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) on the tuple — consumers must
/// match it with `..` in patterns.  The inner `FileId` remains
/// accessible via the public `.0` field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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

/// A freed pair of Salsa input handles retained for reuse by a subsequent
/// [`Database::open_file`] call.
///
/// Salsa 0.26 does not expose a public API for deleting input structs, so
/// once a `SourceFile` / `FileOptions` is allocated its internal slot in
/// the Salsa interner persists for the lifetime of the database.  Without
/// recycling, an LSP-style workload that churns file IDs (open → edit →
/// close) would grow Salsa's input table unboundedly and violate the
/// spec §11.6 steady-state RSS bound.
///
/// To respect the spec bound we pool the handles: `remove_file` pushes
/// the pair into [`Database::free_slots`] and resets the source to empty
/// to free the backing `String`; `open_file` prefers to pop a free slot
/// and reset its fields before allocating a fresh one.  The pool's
/// steady-state size is bounded by the peak number of simultaneously-open
/// files, which is naturally bounded by realistic client behaviour.
struct FreeSlot {
    source_file: SourceFile,
    file_opts: FileOptions,
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
    /// Pool of Salsa input handles freed by [`remove_file`] and available
    /// for reuse on the next [`open_file`].  See [`FreeSlot`] for rationale
    /// (spec §11.6 steady-state RSS bound).
    ///
    /// [`remove_file`]: Database::remove_file
    /// [`open_file`]: Database::open_file
    free_slots: Vec<FreeSlot>,
    /// The single workspace-scoped input; created lazily on first use.
    workspace: Option<WorkspaceInputs>,
    /// Monotonically increasing `FileId` counter.
    next_id: u32,
    /// LRU options captured at construction (immutable).
    #[allow(dead_code)]
    options: DatabaseOptions,
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

    /// Create a new, empty workspace database with default LRU caps (256).
    #[must_use]
    pub fn new() -> Self {
        Self::with_options(DatabaseOptions::default())
    }

    /// Create a new, empty workspace database with the given [`DatabaseOptions`].
    ///
    /// LRU capacities in `opts` are applied via Salsa's runtime
    /// `set_lru_capacity` API immediately after construction.  The options
    /// are immutable after this point.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use cypher_db::{Database, DatabaseOptions};
    ///
    /// let db = Database::with_options(DatabaseOptions {
    ///     parse_lru: 512,
    ///     sema_lru: 128,
    ///     ..DatabaseOptions::default()
    /// });
    /// ```
    #[must_use]
    pub fn with_options(opts: DatabaseOptions) -> Self {
        let mut inner = CypherDatabase::new();

        // Apply runtime LRU capacity adjustments.
        // The compile-time default is 256 (encoded in #[salsa::tracked(lru = 256)]).
        // We always call set_lru_capacity so that non-default values take effect.
        crate::set_parse_cst_lru(&mut inner, opts.parse_lru);
        crate::queries::set_resolved_names_lru(&mut inner, opts.sema_lru);
        crate::queries::set_sema_diagnostics_lru(&mut inner, opts.sema_lru);
        crate::queries::set_plan_of_lru(&mut inner, opts.plan_lru);

        let workspace = Some(inner.new_workspace_inputs(None));
        Self {
            inner,
            files: IndexMap::new(),
            free_slots: Vec::new(),
            workspace,
            next_id: 0,
            options: opts,
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

        let options = AnalysisOptions {
            dialect,
            ..Default::default()
        };

        let (source_file, file_opts) = if let Some(slot) = self.free_slots.pop() {
            // Recycle the pooled Salsa handles: reset all fields so the slot
            // behaves like a freshly-allocated input from the perspective of
            // derived queries.  Keeps Salsa's input-struct interner bounded
            // under LSP-style FileId churn (spec §11.6, bead cy-bh5).
            self.inner.set_source(slot.source_file, source);
            self.inner.set_dialect(slot.source_file, dialect);
            self.inner.set_options(slot.file_opts, options);
            (slot.source_file, slot.file_opts)
        } else {
            let source_file = self.inner.new_source_file_with(source, dialect, 0);
            let file_opts = self.inner.new_file_options(options);
            (source_file, file_opts)
        };

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

    /// Apply a single-range text edit to an already-open file (cy-zv0, spec §11).
    ///
    /// This is the incremental-edit entry point that `textDocument/didChange`
    /// and agent edit flows should prefer over [`update_file`] when only a
    /// byte range changed. The API is shaped so that a future sub-tree
    /// reparse (see `cypher_syntax::edit::incremental_reparse`) can plug in
    /// underneath without breaking callers.
    ///
    /// # Current implementation
    ///
    /// Today the underlying [`incremental_reparse`] is a whole-file reparse
    /// fallback, so the cache-invalidation behaviour is identical to
    /// `update_file`. The observable difference is:
    ///
    /// - Callers pass a [`TextEdit`] value (range + replacement) instead of
    ///   re-materialising the full source, so they don't pay an extra
    ///   `String` allocation for the unchanged prefix / suffix.
    /// - The edit is applied to the current source held by Salsa, not to a
    ///   caller-managed copy, so there is no opportunity for the caller's
    ///   local mirror to drift out of sync.
    ///
    /// Once the smart path lands (tracked as a follow-up bead to cy-zv0),
    /// `edit_file` will become strictly sub-linear in file size for small
    /// edits. Callers do not need to change.
    ///
    /// Returns `Err(UnknownFileId)` if `id` is not currently open.
    ///
    /// [`update_file`]: Database::update_file
    pub fn edit_file(&mut self, id: FileId, edit: &TextEdit) -> Result<(), UnknownFileId> {
        let record = self.files.get(&id).ok_or(UnknownFileId(id))?;
        let sf = record.source_file;

        // Pull the current tree from Salsa so `incremental_reparse` gets a
        // real `SyntaxNode` to dispatch on.  The smart path (when it lands)
        // will reuse the green tree rather than the source string; calling
        // parse_cst here warms the memo and — critically — pins the
        // `Arc<Parse>` that the future sub-tree splicer needs.
        let parse_out = parse_cst(&self.inner, sf);
        let old_tree = parse_out.parse().syntax();

        // Dispatch through the edit crate. Today this is a whole-file
        // reparse fallback; the return value's source string is the new
        // canonical text, which we feed back into Salsa to bump the
        // revision and invalidate downstream queries.
        let new_parse = incremental_reparse(&old_tree, edit);
        let new_source = new_parse.syntax().to_string();

        // Salsa will detect whether the source actually changed and skip
        // downstream re-execution via the standard revision-bump protocol.
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
        let record = self.files.swap_remove(&id).ok_or(UnknownFileId(id))?;

        // Release the backing source string immediately so a long-lived pool
        // entry does not pin a large `String` allocation (spec §11.6).  The
        // Salsa revision bump here is harmless: no derived query will read
        // this `SourceFile` until the slot is recycled, at which point
        // `open_file` sets the new source and bumps the revision again.
        self.inner.set_source(record.source_file, String::new());

        self.free_slots.push(FreeSlot {
            source_file: record.source_file,
            file_opts: record.file_opts,
        });

        Ok(())
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

    /// Iterate every currently-open [`FileId`] in insertion order.
    ///
    /// Needed by cross-file analyses (workspace symbols / references /
    /// goto-definition, spec §14.2, bead cy-kkw) so the navigation
    /// engine can walk every member file of the workspace without the
    /// caller having to track its own file list alongside the
    /// [`Database`].
    pub fn file_ids(&self) -> impl Iterator<Item = FileId> + '_ {
        self.files.keys().copied()
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

    // -----------------------------------------------------------------------
    // edit_file (cy-zv0)
    // -----------------------------------------------------------------------

    #[test]
    fn edit_file_applies_edit_and_invalidates_cache() {
        use cypher_syntax::{TextEdit, TextRange, TextSize};

        let mut db = Database::new();
        let id = db.open_file(
            Path::new("e.cyp"),
            "RETURN 1".into(),
            DialectMode::GqlAligned,
        );
        let before = db.parse_cst(id).unwrap();
        assert_eq!(before.parse().syntax().to_string(), "RETURN 1");

        // Replace "1" (byte 7..8) with "42".
        let edit = TextEdit::replace(
            TextRange::new(TextSize::new(7), TextSize::new(8)),
            "42",
        );
        db.edit_file(id, &edit).unwrap();

        let after = db.parse_cst(id).unwrap();
        assert_eq!(after.parse().syntax().to_string(), "RETURN 42");
        assert_eq!(db.source_of(id).unwrap(), "RETURN 42");
        assert!(
            !Arc::ptr_eq(&before.0, &after.0),
            "edit_file must bump the Salsa revision → new Arc"
        );
    }

    #[test]
    fn edit_file_unknown_fileid() {
        use cypher_syntax::{TextEdit, TextSize};

        let mut db = Database::new();
        let ghost = FileId(999);
        let edit = TextEdit::insert(TextSize::new(0), "x");
        assert_eq!(db.edit_file(ghost, &edit), Err(UnknownFileId(ghost)));
    }

    #[test]
    fn edit_file_preserves_other_files_cache() {
        use cypher_syntax::{TextEdit, TextRange, TextSize};

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

        let oa = db.parse_cst(a).unwrap();
        let ob = db.parse_cst(b).unwrap();

        // Edit file `a`.
        let edit = TextEdit::replace(
            TextRange::new(TextSize::new(7), TextSize::new(8)),
            "99",
        );
        db.edit_file(a, &edit).unwrap();

        // File b's cache must survive.
        let ob2 = db.parse_cst(b).unwrap();
        assert!(
            Arc::ptr_eq(&ob.0, &ob2.0),
            "file b cache must survive edit to file a"
        );

        // File a reflects the edit.
        let oa2 = db.parse_cst(a).unwrap();
        assert!(!Arc::ptr_eq(&oa.0, &oa2.0));
        assert_eq!(oa2.parse().syntax().to_string(), "RETURN 99");
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
    // DatabaseOptions / with_options — bead cy-31b
    // -----------------------------------------------------------------------

    /// `Database::with_options` constructs successfully and the resulting DB
    /// operates correctly (`parse_lru` = 2 stress test).
    #[test]
    fn with_options_parse_lru_2() {
        use crate::DatabaseOptions;

        // parse_lru = 2 means only 2 parse_cst results are kept.
        // This test verifies the API compiles and the DB is functional.
        let db = Database::with_options(DatabaseOptions {
            parse_lru: 2,
            sema_lru: 2,
            plan_lru: 2,
            formatted_lru: 2,
        });

        // Verify the DB is usable.
        assert_eq!(db.files.len(), 0);
    }

    /// `Database::with_options` with `parse_lru` = 2: open 3 files, query each.
    /// The LRU eviction will drop older cached values, so querying is still
    /// correct (Salsa recomputes on cache miss) but the cache is bounded.
    #[test]
    fn with_options_lru_2_three_files() {
        use crate::DatabaseOptions;

        let mut db = Database::with_options(DatabaseOptions {
            parse_lru: 2,
            sema_lru: 2,
            plan_lru: 2,
            ..DatabaseOptions::default()
        });

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

        // All three files parse correctly even with a tiny LRU cap.
        assert_eq!(
            db.parse_cst(a).unwrap().parse().syntax().to_string(),
            "RETURN 1"
        );
        assert_eq!(
            db.parse_cst(b).unwrap().parse().syntax().to_string(),
            "RETURN 2"
        );
        assert_eq!(
            db.parse_cst(c).unwrap().parse().syntax().to_string(),
            "RETURN 3"
        );

        // Re-query all after the cap has been exceeded — Salsa recomputes on
        // cache miss, so results must still be correct.
        assert_eq!(
            db.parse_cst(a).unwrap().parse().syntax().to_string(),
            "RETURN 1"
        );
        assert_eq!(
            db.parse_cst(b).unwrap().parse().syntax().to_string(),
            "RETURN 2"
        );
        assert_eq!(
            db.parse_cst(c).unwrap().parse().syntax().to_string(),
            "RETURN 3"
        );
    }

    /// `Database::new()` uses default options (`parse_lru` = 256).
    #[test]
    fn database_new_uses_default_options() {
        use crate::DatabaseOptions;
        let default_opts = DatabaseOptions::default();
        assert_eq!(default_opts.parse_lru, 256);
        assert_eq!(default_opts.sema_lru, 256);
        assert_eq!(default_opts.plan_lru, 256);
        assert_eq!(default_opts.formatted_lru, 256);

        // Verify Database::new() still works.
        let db = Database::new();
        assert_eq!(db.files.len(), 0);
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
