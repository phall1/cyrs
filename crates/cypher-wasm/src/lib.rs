//! `cypher-wasm` — WebAssembly binding for the Cyrs agent op surface.
//! Spec 0004 §4.
//!
//! This crate is a **thin adapter** (spec 0004 §2.1) over
//! [`cypher_lang_services`], [`cypher_db`], [`cypher_diag`], and
//! [`cypher_fmt`]. No parsing, sema, formatting, or planning logic lives
//! here — every public method translates a JS request shape into a call
//! on the upstream engines and translates the result back via
//! [`serde_wasm_bindgen`].
//!
//! # Op surface
//!
//! The agent v1 wire protocol (spec 0001 §15 / spec 0004 §4.1) is
//! mirrored one-to-one as methods on a single [`CypherDatabase`] JS
//! class:
//!
//! | agent op       | JS method        |
//! | -------------- | ---------------- |
//! | `parse`        | `parse`          |
//! | `check`        | `check`          |
//! | `complete`     | `complete`       |
//! | `hover`        | `hover`          |
//! | `format`       | `format`         |
//! | `rewrite`      | `rewrite`        |
//! | `plan`         | `plan`           |
//! | `explain`      | `explain`        |
//! | `schema_set`   | `schemaSet`      |
//! | `schema_clear` | `schemaClear`    |
//!
//! In addition, a static `protoVersion()` returns the wire-format version
//! a JS caller must match before dispatching ops (spec 0004 §4.3).
//!
//! # `FileId` lifetime model
//!
//! `wasm-bindgen` cannot hold long-lived Rust references across async JS
//! calls the way an LSP session can, so each [`CypherDatabase`] owns a
//! single [`cypher_db::Database`] plus a small registry of `FileId`s
//! (one per dialect, bounded by N ≤ 2 — mirrors the agent crate's
//! `DialectFiles` shape from spec 0001 §11.6).  On every call the source
//! text is written into the dialect's interned `FileId` via
//! [`cypher_db::Database::update_file`] — stable `FileId`s, zero churn.
//!
//! # Plan / explain JSON shape
//!
//! `plan` and `explain` today surface the [`cypher_db::PlanOutput`]'s
//! `Debug` view as a string rather than a fully-serialised JSON tree:
//! the wire-format serialisation path lives in `cypher-plan/serde` which
//! is not on this crate's allow-listed dep set (spec 0004 §3).  When
//! `cypher-lang-services` grows a neutral `plan_json` / `plan_pretty`
//! helper (cy-od5 follow-up), this crate will forward to it unchanged.

#![forbid(unsafe_code)]
// wasm-bindgen generates a shim per exported fn; clippy's
// `needless_pass_by_value` sometimes fires on `&str` vs `String` chosen
// to make the JS side ergonomic — the house style for adapter crates is
// to accept owned where wasm-bindgen wants it and avoid wrestling the
// codegen.
#![allow(clippy::needless_pass_by_value)]

use std::path::Path;
use std::sync::Arc;

use cypher_db::{Database, DialectMode, FileId};
use cypher_diag::json as diag_json;
use cypher_fmt::{FormatOptions, format_with};
use cypher_lang_services::{
    CompletionItemKind as NeutralKind, complete as complete_shared, hover as hover_shared,
    rewrite as rewrite_shared,
};
use cypher_syntax::TextSize;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Proto version — mirrors the agent wire proto pin (cy-2i9 / spec §15).
// A JS caller is expected to read `CypherDatabase.protoVersion()` and
// refuse to dispatch ops if it does not match its own expected value
// (spec 0004 §4.3).
// ---------------------------------------------------------------------------

/// Wire protocol version for all ops exported by this crate.  Bumped via
/// a spec-governed action (spec 0004 §4.3).
pub const PROTO_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Dialect mirror (matches cypher-agent::Dialect)
// ---------------------------------------------------------------------------

/// Mirror of the agent crate's `Dialect` enum.  JS callers pass the
/// `snake_case` string `"gql_aligned"` / `"open_cypher_v9"` via the
/// dialect argument on each op; absent / unknown values fall back to
/// `GqlAligned` per the agent default.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    /// Spec 0001 §9: GQL-aligned default dialect.
    #[default]
    GqlAligned,
    /// Spec 0001 §9: openCypher v9 dialect.
    OpenCypherV9,
}

