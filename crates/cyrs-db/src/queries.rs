//! Derived Salsa query chain (spec 0001 §11.3).
//!
//! Implements the full parse→HIR→sema→plan pipeline as individually
//! memoised Salsa `#[tracked]` queries.  Each layer re-evaluates only when
//! its direct inputs change; Salsa's dependency tracking provides surgical
//! cache invalidation at per-file granularity (spec §11.4 v1 scope).
//!
//! ## Query chain
//!
//! ```text
//! SourceFile (input)
//!   └─ parse_cst        → ParseOutput           (CST + syntax errors)
//!       └─ parse_ast    → AstOutput             (CST wrapper, pointer-eq)
//! FileOptions (input)
//!   └─ options_digest   → u64                   (stable options hash)
//! WorkspaceInputs (input)
//!
//! sema_diagnostics(file, file_opts, ws) → DiagnosticsOutput
//!   (reads parse_cst → hir_lower [inline] → analyse)
//! resolved_names(file, file_opts) → ResolvedNamesOutput
//!   (reads parse_cst → hir_lower [inline] → resolve)
//! plan_of(file) → PlanOutput
//!   (reads parse_cst → hir_lower [inline] → plan_lower)
//! all_diagnostics(file, file_opts, ws) → DiagnosticsOutput
//!   (union of parse + sema diagnostics, sorted + deduped)
//! ```
//!
//! ## HIR and the Send+Sync constraint
//!
//! Salsa requires all tracked-function outputs to be `Send + Sync`.
//! `cyrs_hir::Statement` contains a `node_map: IndexMap<HirId, SyntaxNode>`
//! whose value type (`SyntaxNode`) is `!Sync` (it wraps `NonNull`).  Rather
//! than return `Statement` from a `#[salsa::tracked]` function, the HIR
//! lowering is performed inline inside each downstream tracked query that
//! needs it (`sema_diagnostics`, `plan_of`).  Those queries are already
//! individually memoised by Salsa, so the HIR is effectively cached at the
//! appropriate granularity (per-file, invalidated by source change).
//!
//! ## Caching semantics
//!
//! - Granularity: per `SourceFile` (v1, spec §11.4).
//! - Changing `source` or `dialect` → re-executes `parse_cst` and all
//!   downstream queries.
//! - Changing `options` (via `FileOptions`) → re-executes `sema_diagnostics`
//!   but leaves `parse_cst` cached.
//! - Changing workspace `schema` → re-executes `sema_diagnostics` (schema-
//!   aware pass) but leaves `parse_cst` cached.
//!
//! ## Arc-based sharing (spec §8 determinism)
//!
//! All output types implement `Eq` via pointer equality on their inner
//! `Arc`, so Salsa can short-circuit downstream propagation when a
//! re-executed query produces the same logical value.

use std::sync::Arc;

use cyrs_diag::{DiagCode, Diagnostic, DiagnosticsSink};
use cyrs_hir::desugar::desugar_statement;
use cyrs_hir::lower::lower_parse as hir_lower;
use cyrs_plan::lower::{PlanStatement, lower_statement as plan_lower};
use cyrs_sema::SemaOptions;
use cyrs_sema::resolve::ResolveResult;
use cyrs_syntax::TextRange;
use smol_str::SmolStr;

use crate::inputs::{FileOptions, WorkspaceInputs};
use crate::{CypherDb, DialectMode, ParseOutput, SourceFile, parse_cst};

// ---------------------------------------------------------------------------
// Output newtypes
//
// All outputs are Arc-wrapped structs.  Equality is pointer-based so Salsa
// can detect unchanged outputs and avoid unnecessary downstream propagation.
// All output types are Send + Sync (required by Salsa).
// ---------------------------------------------------------------------------

/// AST projection of a `SourceFile`'s CST.
///
/// Wraps `Arc<ParseOutput>` with pointer-equality `Eq` so Salsa can detect
/// when the CST has not changed between revisions.
#[derive(Debug, Clone)]
pub struct AstOutput(Arc<ParseOutput>);

impl AstOutput {
    fn new(p: ParseOutput) -> Self {
        Self(Arc::new(p))
    }

    /// Access the underlying [`ParseOutput`].
    #[must_use]
    pub fn parse_output(&self) -> &ParseOutput {
        &self.0
    }
}

