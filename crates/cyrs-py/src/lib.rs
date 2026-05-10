//! `cyrs-py` — Python (`PyO3`) binding for the Cyrs agent op surface.
//! Spec 0004 §6.
//!
//! This crate is a **thin adapter** (spec 0004 §2.1) over
//! [`cyrs_lang_services`], [`cyrs_db`], [`cyrs_diag`], and
//! [`cyrs_fmt`].  No parsing, sema, formatting, or planning logic
//! lives here — every method on the Python-visible
//! [`CypherDatabase`] class translates a Python request shape into a
//! call on the upstream engines and translates the result back across
//! the `PyO3` boundary.
//!
//! # Binding choice
//!
//! `PyO3` binds directly to [`cyrs_lang_services`] — it does **not**
//! route through `cyrs-ffi` (spec 0004 §6.1).  `PyO3` handles error
//! conversion, GIL management, and type coercion natively, so a second
//! serialise / deserialise through the C ABI would only cost fidelity.
//! The two adapter crates ship independently.
//!
//! # `abi3` tradeoff
//!
//! `pyo3` is pulled with the `abi3-py310` feature (spec 0004 §6.2), so
//! the produced `cdylib` is built against `CPython`'s stable ABI with
//! 3.10 as the floor.  One wheel per `{os, arch}` covers Python
//! 3.10–3.13 at the cost of losing access to post-3.10 type additions
//! (e.g. the `Py_TPFLAGS_IMMUTABLETYPE` slot, `PyLong_AsInt` fast path).
//! The adapter surface only uses stable-ABI-representable types
//! (strings, ints, dicts, lists, opaque `#[pyclass]` handles) so the
//! tradeoff is free.
//!
//! # Op surface
//!
//! The agent v1 op surface (spec 0001 §15 / spec 0004 §6) is mirrored
//! one-to-one as methods on a single Python [`CypherDatabase`] class:
//!
//! | agent op       | Python method     |
//! | -------------- | ----------------- |
//! | `parse`        | `parse`           |
//! | `check`        | `check`           |
//! | `complete`     | `complete`        |
//! | `hover`        | `hover`           |
//! | `format`       | `format`          |
//! | `rewrite`      | `rewrite`         |
//! | `schema_set`   | `schema_set`      |
//! | `schema_clear` | `schema_clear`    |
//!
//! In addition, the module exports a `PROTO_VERSION` constant — the
//! wire-format version a Python caller should match before dispatching
//! ops (spec 0004 §9.3).
//!
//! # Diagnostic shape
//!
//! Diagnostics are exposed as an opaque [`Diagnostic`] `#[pyclass]` with
//! `.code` (str), `.severity` (str, lower-case), `.message` (str), and
//! `.range` (tuple of four ints: `(start_line, start_col, end_line,
//! end_col)`).  The class is immutable from Python's perspective — the
//! type stub surfaces read-only properties only.
//!
//! # `FileId` lifetime model
//!
//! Mirrors the FFI crate's pattern: each [`CypherDatabase`] owns one
//! [`cyrs_db::Database`] plus a single interned `FileId` reused
//! across calls (stable `FileId`s, zero churn).  Python callers that
//! want multi-file incremental analysis should use the LSP surface or
//! the agent JSON API.

// NOTE: no crate-level `#![forbid(unsafe_code)]` — the Cargo.toml
// `[lints.rust]` table sets `unsafe_code = "allow"` crate-locally so
// PyO3's macro-generated `unsafe` blocks compile.  Every `unsafe`
// reference in the expanded crate originates in PyO3 itself; this
// crate has no hand-written `unsafe` blocks.
//
// PyO3's macros emit `#[pymethods]` impl blocks that legitimately use
// `&self` / `&mut self` in method signatures with `&str` / `String`
// arguments mixed; clippy's `needless_pass_by_value` sometimes fires on
// owned-string arguments that PyO3 prefers.  The house style for adapter
// crates is to accept what the macro wants rather than fight the
// codegen.
#![allow(clippy::needless_pass_by_value)]
// PyO3 generates `__pymethod_*` helpers that are legitimately never
// called directly; `dead_code` would flag them if we let it through.
#![allow(clippy::used_underscore_binding)]
// PyO3's `#[pymethods]` macro expands every `-> PyResult<T>` into a
// shim that calls `.into::<PyErr>()` on the returned error — a no-op
// on a `PyErr` already, which clippy flags as `useless_conversion`.
// The call site is macro-generated so we cannot annotate each method;
// silencing crate-wide is the standard PyO3 adapter pattern.
#![allow(clippy::useless_conversion)]
// Same story for `missing_fields_in_debug` — PyO3's `#[pyclass]` adds
// an internal lock field the hand-written `Debug` impl cannot see.
#![allow(clippy::missing_fields_in_debug)]
// And for `unused_self` — some `#[pymethods]` are pure functions that
// PyO3 still wants threaded through a `&self` receiver.
#![allow(clippy::unused_self)]