impl From<Dialect> for DialectMode {
    fn from(d: Dialect) -> Self {
        match d {
            Dialect::GqlAligned => Self::GqlAligned,
            Dialect::OpenCypherV9 => Self::OpenCypherV9,
        }
    }
}

/// Map an optional JS-side dialect string to the typed enum.  Unknown
/// / missing values silently fall back to `GqlAligned` so JS callers do
/// not need to keep the enum table in lockstep with the Rust side.
fn parse_dialect(tag: Option<&str>) -> Dialect {
    match tag {
        Some("open_cypher_v9") => Dialect::OpenCypherV9,
        _ => Dialect::GqlAligned,
    }
}

// ---------------------------------------------------------------------------
// DialectFiles — one FileId per dialect (spec 0001 §11.6).
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct DialectFiles {
    gql: Option<FileId>,
    openc: Option<FileId>,
}

impl DialectFiles {
    fn get(&self, dialect: Dialect) -> Option<FileId> {
        match dialect {
            Dialect::GqlAligned => self.gql,
            Dialect::OpenCypherV9 => self.openc,
        }
    }

    fn set(&mut self, dialect: Dialect, id: FileId) {
        match dialect {
            Dialect::GqlAligned => self.gql = Some(id),
            Dialect::OpenCypherV9 => self.openc = Some(id),
        }
    }
}

/// Intern `source` into the single `FileId` reserved for `dialect`.
/// Mirrors `cypher-agent::intern_file` behaviour — see spec 0001 §11.6.
fn intern_file(
    db: &mut Database,
    files: &mut DialectFiles,
    source: String,
    dialect: Dialect,
) -> FileId {
    if let Some(id) = files.get(dialect) {
        db.update_file(id, source)
            .expect("interned FileId must remain open");
        return id;
    }
    let id = db.open_file(Path::new("_"), source, dialect.into());
    files.set(dialect, id);
    id
}

// ---------------------------------------------------------------------------
// Schema — deserialize-only JSON mirror used by `schemaSet`.
// Kept intentionally minimal: a real caller assembles the JSON per the
// agent spec (§15) and ships it unchanged.  This crate does not own the
// schema wire shape.
// ---------------------------------------------------------------------------

/// Minimal schema provider the wasm layer accepts via `schemaSet`.
/// Mirrors the agent's `AgentSchema` deserialise contract so callers can
/// share JSON between the stdio agent and the wasm build.
#[derive(Debug, Deserialize)]
struct WasmSchema {
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    rel_types: Vec<String>,
}

impl cypher_schema::SchemaProvider for WasmSchema {
    fn labels(&self) -> Vec<smol_str::SmolStr> {
        self.labels.iter().map(smol_str::SmolStr::new).collect()
    }

    fn relationship_types(&self) -> Vec<smol_str::SmolStr> {
        self.rel_types.iter().map(smol_str::SmolStr::new).collect()
    }

    fn node_properties(&self, label: &str) -> Option<Vec<cypher_schema::PropertyDecl>> {
        if self.labels.iter().any(|l| l == label) {
            Some(Vec::new())
        } else {
            None
        }
    }

    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<cypher_schema::PropertyDecl>> {
        if self.rel_types.iter().any(|r| r == rel_type) {
            Some(Vec::new())
        } else {
            None
        }
    }

    fn relationship_endpoints(&self, _rel_type: &str) -> Vec<cypher_schema::EndpointDecl> {
        Vec::new()
    }

    fn inverse_of(&self, _rel_type: &str) -> Option<smol_str::SmolStr> {
        None
    }

    fn function(&self, _name: &str) -> Option<cypher_schema::FunctionSignature> {
        None
    }

    fn procedure(&self, _name: &str) -> Option<cypher_schema::ProcedureSignature> {
        None
    }

    fn schema_digest(&self) -> [u8; 32] {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for l in &self.labels {
            l.hash(&mut h);
        }
        for r in &self.rel_types {
            r.hash(&mut h);
        }
        let n = h.finish();
        let mut out = [0u8; 32];
        out[..8].copy_from_slice(&n.to_le_bytes());
        out
    }
}