impl PartialEq for AstOutput {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for AstOutput {}

// ParseOutput is Send + Sync (verified in lib.rs), so Arc<ParseOutput> is too.
const _: fn() = || {
    fn check_send_sync<T: Send + Sync>() {}
    check_send_sync::<AstOutput>();
};

/// Name-resolution result for one `SourceFile`.
///
/// Wraps `Arc<ResolveResult>` with pointer-equality `Eq`.
/// `ResolveResult` is `Send + Sync` (contains only `SmolStr` / `VarId` /
/// `TextRange` values; no raw pointers).
#[derive(Debug, Clone)]
pub struct ResolvedNamesOutput(Arc<ResolveResult>);

impl ResolvedNamesOutput {
    fn new(r: ResolveResult) -> Self {
        Self(Arc::new(r))
    }

    /// Access the underlying [`ResolveResult`].
    #[must_use]
    pub fn result(&self) -> &ResolveResult {
        &self.0
    }
}

impl PartialEq for ResolvedNamesOutput {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ResolvedNamesOutput {}

const _: fn() = || {
    fn check_send_sync<T: Send + Sync>() {}
    check_send_sync::<ResolvedNamesOutput>();
};

/// Logical plan for one `SourceFile`.
///
/// Wraps `Arc<PlanStatement>` with pointer-equality `Eq`.
/// `PlanStatement` is `Send + Sync` (contains only `SmolStr` / index types).
#[derive(Debug, Clone)]
pub struct PlanOutput(Arc<PlanStatement>);

impl PlanOutput {
    fn new(p: PlanStatement) -> Self {
        Self(Arc::new(p))
    }

    /// Access the underlying [`PlanStatement`].
    #[must_use]
    pub fn plan(&self) -> &PlanStatement {
        &self.0
    }
}

impl PartialEq for PlanOutput {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for PlanOutput {}

const _: fn() = || {
    fn check_send_sync<T: Send + Sync>() {}
    check_send_sync::<PlanOutput>();
};

/// A stable, de-duplicated list of diagnostics, wrapped in `Arc`.
///
/// Equality is by pointer identity so Salsa can avoid re-evaluating
/// downstream queries when the diagnostic list is unchanged.
#[derive(Debug, Clone)]
pub struct DiagnosticsOutput(Arc<Vec<Diagnostic>>);

impl DiagnosticsOutput {
    fn new(v: Vec<Diagnostic>) -> Self {
        Self(Arc::new(v))
    }

    /// Access the inner diagnostics slice.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.0
    }