use std::path::Path;
use std::sync::Arc;

use cyrs_db::{Database, DialectMode, FileId};
use cyrs_diag::Severity;
use cyrs_fmt::{FormatOptions, format_with};
use cyrs_lang_services::{
    CompletionItem, CompletionItemKind as NeutralKind, Hover, RewritePayload,
    complete as complete_shared, hover as hover_shared, rewrite as rewrite_shared,
};
use cyrs_schema::file::load_from_toml_str;
use cyrs_syntax::{LineIndex, TextSize};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

// ---------------------------------------------------------------------------
// Proto version — mirrors the agent wire proto pin (cy-2i9 / spec §15).
// ---------------------------------------------------------------------------

/// Wire protocol version for all ops exported by this crate.  Bumped via
/// a spec-governed action (spec 0004 §9.3).
pub const PROTO_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Diagnostic — Python-visible dataclass-ish struct.
// ---------------------------------------------------------------------------

/// Python-facing diagnostic.  Constructed only by this crate; the
/// `#[pyclass]` exposes read-only accessors so Python callers can treat
/// it as an immutable value object (matches the `dataclass(frozen=True)`
/// shape the type stub describes).
///
/// `skip_from_py_object` — pyo3 0.28 emits an automatic `FromPyObject`
/// impl for `Clone` pyclasses but is transitioning this to opt-in.
/// `Diagnostic` is a return-only type (never crosses the boundary
/// Python → Rust), so opting out now mirrors the eventual default and
/// silences the deprecation warning.
#[pyclass(module = "cypher", frozen, skip_from_py_object)]
#[derive(Debug, Clone)]
pub struct Diagnostic {
    code: String,
    severity: String,
    message: String,
    // Stored as four separate u32s so the `range` getter can hand back
    // a plain Python tuple without re-allocating on every access.
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

#[pymethods]
impl Diagnostic {
    /// Stable diagnostic code, e.g. `"E0001"` (spec 0001 §10).
    #[getter]
    fn code(&self) -> &str {
        &self.code
    }

    /// Lower-case severity string: `"error" | "warning" | "help" | "note"`.
    #[getter]
    fn severity(&self) -> &str {
        &self.severity
    }

    /// Human-readable diagnostic message.
    #[getter]
    fn message(&self) -> &str {
        &self.message
    }

    /// Primary range as `(start_line, start_col, end_line, end_col)` —
    /// 0-based UTF-8 columns (spec 0001 §4.5).
    #[getter]
    fn range(&self) -> (u32, u32, u32, u32) {
        (self.start_line, self.start_col, self.end_line, self.end_col)
    }

    fn __repr__(&self) -> String {
        format!(
            "Diagnostic(code={:?}, severity={:?}, range=({}, {}, {}, {}), message={:?})",
            self.code,
            self.severity,
            self.start_line,
            self.start_col,
            self.end_line,
            self.end_col,
            self.message,
        )
    }
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Help => "help",
        // `Severity` is `#[non_exhaustive]` — the catch-all covers both
        // the existing `Note` variant and any future additions, keeping
        // the Python API stable.
        _ => "note",
    }
}

fn diag_from(idx: &LineIndex, d: &cyrs_diag::Diagnostic) -> Diagnostic {
    let s = idx.line_col(d.primary.range.start());
    let e = idx.line_col(d.primary.range.end());
    Diagnostic {
        code: d.code.to_string(),
        severity: severity_str(d.severity).to_owned(),
        message: d.message.as_str().to_owned(),
        start_line: s.line,
        start_col: s.col,
        end_line: e.line,
        end_col: e.col,
    }
}