// ---------------------------------------------------------------------------
// CypherDatabase — the sole JS-facing handle.
// ---------------------------------------------------------------------------

/// JS-facing handle wrapping one [`cypher_db::Database`] plus the
/// per-dialect `FileId` registry.  Instances are constructed from JS via
/// `new CypherDatabase()`.
#[wasm_bindgen]
#[derive(Debug)]
pub struct CypherDatabase {
    db: Database,
    files: DialectFiles,
}

impl Default for CypherDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl CypherDatabase {
    /// Construct a fresh database with no interned files and no schema.
    #[must_use]
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            db: Database::new(),
            files: DialectFiles::default(),
        }
    }

    /// Wire protocol version (spec 0004 §4.3).  JS callers should read
    /// this before dispatching any op; a mismatch is a hard-stop per the
    /// stability policy.
    #[must_use]
    #[wasm_bindgen(js_name = protoVersion)]
    pub fn proto_version_static() -> u32 {
        PROTO_VERSION
    }

    // ----------------------------------------------------------------------
    // parse → { cst_string, syntax_errors, proto_version }
    // ----------------------------------------------------------------------

    /// `parse` op — return the CST pretty-print and parse diagnostics.
    pub fn parse(&mut self, source: String, dialect: Option<String>) -> JsValue {
        let dialect = parse_dialect(dialect.as_deref());
        let id = intern_file(&mut self.db, &mut self.files, source, dialect);
        let value = match self.db.parse_cst(id) {
            Ok(out) => {
                let parse = out.parse();
                let errors: Vec<String> =
                    parse.errors().iter().map(|e| e.message.clone()).collect();
                json!({
                    "op": "parse",
                    "cst_string": parse.syntax().to_string(),
                    "syntax_errors": errors,
                    "proto_version": PROTO_VERSION,
                })
            }
            Err(e) => error_value(&e.to_string()),
        };
        to_js(&value)
    }

    // ----------------------------------------------------------------------
    // check → { diagnostics, proto_version }
    // ----------------------------------------------------------------------

    /// `check` op — full pipeline diagnostics (parse + sema).
    pub fn check(&mut self, source: String, dialect: Option<String>) -> JsValue {
        let dialect = parse_dialect(dialect.as_deref());
        let id = intern_file(&mut self.db, &mut self.files, source, dialect);
        let value = match self.db.all_diagnostics(id) {
            Ok(out) => {
                let diagnostics: Vec<Value> =
                    out.diagnostics().iter().map(diag_json::to_json).collect();
                json!({
                    "op": "check",
                    "diagnostics": diagnostics,
                    "proto_version": PROTO_VERSION,
                })
            }
            Err(e) => error_value(&e.to_string()),
        };
        to_js(&value)
    }

    // ----------------------------------------------------------------------
    // complete / hover / rewrite — delegated to cypher-lang-services.
    // ----------------------------------------------------------------------

    /// `complete` op — completion items keyed on `(source, offset)`.
    pub fn complete(&mut self, source: String, offset: u32, dialect: Option<String>) -> JsValue {
        let dialect = parse_dialect(dialect.as_deref());
        let id = intern_file(&mut self.db, &mut self.files, source, dialect);
        let items: Vec<Value> = complete_shared(&self.db, id, TextSize::from(offset))
            .into_iter()
            .map(|item| {
                let kind = match item.kind {
                    NeutralKind::Keyword => "keyword",
                    NeutralKind::Label => "label",
                    NeutralKind::RelationshipType => "relationship_type",
                    NeutralKind::Parameter => "parameter",
                    NeutralKind::Property => "property",
                    _ => "other",
                };
                if matches!(item.kind, NeutralKind::Parameter)
                    && item.detail.as_deref() == Some("placeholder")
                {
                    json!({ "label": item.label.to_string(), "kind": kind, "detail": "placeholder" })
                } else {
                    json!({ "label": item.label.to_string(), "kind": kind })
                }
            })
            .collect();
        to_js(&json!({
            "op": "complete",
            "items": items,
            "deferred": false,
            "deferred_reason": "",
            "proto_version": PROTO_VERSION,
        }))
    }

    /// `hover` op — markdown + byte range at the given cursor.
    pub fn hover(&mut self, source: String, offset: u32, dialect: Option<String>) -> JsValue {
        let dialect = parse_dialect(dialect.as_deref());
        let id = intern_file(&mut self.db, &mut self.files, source, dialect);
        let payload = hover_shared(&self.db, id, TextSize::from(offset));
        to_js(&json!({
            "op": "hover",
            "markdown": payload.markdown,
            "range": [
                u32::from(payload.range.start()),
                u32::from(payload.range.end()),
            ],
            "deferred": false,
            "deferred_reason": "",
            "proto_version": PROTO_VERSION,
        }))
    }

    /// `format` op — returns formatted source as a plain string.
    /// On formatter error the returned string is the input unchanged —
    /// matches the agent's "format is infallible on malformed input"
    /// shape.  Callers wanting the error message should call `check`.
    #[must_use]
    pub fn format(&mut self, source: String) -> String {
        format_with(&source, &FormatOptions::default()).unwrap_or(source)
    }

    /// `rewrite` op — apply `Diagnostic::fixes` edits by id.
    /// `fix_ids` is a JS array of strings — `None`/missing is treated as
    /// an empty list.
    pub fn rewrite(&mut self, source: String, fix_ids: JsValue) -> JsValue {
        let fix_ids: Vec<String> = serde_wasm_bindgen::from_value(fix_ids).unwrap_or_default();
        let id = intern_file(
            &mut self.db,
            &mut self.files,
            source.clone(),
            Dialect::GqlAligned,
        );
        let payload = rewrite_shared(&self.db, id, &source, &fix_ids);
        let applied_edits: Vec<Value> = payload
            .applied_edits
            .into_iter()
            .map(|e| {
                json!({
                    "fix_id": e.fix_id,
                    "range": [u32::from(e.range.start()), u32::from(e.range.end())],
                    "replacement": e.replacement,
                })
            })
            .collect();
        to_js(&json!({
            "op": "rewrite",
            "applied_edits": applied_edits,
            "resulting_text": payload.resulting_text,
            "deferred": false,
            "deferred_reason": if payload.unknown_fix_ids.is_empty() {
                String::new()
            } else {
                format!("unknown fix_ids: {}", payload.unknown_fix_ids.join(", "))
            },
            "proto_version": PROTO_VERSION,
        }))
    }

    // ----------------------------------------------------------------------
    // plan / explain — surface the db's PlanOutput Debug view.
    //
    // Spec 0004 §3 narrows cypher-wasm's allow-listed deps to
    // { cypher-lang-services, cypher-db, cypher-diag, cypher-fmt,
    //   wasm-bindgen, serde-wasm-bindgen }.  The full plan-JSON
    // serialisation path lives behind cypher-plan's `serde` feature —
    // not reachable from here without widening the dep set.  Until a
    // neutral helper lands in cypher-lang-services (cy-od5 follow-up),
    // the wasm ops return a Debug-formatted string for plan and explain.
    // ----------------------------------------------------------------------

    /// `plan` op — surfaces the [`cypher_db::PlanOutput`] Debug dump as
    /// `plan_text`.  See the module-level docs for the trade-off.
    pub fn plan(&mut self, source: String, dialect: Option<String>) -> JsValue {
        let dialect = parse_dialect(dialect.as_deref());
        let id = intern_file(&mut self.db, &mut self.files, source, dialect);
        let value = match self.db.plan_of(id) {
            Ok(plan) => json!({
                "op": "plan",
                "plan_text": format!("{:#?}", plan.plan()),
                "proto_version": PROTO_VERSION,
            }),
            Err(e) => error_value(&e.to_string()),
        };
        to_js(&value)
    }

    /// `explain` op — plain-text summary of the plan.  Currently the
    /// same Debug dump as `plan`; superseded when the neutral pretty
    /// helper lands in `cypher-lang-services`.
    pub fn explain(&mut self, source: String, dialect: Option<String>) -> JsValue {
        let dialect = parse_dialect(dialect.as_deref());
        let id = intern_file(&mut self.db, &mut self.files, source, dialect);
        let value = match self.db.plan_of(id) {
            Ok(plan) => json!({
                "op": "explain",
                "markdown": format!("{:#?}", plan.plan()),
                "proto_version": PROTO_VERSION,
            }),
            Err(e) => error_value(&e.to_string()),
        };
        to_js(&value)
    }

    // ----------------------------------------------------------------------
    // schemaSet / schemaClear
    // ----------------------------------------------------------------------

    /// `schema_set` op — install a JSON schema.  Returns `{ ok: true }`
    /// on success; an error object otherwise.
    #[wasm_bindgen(js_name = schemaSet)]
    pub fn schema_set(&mut self, schema_json: JsValue) -> JsValue {
        let value: Result<WasmSchema, _> = serde_wasm_bindgen::from_value(schema_json);
        match value {
            Ok(schema) => {
                let schema: Arc<dyn cypher_schema::SchemaProvider> = Arc::new(schema);
                self.db.set_schema(Some(schema));
                to_js(&json!({
                    "op": "schema_set",
                    "ok": true,
                    "proto_version": PROTO_VERSION,
                }))
            }
            Err(e) => to_js(&error_value(&format!("invalid schema_json: {e}"))),
        }
    }

    /// `schema_clear` op — drop the in-session schema.  Always succeeds.
    #[wasm_bindgen(js_name = schemaClear)]
    pub fn schema_clear(&mut self) -> JsValue {
        self.db.set_schema(None);
        to_js(&json!({
            "op": "schema_clear",
            "ok": true,
            "proto_version": PROTO_VERSION,
        }))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an error response value in the agent-compatible shape.
fn error_value(message: &str) -> Value {
    json!({
        "op": "error",
        "message": message,
        "proto_version": PROTO_VERSION,
    })
}

/// Convert a `serde_json::Value` into a JS object via
/// `serde-wasm-bindgen`.  On encoding error (should be unreachable for
/// well-formed JSON), fall back to the JS `null` value so callers see a
/// missing-payload rather than a panic.
fn to_js(value: &Value) -> JsValue {
    serde_wasm_bindgen::to_value(value).unwrap_or(JsValue::NULL)
}

// ---------------------------------------------------------------------------
// Native-target unit tests.
// wasm-bindgen-test runs under `wasm32-unknown-unknown` only; these tests
// exercise the plain Rust happy-paths on the host toolchain so the gate
// catches regressions without needing wasm tooling installed locally.
// Feature-gated smoke tests (see cypher-wasm/tests/wasm_smoke.rs) take
// over for the wasm target proper.
// ---------------------------------------------------------------------------

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    fn new_db() -> CypherDatabase {
        CypherDatabase::new()
    }

    #[test]
    fn proto_version_is_one() {
        assert_eq!(CypherDatabase::proto_version_static(), 1);
    }

    #[test]
    fn intern_file_same_dialect_same_fileid() {
        let mut db = new_db();
        let id1 = intern_file(
            &mut db.db,
            &mut db.files,
            "MATCH (n) RETURN n".into(),
            Dialect::GqlAligned,
        );
        let id2 = intern_file(
            &mut db.db,
            &mut db.files,
            "RETURN 42".into(),
            Dialect::GqlAligned,
        );
        assert_eq!(id1, id2, "same dialect must reuse the one interned FileId");
    }

    #[test]
    fn intern_file_different_dialects_distinct_fileids() {
        let mut db = new_db();
        let a = intern_file(
            &mut db.db,
            &mut db.files,
            "MATCH (n) RETURN n".into(),
            Dialect::GqlAligned,
        );
        let b = intern_file(
            &mut db.db,
            &mut db.files,
            "MATCH (n) RETURN n".into(),
            Dialect::OpenCypherV9,
        );
        assert_ne!(a, b, "distinct dialects must mint distinct FileIds");
    }

    #[test]
    fn parse_dialect_unknown_falls_back_to_default() {
        assert!(matches!(parse_dialect(None), Dialect::GqlAligned));
        assert!(matches!(
            parse_dialect(Some("nonsense")),
            Dialect::GqlAligned
        ));
        assert!(matches!(
            parse_dialect(Some("open_cypher_v9")),
            Dialect::OpenCypherV9
        ));
    }

    #[test]
    fn format_is_infallible_on_malformed_input() {
        let mut db = new_db();
        let out = db.format("return 1".into());
        assert!(
            out.contains("RETURN") || out.contains("return"),
            "format should return something printable: {out:?}"
        );
    }
}