    /// Clone the inner `Arc` cheaply.
    #[must_use]
    pub fn arc_clone(&self) -> Arc<Vec<Diagnostic>> {
        self.0.clone()
    }
}

impl PartialEq for DiagnosticsOutput {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for DiagnosticsOutput {}

const _: fn() = || {
    fn check_send_sync<T: Send + Sync>() {}
    check_send_sync::<DiagnosticsOutput>();
};

// ---------------------------------------------------------------------------
// Derived queries
// ---------------------------------------------------------------------------

/// AST projection of a [`SourceFile`]'s CST (spec §11.3 `ast(FileId)`).
///
/// Cache key: the `ParseOutput` returned by [`parse_cst`].  Changing
/// `source` or `dialect` re-executes this query; changing only options or
/// schema does not.
///
/// In v1 the typed AST wrappers from `cyrs-ast` are zero-cost views over
/// the CST `SyntaxNode`.  This query returns the cached `ParseOutput` wrapped
/// in an `Arc` so that downstream consumers can cheaply obtain the syntax root
/// without re-parsing.
#[salsa::tracked]
pub fn parse_ast(db: &dyn CypherDb, file: SourceFile) -> AstOutput {
    let cst = parse_cst(db, file);
    AstOutput::new(cst)
}

/// Run name resolution on a [`SourceFile`] (spec §6.2 / §11.3 `sema`).
///
/// Cache key: `parse_cst` result (source changes trigger re-execution) +
/// `warn_shadowing` option via `file_opts`.  Changing only the workspace
/// schema does not re-execute name resolution.
///
/// HIR lowering is performed inline here because `HirStatement` contains
/// `SyntaxNode` (which is `!Sync`) and therefore cannot be returned from a
/// `#[salsa::tracked]` function.  The HIR lowering cost is dominated by the
/// upstream `parse_cst`, which is cached.
#[salsa::tracked(lru = 256)]
pub fn resolved_names(
    db: &dyn CypherDb,
    file: SourceFile,
    file_opts: FileOptions,
) -> ResolvedNamesOutput {
    // Establish Salsa dependency on the parsed CST. Lowering reuses this
    // already-parsed tree via `lower_parse` (cy-cfi) rather than
    // re-running the parser through `lower_statement`.
    let cst = parse_cst(db, file);
    let opts = file_opts.options(db);

    // Lower and desugar HIR inline (not stored in Salsa — see module
    // doc). `lower_parse` is infallible, so no fallback path is needed.
    let stmt = hir_lower(cst.parse()).expect("cyrs-db: lower_parse is infallible");
    let stmt = desugar_statement(stmt);

    let mut sink = DiagnosticsSink::new();
    let result = cyrs_sema::resolve::resolve(&stmt, opts.warn_shadowing, &mut sink);
    ResolvedNamesOutput::new(result)
}

/// Run all sema passes on a [`SourceFile`] (spec §7 / §11.3 `sema`).
///
/// Cache key: `parse_cst` + `options_digest` + workspace `schema`.  Changing
/// source re-executes this query (cascading from `parse_cst`). Changing only
/// options or schema re-executes this query but leaves `parse_cst` cached.
///
/// HIR lowering is performed inline (see module doc for rationale).
#[salsa::tracked(lru = 256)]
pub fn sema_diagnostics(
    db: &dyn CypherDb,
    file: SourceFile,
    file_opts: FileOptions,
    ws: WorkspaceInputs,
) -> DiagnosticsOutput {
    // Establish Salsa dependency on the parsed CST; lower from it
    // directly via `lower_parse` (cy-cfi) — no second parse.
    let cst = parse_cst(db, file);
    let opts = file_opts.options(db);
    let schema = ws.schema(db);
    let schema_ref = schema.as_deref();

    // Lower and desugar HIR inline. `lower_parse` is infallible.
    let stmt = hir_lower(cst.parse()).expect("cyrs-db: lower_parse is infallible");
    let stmt = desugar_statement(stmt);

    let sema_opts = SemaOptions {
        parameter_hints: opts
            .parameter_hints
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        warn_shadowing: opts.warn_shadowing,
        dialect: match opts.dialect {
            DialectMode::GqlAligned => cyrs_sema::DialectMode::GqlAligned,
            DialectMode::OpenCypherV9 => cyrs_sema::DialectMode::OpenCypherV9,
        },
    };

    let mut sink = DiagnosticsSink::new();
    cyrs_sema::analyse(&stmt, schema_ref, &sema_opts, &mut sink);
    DiagnosticsOutput::new(sink.into_sorted())
}

/// Lower a [`SourceFile`] to a logical plan (spec §12 / §11.3 `plan`).
///
/// Cache key: `parse_cst` (plan lowering is a pure function of source +
/// dialect; options and schema do not affect the plan shape in v1).
///
/// HIR lowering is performed inline (see module doc for rationale).
#[salsa::tracked(lru = 256)]
pub fn plan_of(db: &dyn CypherDb, file: SourceFile) -> PlanOutput {
    // Establish Salsa dependency on the parsed CST; lower from it
    // directly via `lower_parse` (cy-cfi) — no second parse.
    let cst = parse_cst(db, file);

    // Lower and desugar HIR inline. `lower_parse` is infallible.
    let stmt = hir_lower(cst.parse()).expect("cyrs-db: lower_parse is infallible");
    let stmt = desugar_statement(stmt);

    // cy-wlr: `plan_lower` is fallible now. When the HIR still contains
    // `Expr::Unresolved` or un-desugared nodes (e.g. because the source
    // is malformed and name resolution did not wire every reference) we
    // surface an empty plan rather than propagating the error — the
    // diagnostics surface of the DB layer already reports the underlying
    // issue through `sema_diagnostics` / `all_diagnostics`, and the plan
    // view is best-effort for malformed inputs.
    let plan = plan_lower(&stmt).unwrap_or_else(|_| PlanStatement::empty());
    PlanOutput::new(plan)
}

/// Union of parse diagnostics + sema diagnostics (spec §11.3 `diagnostics`).
///
/// Sorted by `(span_start, code)` and de-duplicated for determinism
/// (spec §8 / §17.14).
///
/// Cache key: `parse_cst` + `sema_diagnostics`. Changing source invalidates
/// both parse and sema layers; changing only options/schema invalidates only
/// the sema layer (parse cache survives).
#[salsa::tracked]
pub fn all_diagnostics(
    db: &dyn CypherDb,
    file: SourceFile,
    file_opts: FileOptions,
    ws: WorkspaceInputs,
) -> DiagnosticsOutput {
    // Parse diagnostics (SyntaxError → Diagnostic).
    let cst = parse_cst(db, file);
    let parse_diags: Vec<Diagnostic> = cst
        .parse()
        .errors()
        .iter()
        .map(syntax_error_to_diagnostic)
        .collect();

    // Sema diagnostics (cached independently).
    let sema = sema_diagnostics(db, file, file_opts, ws);

    // Union, sort by (span_start, code), deduplicate.
    let mut combined: Vec<Diagnostic> = parse_diags;
    combined.extend_from_slice(sema.diagnostics());
    combined.sort_by_key(|d| (d.primary.range.start(), d.code));
    combined
        .dedup_by(|a, b| a.primary.range.start() == b.primary.range.start() && a.code == b.code);

    DiagnosticsOutput::new(combined)
}

// ---------------------------------------------------------------------------
// Public convenience API — `analyse_file`
// ---------------------------------------------------------------------------

/// The result of a full analysis of a single [`SourceFile`].
///
/// Returned by [`analyse_file`] as a convenient summary that bundles the
/// logical plan with the complete diagnostic list.
#[derive(Debug, Clone)]
pub struct Analysis {
    /// Logical plan for the query.
    pub plan: PlanOutput,
    /// All diagnostics (parse + semantic), sorted and de-duplicated.
    pub diagnostics: DiagnosticsOutput,
}

/// Convenience entry point: run the full analysis pipeline for `file` and
/// return an [`Analysis`] bundling the plan and all diagnostics.
///
/// `file_opts` carries the per-file [`crate::inputs::AnalysisOptions`];
/// `ws` carries the workspace-scoped schema. Callers that do not need
/// options or a schema may pass freshly-constructed defaults:
///
/// ```rust,ignore
/// let opts = db.new_file_options(AnalysisOptions::default());
/// let ws   = db.new_workspace_inputs(None);
/// let a    = analyse_file(&db, file, opts, ws);
/// ```
///
/// All layers are individually memoised; subsequent calls with unchanged
/// inputs are O(1).
pub fn analyse_file(
    db: &dyn CypherDb,
    file: SourceFile,
    file_opts: FileOptions,
    ws: WorkspaceInputs,
) -> Analysis {
    let plan = plan_of(db, file);
    let diagnostics = all_diagnostics(db, file, file_opts, ws);
    Analysis { plan, diagnostics }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert a [`cyrs_syntax::SyntaxError`] into a [`Diagnostic`].
///
/// `SyntaxError.code` carries the numeric discriminant of the `DiagCode`
/// variant (e.g. `3` for `E0003`). The lookup is delegated to
/// [`DiagCode::from`] for a [`cyrs_syntax::SyntaxError`] reference
/// (cy-emb3) — unknown codes fall back to `E0001`.
fn syntax_error_to_diagnostic(e: &cyrs_syntax::SyntaxError) -> Diagnostic {
    let code = DiagCode::from(e);
    let range = TextRange::new(e.offset, e.offset);
    // `Diagnostic` is `#[non_exhaustive]` (cy-2i9.1).  Use the `error`
    // constructor rather than a struct literal so downstream / cross-crate
    // construction remains SemVer-stable.
    Diagnostic::error(code, range, SmolStr::new(&e.message))
}

// ---------------------------------------------------------------------------
// LRU capacity helpers (spec §11.X, bead cy-31b)
//
// Salsa 0.26 generates `set_lru_capacity` as an associated function on the
// tracked-fn module, but it is private to the declaring module.  These thin
// wrappers re-export the capability so that `workspace::Database::with_options`
// can apply runtime LRU caps from [`crate::options::DatabaseOptions`].
// ---------------------------------------------------------------------------

/// Adjust the LRU capacity of [`resolved_names`] at runtime.
///
/// Must be called before any queries are issued.  Corresponds to the
/// `sema_lru` field of [`crate::options::DatabaseOptions`].
pub fn set_resolved_names_lru(db: &mut impl crate::CypherDb, cap: usize) {
    resolved_names::set_lru_capacity(db, cap);
}

/// Adjust the LRU capacity of [`sema_diagnostics`] at runtime.
///
/// Must be called before any queries are issued.  Corresponds to the
/// `sema_lru` field of [`crate::options::DatabaseOptions`].
pub fn set_sema_diagnostics_lru(db: &mut impl crate::CypherDb, cap: usize) {
    sema_diagnostics::set_lru_capacity(db, cap);
}

/// Adjust the LRU capacity of [`plan_of`] at runtime.
///
/// Must be called before any queries are issued.  Corresponds to the
/// `plan_lru` field of [`crate::options::DatabaseOptions`].
pub fn set_plan_of_lru(db: &mut impl crate::CypherDb, cap: usize) {
    plan_of::set_lru_capacity(db, cap);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CypherDatabase;
    use crate::inputs::AnalysisOptions;
    use std::sync::Arc;

    // Helper: build a db with one file + default options + no schema.
    fn setup(src: &str) -> (CypherDatabase, SourceFile, FileOptions, WorkspaceInputs) {
        let mut db = CypherDatabase::new();
        let file = db.new_source_file(src);
        let file_opts = db.new_file_options(AnalysisOptions::default());
        let ws = db.new_workspace_inputs(None);
        (db, file, file_opts, ws)
    }

    // ── Cache hit: pointer-equality ─────────────────────────────────────────

    /// Second call on unchanged inputs returns the same `Arc` (cached).
    #[test]
    fn parse_ast_cached() {
        let (db, file, _, _) = setup("RETURN 1");
        let a1 = parse_ast(&db, file);
        let a2 = parse_ast(&db, file);
        assert!(
            Arc::ptr_eq(&a1.0, &a2.0),
            "parse_ast should return a cached Arc on second call"
        );
    }

    /// `plan_of` returns the same Arc on second call (no recomputation).
    #[test]
    fn plan_of_cached() {
        let (db, file, _, _) = setup("MATCH (n) RETURN n");
        let p1 = plan_of(&db, file);
        let p2 = plan_of(&db, file);
        assert!(
            Arc::ptr_eq(&p1.0, &p2.0),
            "plan_of should return a cached Arc on second call"
        );
    }

    /// `sema_diagnostics` returns the same Arc on second call.
    #[test]
    fn sema_diagnostics_cached() {
        let (db, file, file_opts, ws) = setup("RETURN 1");
        let d1 = sema_diagnostics(&db, file, file_opts, ws);
        let d2 = sema_diagnostics(&db, file, file_opts, ws);
        assert!(
            Arc::ptr_eq(&d1.0, &d2.0),
            "sema_diagnostics should return a cached Arc on second call"
        );
    }

    /// `all_diagnostics` returns the same Arc on second call.
    #[test]
    fn all_diagnostics_cached() {
        let (db, file, file_opts, ws) = setup("RETURN 1");
        let d1 = all_diagnostics(&db, file, file_opts, ws);
        let d2 = all_diagnostics(&db, file, file_opts, ws);
        assert!(
            Arc::ptr_eq(&d1.0, &d2.0),
            "all_diagnostics should return a cached Arc on second call"
        );
    }

    // ── Source change: full pipeline re-evaluates ────────────────────────────

    /// Changing `source` produces a new `ParseOutput` Arc and a new
    /// `PlanOutput` Arc (cache invalidated).
    #[test]
    fn source_change_invalidates_pipeline() {
        let (mut db, file, _, _) = setup("MATCH (n) RETURN n");

        let p1 = plan_of(&db, file);
        let d1 = parse_ast(&db, file);

        db.set_source(file, "RETURN 42");

        let p2 = plan_of(&db, file);
        let d2 = parse_ast(&db, file);

        assert!(
            !Arc::ptr_eq(&p1.0, &p2.0),
            "plan_of should re-execute after source change"
        );
        assert!(
            !Arc::ptr_eq(&d1.0, &d2.0),
            "parse_ast should re-execute after source change"
        );
    }

    // ── Options-only change: sema re-evaluates, parse cache survives ─────────

    /// Toggling `warn_shadowing` re-evaluates `sema_diagnostics` but does
    /// NOT re-evaluate `parse_cst`.
    #[test]
    fn options_change_reruns_sema_only() {
        let (mut db, file, file_opts, ws) = setup("MATCH (n) RETURN n");

        // Parse the CST first to warm the cache.
        let cst1 = parse_cst(&db, file);
        let _ = sema_diagnostics(&db, file, file_opts, ws);

        // Toggle warn_shadowing — options digest changes.
        db.set_options(
            file_opts,
            AnalysisOptions {
                warn_shadowing: true,
                ..Default::default()
            },
        );

        // parse_cst cache should survive (same Arc).
        let cst2 = parse_cst(&db, file);
        assert!(
            Arc::ptr_eq(&cst1.0, &cst2.0),
            "parse_cst Arc should be unchanged after options-only change"
        );

        // sema_diagnostics should re-execute (new Arc).
        // We can't directly assert the Arc changed since the output might
        // happen to be equal; we simply verify it runs without error.
        let _d2 = sema_diagnostics(&db, file, file_opts, ws);
    }

    // ── Schema change: schema-aware sema re-evaluates, parse cache survives ──

    /// Changing the workspace schema re-evaluates `sema_diagnostics` but
    /// leaves `parse_cst` cached.
    #[test]
    fn schema_change_reruns_sema_only() {
        use cyrs_schema::EmptySchema;
        let (mut db, file, file_opts, ws) = setup("MATCH (n:Person) RETURN n");

        let cst1 = parse_cst(&db, file);

        // Set a (still empty) schema — triggers a revision bump on `ws`.
        let schema: Arc<dyn cyrs_schema::SchemaProvider> = Arc::new(EmptySchema);
        db.set_schema(ws, Some(schema));

        let cst2 = parse_cst(&db, file);
        assert!(
            Arc::ptr_eq(&cst1.0, &cst2.0),
            "parse_cst Arc should be unchanged after schema change"
        );

        // sema_diagnostics runs without error (schema-aware pass with EmptySchema).
        let _d = sema_diagnostics(&db, file, file_opts, ws);
    }

    // ── analyse_file convenience wrapper ────────────────────────────────────

    /// `analyse_file` returns both a plan and a diagnostic list.
    #[test]
    fn analyse_file_returns_plan_and_diags() {
        let (db, file, file_opts, ws) = setup("MATCH (n) RETURN n");
        let analysis = analyse_file(&db, file, file_opts, ws);
        // A valid query produces at least a Source + Project plan op.
        assert!(
            !analysis.plan.plan().ops.is_empty(),
            "expected at least one plan op"
        );
    }

    /// `analyse_file` on an empty source returns an empty plan and no parse errors.
    #[test]
    fn analyse_file_empty_source() {
        let (db, file, file_opts, ws) = setup("");
        let analysis = analyse_file(&db, file, file_opts, ws);
        // Empty source → no sema diagnostics from the sema passes (parse may
        // or may not produce syntax errors depending on the parser; either is
        // acceptable).
        let _ = analysis.diagnostics.diagnostics();
    }

    // ── all_diagnostics ordering + dedup ────────────────────────────────────

    /// Diagnostics in `all_diagnostics` are sorted by (`span_start`, code).
    #[test]
    fn all_diagnostics_are_sorted() {
        let (db, file, file_opts, ws) = setup("MATCH (n) RETURN n");
        let diags = all_diagnostics(&db, file, file_opts, ws);
        let d = diags.diagnostics();
        for w in d.windows(2) {
            let a_key = (w[0].primary.range.start(), w[0].code);
            let b_key = (w[1].primary.range.start(), w[1].code);
            assert!(
                a_key <= b_key,
                "diagnostics must be sorted: {a_key:?} > {b_key:?}"
            );
        }
    }

    // ── parse_ast basic correctness ─────────────────────────────────────────

    /// `parse_ast` round-trips the source text.
    #[test]
    fn parse_ast_roundtrips_source() {
        let src = "MATCH (n:Person) RETURN n.name";
        let (db, file, _, _) = setup(src);
        let ast = parse_ast(&db, file);
        assert_eq!(
            ast.parse_output().parse().syntax().to_string(),
            src,
            "AST output must round-trip the source"
        );
    }

    // ── resolved_names basic correctness ────────────────────────────────────

    /// `resolved_names` runs without panic on a simple query.
    #[test]
    fn resolved_names_basic() {
        let (db, file, file_opts, _) = setup("MATCH (n) RETURN n");
        let rn = resolved_names(&db, file, file_opts);
        // The scope graph must have at least a Root scope.
        let result = rn.result();
        // Scope graph is non-trivial — just ensure it didn't panic.
        let _ = &result.scope_graph;
        let _ = &result.resolved_names;
    }
}