// ---------------------------------------------------------------------------
// CypherDatabase — the sole Python-facing handle.
// ---------------------------------------------------------------------------

/// Python-facing handle wrapping one [`cyrs_db::Database`] plus a
/// single interned `FileId` reused across calls.  Instances are
/// constructed from Python via `CypherDatabase()`.
///
/// `unsendable` — pyo3 ≥ 0.23 requires every `#[pyclass]` to be
/// `Send + Sync` by default so free-threaded Python can share the
/// handle across interpreter threads.  `cyrs_db::Database` wraps
/// `salsa::Storage`, which holds an `UnsafeCell<HashMap<…>>` in its
/// thread-local and is therefore not `Sync`.  Marking the class
/// `unsendable` keeps the GIL-era semantics — pyo3 panics at runtime
/// if Python code tries to access the handle from a different thread
/// than the one that created it (spec 0004 §6 keeps the binding
/// single-threaded; multi-thread use is out of scope).
#[pyclass(module = "cypher", unsendable)]
pub struct CypherDatabase {
    db: Database,
    file: Option<FileId>,
    dialect: DialectMode,
}

impl std::fmt::Debug for CypherDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CypherDatabase")
            .field("has_file", &self.file.is_some())
            .field("dialect", &self.dialect)
            .finish()
    }
}

impl Default for CypherDatabase {
    fn default() -> Self {
        Self::new_inner()
    }
}

impl CypherDatabase {
    fn new_inner() -> Self {
        Self {
            db: Database::new(),
            file: None,
            dialect: DialectMode::GqlAligned,
        }
    }

    /// Intern or update the single `FileId`.  Returns the `FileId` the
    /// caller should pass to downstream queries.
    fn ingest(&mut self, source: String) -> FileId {
        if let Some(id) = self.file {
            self.db
                .update_file(id, source)
                .expect("interned FileId must remain open");
            id
        } else {
            let id = self.db.open_file(Path::new("_"), source, self.dialect);
            self.file = Some(id);
            id
        }
    }
}

#[pymethods]
impl CypherDatabase {
    /// Construct a fresh database with no interned file and no schema.
    #[new]
    fn py_new() -> Self {
        Self::new_inner()
    }

    // ----------------------------------------------------------------------
    // parse → dict with `cst_string`, `syntax_errors`, `proto_version`.
    // ----------------------------------------------------------------------

    /// `parse` op — return the CST pretty-print and raw parse errors.
    ///
    /// Returns a dict `{"op", "cst_string", "syntax_errors",
    /// "proto_version"}`.  On database failure a `RuntimeError` is
    /// raised instead; malformed source is *not* an error — it is
    /// surfaced via the `syntax_errors` list.
    fn parse<'py>(&mut self, py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyDict>> {
        let id = self.ingest(text.to_owned());
        let out = self
            .db
            .parse_cst(id)
            .map_err(|e| PyRuntimeError::new_err(format!("parse_cst failed: {e}")))?;
        let parse = out.parse();
        let errors: Vec<String> = parse.errors().iter().map(|e| e.message.clone()).collect();
        let dict = PyDict::new(py);
        dict.set_item("op", "parse")?;
        dict.set_item("cst_string", parse.syntax().to_string())?;
        dict.set_item("syntax_errors", errors)?;
        dict.set_item("proto_version", PROTO_VERSION)?;
        Ok(dict)
    }

    // ----------------------------------------------------------------------
    // check → list of Diagnostic.
    // ----------------------------------------------------------------------

    /// `check` op — full pipeline diagnostics (parse + sema).  Returns
    /// a `list[Diagnostic]`; an empty list means "source is clean".
    fn check(&mut self, text: &str) -> PyResult<Vec<Diagnostic>> {
        let line_idx = LineIndex::new(text);
        let id = self.ingest(text.to_owned());
        let out = self
            .db
            .all_diagnostics(id)
            .map_err(|e| PyRuntimeError::new_err(format!("all_diagnostics failed: {e}")))?;
        Ok(out
            .diagnostics()
            .iter()
            .map(|d| diag_from(&line_idx, d))
            .collect())
    }

    // ----------------------------------------------------------------------
    // complete / hover — delegated to cyrs-lang-services.
    // ----------------------------------------------------------------------

    /// `complete` op — completion items at byte offset `offset`.
    ///
    /// Returns a `list[dict]` with `{"label", "kind"}` per item; the
    /// `kind` string matches the wire tags (`"keyword" | "label" |
    /// "relationship_type" | "parameter" | "property" | "other"`).
    fn complete<'py>(
        &mut self,
        py: Python<'py>,
        text: &str,
        offset: u32,
    ) -> PyResult<Bound<'py, PyList>> {
        let id = self.ingest(text.to_owned());
        let items = complete_shared(&self.db, id, TextSize::from(offset));
        let list = PyList::empty(py);
        for item in &items {
            list.append(completion_to_dict(py, item)?)?;
        }
        Ok(list)
    }

    /// `hover` op — markdown + byte range at byte offset `offset`.
    ///
    /// Returns a dict `{"markdown", "range", "proto_version"}`.  An
    /// empty `"markdown"` string means "no content at cursor".
    fn hover<'py>(
        &mut self,
        py: Python<'py>,
        text: &str,
        offset: u32,
    ) -> PyResult<Bound<'py, PyDict>> {
        let id = self.ingest(text.to_owned());
        let Hover {
            markdown, range, ..
        } = hover_shared(&self.db, id, TextSize::from(offset));
        let dict = PyDict::new(py);
        dict.set_item("op", "hover")?;
        dict.set_item("markdown", markdown)?;
        dict.set_item("range", (u32::from(range.start()), u32::from(range.end())))?;
        dict.set_item("proto_version", PROTO_VERSION)?;
        Ok(dict)
    }

    // ----------------------------------------------------------------------
    // format — infallible on malformed input (matches the agent
    // wire contract: formatter errors return the input unchanged).
    // ----------------------------------------------------------------------

    /// `format` op — return the formatted source.
    ///
    /// On formatter failure the returned string is the input unchanged
    /// (mirrors the agent's "format is infallible on malformed input"
    /// contract, spec §15).  Python callers that want the error message
    /// should use [`check`].
    fn format(&mut self, text: &str) -> String {
        format_with(text, &FormatOptions::default()).unwrap_or_else(|_| text.to_owned())
    }

    // ----------------------------------------------------------------------
    // rewrite — apply Diagnostic::fixes edits by id.
    // ----------------------------------------------------------------------

    /// `rewrite` op — apply the listed `fix_ids` to `text`.
    ///
    /// Returns a dict `{"resulting_text", "applied_edits",
    /// "unknown_fix_ids", "proto_version"}`.
    fn rewrite<'py>(
        &mut self,
        py: Python<'py>,
        text: &str,
        fix_ids: Vec<String>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let id = self.ingest(text.to_owned());
        let payload: RewritePayload = rewrite_shared(&self.db, id, text, &fix_ids);
        let applied = PyList::empty(py);
        for e in &payload.applied_edits {
            let edit_dict = PyDict::new(py);
            edit_dict.set_item("fix_id", &e.fix_id)?;
            edit_dict.set_item(
                "range",
                (u32::from(e.range.start()), u32::from(e.range.end())),
            )?;
            edit_dict.set_item("replacement", &e.replacement)?;
            applied.append(edit_dict)?;
        }
        let dict = PyDict::new(py);
        dict.set_item("op", "rewrite")?;
        dict.set_item("applied_edits", applied)?;
        dict.set_item("resulting_text", payload.resulting_text)?;
        dict.set_item("unknown_fix_ids", payload.unknown_fix_ids)?;
        dict.set_item("proto_version", PROTO_VERSION)?;
        Ok(dict)
    }

    // ----------------------------------------------------------------------
    // schema_set / schema_clear.
    // ----------------------------------------------------------------------

    /// `schema_set` op — install a schema from a TOML string.
    ///
    /// Delegates to [`cyrs_schema::file::load_from_toml_str`] so the
    /// schema file shape matches the CLI / agent.  Raises `ValueError`
    /// with the parser's error message on malformed input.
    fn schema_set(&mut self, toml: &str) -> PyResult<()> {
        let schema = load_from_toml_str(toml)
            .map_err(|e| PyValueError::new_err(format!("invalid schema TOML: {e}")))?;
        let schema: Arc<dyn cyrs_schema::SchemaProvider> = Arc::new(schema);
        self.db.set_schema(Some(schema));
        Ok(())
    }

    /// `schema_clear` op — drop the in-session schema.
    fn schema_clear(&mut self) {
        self.db.set_schema(None);
    }
}

// ---------------------------------------------------------------------------
// Helpers — completion item coercion.
// ---------------------------------------------------------------------------

fn completion_kind_str(k: NeutralKind) -> &'static str {
    match k {
        NeutralKind::Keyword => "keyword",
        NeutralKind::Label => "label",
        NeutralKind::RelationshipType => "relationship_type",
        NeutralKind::Parameter => "parameter",
        NeutralKind::Property => "property",
        // Non-exhaustive upstream enum — catch-all keeps the Python
        // wire contract stable.
        _ => "other",
    }
}

fn completion_to_dict<'py>(py: Python<'py>, item: &CompletionItem) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("label", item.label.as_str())?;
    dict.set_item("kind", completion_kind_str(item.kind))?;
    if let Some(detail) = item.detail.as_deref() {
        dict.set_item("detail", detail)?;
    }
    Ok(dict)
}

// ---------------------------------------------------------------------------
// #[pymodule] entry point.
// ---------------------------------------------------------------------------

/// Python module entry point.  `maturin develop` / wheel install makes
/// `from cypher._cypher import CypherDatabase, Diagnostic` resolve
/// here; `python/cypher/__init__.py` then re-exports to the top-level
/// `cypher` namespace so callers write `import cypher`.  The
/// `_cypher` split lets us ship a mixed-layout wheel (Rust extension +
/// pure-Python stubs) without the `.pyi` type stub colliding with
/// the extension module's import name.
#[pymodule]
fn _cypher(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<CypherDatabase>()?;
    m.add_class::<Diagnostic>()?;
    m.add("PROTO_VERSION", PROTO_VERSION)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Native-target unit tests.
// PyO3's `extension-module` feature disables the Python interpreter
// link at build time, so these tests exercise the plain Rust helper
// paths directly without touching the `#[pymethods]` wrappers.
// End-to-end Python-side tests live in `tests/test_smoke.py` and run
// via `pytest` after `maturin develop`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_version_is_one() {
        assert_eq!(PROTO_VERSION, 1);
    }

    #[test]
    fn severity_str_matches_known_variants() {
        assert_eq!(severity_str(Severity::Error), "error");
        assert_eq!(severity_str(Severity::Warning), "warning");
        assert_eq!(severity_str(Severity::Help), "help");
        // The `_` arm covers `Note` plus any future non_exhaustive
        // additions.
        assert_eq!(severity_str(Severity::Note), "note");
    }

    #[test]
    fn completion_kind_str_covers_known_variants() {
        assert_eq!(completion_kind_str(NeutralKind::Keyword), "keyword");
        assert_eq!(completion_kind_str(NeutralKind::Label), "label");
        assert_eq!(
            completion_kind_str(NeutralKind::RelationshipType),
            "relationship_type"
        );
        assert_eq!(completion_kind_str(NeutralKind::Parameter), "parameter");
        assert_eq!(completion_kind_str(NeutralKind::Property), "property");
    }

    #[test]
    fn ingest_same_source_reuses_fileid() {
        let mut h = CypherDatabase::new_inner();
        let a = h.ingest("RETURN 1".to_owned());
        let b = h.ingest("RETURN 2".to_owned());
        assert_eq!(a, b, "the single interned FileId must survive updates");
    }

    #[test]
    fn diag_from_has_plausible_range() {
        // Build one diagnostic via the full pipeline so LineIndex maps
        // through a real range.  Unclosed paren → E-coded diagnostic.
        let mut h = CypherDatabase::new_inner();
        let src = "MATCH (n RETURN n";
        let line_idx = LineIndex::new(src);
        let id = h.ingest(src.to_owned());
        let out = h.db.all_diagnostics(id).expect("query ok");
        let d = out.diagnostics().first().expect("at least one diag");
        let diag = diag_from(&line_idx, d);
        assert!(diag.code.starts_with('E'), "got {}", diag.code);
        assert_eq!(diag.severity, "error");
        // 0-based coords; the unclosed paren sits on line 0.
        assert_eq!(diag.start_line, 0);
    }
}
