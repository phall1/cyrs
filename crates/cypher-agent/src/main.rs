//! `cypher-agent` — JSON-over-stdio agent API. Spec 0001 §15.
//!
//! One request per line on stdin; one response per line on stdout. All
//! operations synchronous. No network, no subprocess, no filesystem
//! writes (sandbox-safe per §15.3).
//!
//! ## Operations (spec §15.2)
//!
//! | op             | request fields                          | response fields                        |
//! |----------------|-----------------------------------------|----------------------------------------|
//! | `parse`        | `text`, `dialect?`                      | `cst_string`, `syntax_errors`          |
//! | `check`        | `text`, `dialect?`                      | `diagnostics`                          |
//! | `complete`     | `text`, `offset`, `dialect?`            | `items`, `deferred`, `deferred_reason` |
//! | `hover`        | `text`, `offset`, `dialect?`            | `markdown`, `range`, `deferred`, `deferred_reason` |
//! | `format`       | `text`                                  | `formatted`                            |
//! | `rewrite`      | `text`, `fix_ids`                       | `applied_edits`, `resulting_text`, `deferred`, `deferred_reason` |
//! | `plan`         | `text`, `dialect?`                      | `plan_json`                            |
//! | `explain`      | `text`, `dialect?`                      | `markdown`                             |
//! | `schema_set`   | `schema_json`                           | `ok: true`                             |
//! | `schema_clear` | —                                       | `ok: true`                             |
//! | `shutdown`     | —                                       | (exits loop)                           |
//!
//! ## Engine status
//!
//! `complete`, `hover`, and `rewrite` use real engines (cy-da0 /
//! cy-o59 / cy-taz) that mirror the LSP v1 logic: keyword + label +
//! parameter completion; keyword + binding hover; and FixIt-driven
//! rewrite keyed off `Diagnostic.fixes`.  Responses still carry
//! `deferred` / `deferred_reason` for backwards compatibility —
//! `deferred` is now `false` except on rewrite, where
//! `deferred_reason` may carry an `unknown fix_ids: …` note when a
//! requested id does not match any `FixIt`.

#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use cypher_db::{Database, DialectMode, FileId};
use cypher_diag::json as diag_json;
use cypher_fmt::{FormatOptions, format_with};
use cypher_hir::desugar::desugar_statement;
use cypher_hir::lower::lower_statement as hir_lower;
use cypher_lang_services::{
    CompletionItem as NeutralItem, CompletionItemKind as NeutralKind, complete as complete_shared,
    hover as hover_shared, rewrite as rewrite_shared,
};
use cypher_plan::lower::lower_statement as plan_lower;
use cypher_plan::pretty::pretty as plan_pretty;
use cypher_schema::SchemaProvider;
use cypher_syntax::TextSize;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Dialect
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Dialect {
    #[default]
    GqlAligned,
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

// ---------------------------------------------------------------------------
// DialectFiles — one FileId per dialect (spec §11.6)
// ---------------------------------------------------------------------------

/// In-session `FileId` registry holding at most one `FileId` per dialect.
///
/// Per spec §11.6, the agent is stateless-per-call: every request carries
/// `{text}` rather than a stable handle.  To keep Salsa's memoisation budget
/// bounded by the per-query LRU caps (cy-31b) rather than by request count,
/// we intern all requests onto a single `FileId` per dialect.  The `FileId`
/// is stable for the life of the process; only its source text is rewritten
/// on each request.  Steady state is N dialects = 2 `FileId`s today.
///
/// `schema_set` / `schema_clear` update the workspace-scoped schema input
/// on the database, which bumps `schema_digest` and invalidates the
/// schema-aware derived queries without minting or evicting `FileId`s.
#[derive(Default)]
struct DialectFiles {
    gql: Option<FileId>,
    openc: Option<FileId>,
}

impl DialectFiles {
    fn new() -> Self {
        Self::default()
    }

    /// Return the `FileId` slot for `dialect`, if one has been interned.
    fn get(&self, dialect: Dialect) -> Option<FileId> {
        match dialect {
            Dialect::GqlAligned => self.gql,
            Dialect::OpenCypherV9 => self.openc,
        }
    }

    /// Set the `FileId` slot for `dialect`.
    fn set(&mut self, dialect: Dialect, id: FileId) {
        match dialect {
            Dialect::GqlAligned => self.gql = Some(id),
            Dialect::OpenCypherV9 => self.openc = Some(id),
        }
    }

    /// Number of interned `FileId`s (test-only introspection).
    #[cfg(test)]
    fn len(&self) -> usize {
        usize::from(self.gql.is_some()) + usize::from(self.openc.is_some())
    }
}

// ---------------------------------------------------------------------------
// Request (spec §15.2)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum AgentRequest {
    /// parse: source → CST pretty-print + parse diagnostics
    Parse {
        text: String,
        #[serde(default)]
        dialect: Dialect,
    },
    /// check: source → all diagnostics (parse + sema)
    Check {
        text: String,
        #[serde(default)]
        dialect: Dialect,
    },
    /// complete: source + offset → completion items (cy-da0).
    Complete {
        text: String,
        offset: u32,
        #[serde(default)]
        dialect: Dialect,
    },
    /// hover: source + offset → markdown + byte range (cy-o59).
    Hover {
        text: String,
        offset: u32,
        #[serde(default)]
        dialect: Dialect,
    },
    /// format: source → formatted source
    Format { text: String },
    /// rewrite: source + `fix_ids` → applied edits + resulting text (cy-taz).
    Rewrite {
        text: String,
        #[serde(default)]
        fix_ids: Vec<String>,
    },
    /// plan: source → plan JSON
    Plan {
        text: String,
        #[serde(default)]
        #[allow(dead_code)]
        dialect: Dialect,
    },
    /// explain: source → human-readable summary
    Explain {
        text: String,
        #[serde(default)]
        #[allow(dead_code)]
        dialect: Dialect,
    },
    /// `schema_set`: schema JSON object → ok
    SchemaSet { schema_json: Value },
    /// `schema_clear`: → ok
    SchemaClear,
    /// shutdown: → exits the agent loop
    Shutdown,
}

// ---------------------------------------------------------------------------
// Response (spec §15.2)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum AgentResponse {
    /// parse response
    Parse {
        cst_string: String,
        syntax_errors: Vec<String>,
    },
    /// check response
    Check { diagnostics: Vec<Value> },
    /// complete response (v1: engine deferred; see top-level docs).
    Complete {
        items: Vec<Value>,
        deferred: bool,
        deferred_reason: String,
    },
    /// hover response (v1: engine deferred; see top-level docs).
    Hover {
        markdown: String,
        range: [u32; 2],
        deferred: bool,
        deferred_reason: String,
    },
    /// format response
    Format { formatted: String },
    /// rewrite response (v1: fix-application engine deferred; see top-level docs).
    Rewrite {
        applied_edits: Vec<Value>,
        resulting_text: String,
        deferred: bool,
        deferred_reason: String,
    },
    /// plan response
    Plan { plan_json: Value },
    /// explain response
    Explain { markdown: String },
    /// `schema_set` response
    SchemaSet { ok: bool },
    /// `schema_clear` response
    SchemaClear { ok: bool },
    /// shutdown response (sent before exit)
    Shutdown,
    /// error response (malformed input or internal error)
    Error { message: String },
}

// ---------------------------------------------------------------------------
// In-memory schema loaded from schema_set JSON (mirrors cypher-testkit)
// ---------------------------------------------------------------------------

/// A minimal `SchemaProvider` deserialized from the agent's `schema_set` JSON.
/// Mirrors the `JsonSchema` in `cypher-testkit` so the wire shape is identical.
#[derive(Debug, serde::Deserialize)]
struct AgentSchema {
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    rel_types: Vec<String>,
    #[serde(default)]
    node_properties: std::collections::BTreeMap<String, Vec<AgentPropDecl>>,
    #[serde(default)]
    rel_properties: std::collections::BTreeMap<String, Vec<AgentPropDecl>>,
    #[serde(default)]
    rel_endpoints: std::collections::BTreeMap<String, Vec<AgentEndpoint>>,
    #[serde(default)]
    functions: Vec<AgentFnDecl>,
    #[serde(default)]
    procedures: Vec<AgentProcDecl>,
}

#[derive(Debug, serde::Deserialize)]
struct AgentPropDecl {
    name: String,
    #[serde(rename = "type", default = "default_any")]
    ty: String,
    #[serde(default)]
    required: bool,
}

fn default_any() -> String {
    "Any".to_string()
}

#[derive(Debug, serde::Deserialize)]
struct AgentEndpoint {
    source: String,
    target: String,
}

#[derive(Debug, serde::Deserialize)]
struct AgentFnDecl {
    name: String,
    #[serde(default)]
    params: Vec<String>,
    #[serde(default = "default_any")]
    return_type: String,
    variadic: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AgentProcDecl {
    name: String,
    #[serde(default)]
    params: Vec<String>,
    #[serde(default)]
    yield_columns: Vec<String>,
}

fn parse_prop_type(s: &str) -> cypher_schema::PropertyType {
    match s {
        "String" => cypher_schema::PropertyType::String,
        "Int" => cypher_schema::PropertyType::Int,
        "Float" => cypher_schema::PropertyType::Float,
        "Bool" => cypher_schema::PropertyType::Bool,
        "Date" => cypher_schema::PropertyType::Date,
        "Datetime" => cypher_schema::PropertyType::Datetime,
        _ => cypher_schema::PropertyType::Any,
    }
}

impl SchemaProvider for AgentSchema {
    fn labels(&self) -> Vec<smol_str::SmolStr> {
        self.labels.iter().map(smol_str::SmolStr::new).collect()
    }

    fn relationship_types(&self) -> Vec<smol_str::SmolStr> {
        self.rel_types.iter().map(smol_str::SmolStr::new).collect()
    }

    fn node_properties(&self, label: &str) -> Option<Vec<cypher_schema::PropertyDecl>> {
        if !self.labels.iter().any(|l| l == label) {
            return None;
        }
        let decls = self.node_properties.get(label).map_or_else(Vec::new, |ps| {
            ps.iter()
                .map(|p| {
                    cypher_schema::PropertyDecl::new(
                        smol_str::SmolStr::new(&p.name),
                        parse_prop_type(&p.ty),
                        p.required,
                    )
                })
                .collect()
        });
        Some(decls)
    }

    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<cypher_schema::PropertyDecl>> {
        if !self.rel_types.iter().any(|r| r == rel_type) {
            return None;
        }
        let decls = self
            .rel_properties
            .get(rel_type)
            .map_or_else(Vec::new, |ps| {
                ps.iter()
                    .map(|p| {
                        cypher_schema::PropertyDecl::new(
                            smol_str::SmolStr::new(&p.name),
                            parse_prop_type(&p.ty),
                            p.required,
                        )
                    })
                    .collect()
            });
        Some(decls)
    }

    fn relationship_endpoints(&self, rel_type: &str) -> Vec<cypher_schema::EndpointDecl> {
        self.rel_endpoints
            .get(rel_type)
            .map_or_else(Vec::new, |eps| {
                eps.iter()
                    .map(|ep| cypher_schema::EndpointDecl {
                        from: smol_str::SmolStr::new(&ep.source),
                        to: smol_str::SmolStr::new(&ep.target),
                        cardinality: cypher_schema::Cardinality::ManyToMany,
                    })
                    .collect()
            })
    }

    fn inverse_of(&self, _rel_type: &str) -> Option<smol_str::SmolStr> {
        None
    }

    fn function(&self, name: &str) -> Option<cypher_schema::FunctionSignature> {
        self.functions.iter().find(|f| f.name == name).map(|f| {
            let params = f
                .params
                .iter()
                .map(|p| cypher_schema::ParamDecl {
                    name: smol_str::SmolStr::new(p),
                    ty: cypher_schema::PropertyType::Any,
                    default: None,
                })
                .collect();
            let variadic = f.variadic.as_deref().map(|_| cypher_schema::ParamDecl {
                name: smol_str::SmolStr::new("..."),
                ty: cypher_schema::PropertyType::Any,
                default: None,
            });
            cypher_schema::FunctionSignature {
                name: smol_str::SmolStr::new(&f.name),
                params,
                variadic,
                return_ty: cypher_schema::ReturnTy::Constant(parse_prop_type(&f.return_type)),
                categories: cypher_schema::FnCategories {
                    pure: true,
                    aggregate: false,
                    deterministic: true,
                },
            }
        })
    }

    fn procedure(&self, name: &str) -> Option<cypher_schema::ProcedureSignature> {
        self.procedures.iter().find(|p| p.name == name).map(|p| {
            let params = p
                .params
                .iter()
                .map(|param| cypher_schema::ParamDecl {
                    name: smol_str::SmolStr::new(param),
                    ty: cypher_schema::PropertyType::Any,
                    default: None,
                })
                .collect();
            let yields = p
                .yield_columns
                .iter()
                .map(|col| cypher_schema::YieldDecl {
                    name: smol_str::SmolStr::new(col),
                    ty: cypher_schema::PropertyType::Any,
                })
                .collect();
            cypher_schema::ProcedureSignature {
                name: smol_str::SmolStr::new(&p.name),
                params,
                yields,
                mode: cypher_schema::ProcMode::Read,
            }
        })
    }

    fn schema_digest(&self) -> [u8; 32] {
        // Simple stable hash for the agent schema using sha2-like via XOR.
        // Spec §8 requires it changes on observable change; deterministic.
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for l in &self.labels {
            l.hash(&mut h);
        }
        for r in &self.rel_types {
            r.hash(&mut h);
        }
        let n = h.finish();
        let mut digest = [0u8; 32];
        digest[..8].copy_from_slice(&n.to_le_bytes());
        digest
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CYPHER_AGENT_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    // In-session schema, set/cleared by schema_set/schema_clear ops.
    let mut session_schema: Option<Arc<dyn SchemaProvider>> = None;
    // Shared database and per-dialect FileId registry (spec §11.6).
    let mut db = Database::new();
    let mut dialect_files = DialectFiles::new();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<AgentRequest>(&line) {
            Ok(req) => handle(req, &mut session_schema, &mut db, &mut dialect_files),
            Err(e) => AgentResponse::Error {
                message: e.to_string(),
            },
        };
        let is_shutdown = matches!(response, AgentResponse::Shutdown);
        serde_json::to_writer(&mut stdout, &response)?;
        writeln!(stdout)?;
        stdout.flush()?;
        if is_shutdown {
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Intern `source` into the single `FileId` reserved for `dialect` (spec §11.6).
///
/// On first call for this dialect, opens a new file in `db` and records its
/// `FileId` in `files`.  On every subsequent call, overwrites the existing
/// `FileId`'s source text via [`Database::update_file`], which bumps the
/// Salsa revision and cascades invalidation through the derived queries.
/// The memoisation budget is bounded by the per-query LRU caps (cy-31b) on
/// that single `FileId` rather than by request count.
fn intern_file(
    db: &mut Database,
    files: &mut DialectFiles,
    source: String,
    dialect: Dialect,
) -> FileId {
    if let Some(id) = files.get(dialect) {
        // Stable FileId: overwrite its source so derived queries rerun on the
        // new text.  update_file only errors for unknown FileIds, which
        // cannot happen here — `files` only holds IDs that were minted by
        // open_file on this same `db` and are never removed.
        db.update_file(id, source)
            .expect("interned FileId must remain open");
        return id;
    }
    let id = db.open_file(Path::new("_"), source, dialect.into());
    files.set(dialect, id);
    id
}

fn handle(
    req: AgentRequest,
    session_schema: &mut Option<Arc<dyn SchemaProvider>>,
    db: &mut Database,
    dialect_files: &mut DialectFiles,
) -> AgentResponse {
    match req {
        // ------------------------------------------------------------------
        // parse: CST pretty-print + syntax errors
        // ------------------------------------------------------------------
        AgentRequest::Parse { text, dialect } => {
            let id = intern_file(db, dialect_files, text, dialect);
            match db.parse_cst(id) {
                Ok(out) => {
                    let parse = out.parse();
                    AgentResponse::Parse {
                        cst_string: parse.syntax().to_string(),
                        syntax_errors: parse.errors().iter().map(|e| e.message.clone()).collect(),
                    }
                }
                Err(e) => AgentResponse::Error {
                    message: e.to_string(),
                },
            }
        }

        // ------------------------------------------------------------------
        // check: all diagnostics (parse + sema)
        // ------------------------------------------------------------------
        AgentRequest::Check { text, dialect } => {
            if let Some(schema) = session_schema.clone() {
                db.set_schema(Some(schema));
            }
            let id = intern_file(db, dialect_files, text, dialect);
            match db.all_diagnostics(id) {
                Ok(out) => AgentResponse::Check {
                    diagnostics: out.diagnostics().iter().map(diag_json::to_json).collect(),
                },
                Err(e) => AgentResponse::Error {
                    message: e.to_string(),
                },
            }
        }

        // ------------------------------------------------------------------
        // complete — delegated to cypher-lang-services (cy-da0 / cy-4t0).
        // ------------------------------------------------------------------
        AgentRequest::Complete {
            text,
            offset,
            dialect,
        } => {
            if let Some(schema) = session_schema.clone() {
                db.set_schema(Some(schema));
            }
            let id = intern_file(db, dialect_files, text, dialect);
            let items = complete_shared(db, id, TextSize::from(offset))
                .into_iter()
                .map(completion_item_to_json)
                .collect();
            AgentResponse::Complete {
                items,
                deferred: false,
                deferred_reason: String::new(),
            }
        }

        // ------------------------------------------------------------------
        // hover — delegated to cypher-lang-services (cy-o59 / cy-4t0).
        // ------------------------------------------------------------------
        AgentRequest::Hover {
            text,
            offset,
            dialect,
        } => {
            let id = intern_file(db, dialect_files, text, dialect);
            let payload = hover_shared(db, id, TextSize::from(offset));
            let range = [
                u32::from(payload.range.start()),
                u32::from(payload.range.end()),
            ];
            AgentResponse::Hover {
                markdown: payload.markdown,
                range,
                deferred: false,
                deferred_reason: String::new(),
            }
        }

        // ------------------------------------------------------------------
        // format: formatted source
        // ------------------------------------------------------------------
        AgentRequest::Format { text } => match format_with(&text, &FormatOptions::default()) {
            Ok(formatted) => AgentResponse::Format { formatted },
            Err(e) => AgentResponse::Error {
                message: e.to_string(),
            },
        },

        // ------------------------------------------------------------------
        // rewrite — delegated to cypher-lang-services (cy-taz / cy-4t0).
        // Input text is returned unchanged when no ids match; unknown
        // ids are surfaced for the caller to diff.
        // ------------------------------------------------------------------
        AgentRequest::Rewrite { text, fix_ids } => {
            if let Some(schema) = session_schema.clone() {
                db.set_schema(Some(schema));
            }
            let id = intern_file(db, dialect_files, text.clone(), Dialect::default());
            let payload = rewrite_shared(db, id, &text, &fix_ids);
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
            AgentResponse::Rewrite {
                applied_edits,
                resulting_text: payload.resulting_text,
                deferred: false,
                deferred_reason: if payload.unknown_fix_ids.is_empty() {
                    String::new()
                } else {
                    format!("unknown fix_ids: {}", payload.unknown_fix_ids.join(", "))
                },
            }
        }

        // ------------------------------------------------------------------
        // plan: plan JSON
        // ------------------------------------------------------------------
        AgentRequest::Plan { text, dialect: _ } => {
            let stmt = hir_lower(&text);
            let stmt = desugar_statement(stmt);
            let plan = plan_lower(&stmt);
            match serde_json::to_value(&plan) {
                Ok(plan_json) => AgentResponse::Plan { plan_json },
                Err(e) => AgentResponse::Error {
                    message: e.to_string(),
                },
            }
        }

        // ------------------------------------------------------------------
        // explain: human-readable plan summary
        // ------------------------------------------------------------------
        AgentRequest::Explain { text, dialect: _ } => {
            let stmt = hir_lower(&text);
            let stmt = desugar_statement(stmt);
            let plan = plan_lower(&stmt);
            AgentResponse::Explain {
                markdown: plan_pretty(&plan),
            }
        }

        // ------------------------------------------------------------------
        // schema_set: parse JSON into an in-session schema.  Updates the
        // workspace-scoped schema input on the database so `schema_digest`
        // bumps and schema-aware derived queries invalidate on every
        // already-interned FileId (spec §11.6 — no FileId churn).
        // ------------------------------------------------------------------
        AgentRequest::SchemaSet { schema_json } => {
            match serde_json::from_value::<AgentSchema>(schema_json) {
                Ok(schema) => {
                    let schema: Arc<dyn SchemaProvider> = Arc::new(schema);
                    *session_schema = Some(schema.clone());
                    db.set_schema(Some(schema));
                    AgentResponse::SchemaSet { ok: true }
                }
                Err(e) => AgentResponse::Error {
                    message: format!("invalid schema_json: {e}"),
                },
            }
        }

        // ------------------------------------------------------------------
        // schema_clear: drop the in-session schema and bump the database's
        // `schema_digest` back to the no-schema state.  Invalidates via the
        // `WorkspaceInputs` input, not FileId eviction (spec §11.6).
        // ------------------------------------------------------------------
        AgentRequest::SchemaClear => {
            *session_schema = None;
            db.set_schema(None);
            AgentResponse::SchemaClear { ok: true }
        }

        // ------------------------------------------------------------------
        // shutdown
        // ------------------------------------------------------------------
        AgentRequest::Shutdown => AgentResponse::Shutdown,
    }
}

// ---------------------------------------------------------------------------
// Neutral → JSON adapters for cypher-lang-services DTOs
// ---------------------------------------------------------------------------

/// Translate a neutral [`NeutralItem`] into the agent's wire JSON shape.
///
/// Mirrors the v1 agent completion wire format: `{label, kind}` always
/// present, `detail` only for placeholder parameter items.  The shared
/// engine emits neutral `CompletionItemKind` variants; this helper maps
/// them to the `snake_case` strings the agent has always used.
fn completion_item_to_json(item: NeutralItem) -> Value {
    let kind = match item.kind {
        NeutralKind::Keyword => "keyword",
        NeutralKind::Label => "label",
        NeutralKind::RelationshipType => "relationship_type",
        NeutralKind::Parameter => "parameter",
        NeutralKind::Property => "property",
        // `CompletionItemKind` is `#[non_exhaustive]` (cy-2i9.1).
        _ => "other",
    };
    // Placeholder parameters carry `detail: "placeholder"` on the wire
    // to match the v1 shape; every other item omits the field.
    if matches!(item.kind, NeutralKind::Parameter) && item.detail.as_deref() == Some("placeholder")
    {
        json!({ "label": item.label.to_string(), "kind": kind, "detail": "placeholder" })
    } else {
        json!({ "label": item.label.to_string(), "kind": kind })
    }
}

// ---------------------------------------------------------------------------
// resolve helper used by future resolve op
// (exposed here so test module can reach it)
// ---------------------------------------------------------------------------

/// Build a resolved-name overlay text for `text`.
#[cfg(test)]
fn resolve_overlay(text: &str) -> String {
    use cypher_diag::DiagnosticsSink;
    use cypher_hir::desugar::desugar_statement;
    use cypher_hir::lower::lower_statement;
    use cypher_hir::pretty::print_overlay;

    let stmt = lower_statement(text);
    let stmt = desugar_statement(stmt);
    let mut sink = DiagnosticsSink::new();
    let result = cypher_sema::resolve::resolve(&stmt, false, &mut sink);
    print_overlay(&stmt, &result.resolved_names)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = "MATCH (n) RETURN n";

    // -----------------------------------------------------------------------
    // Serde roundtrip — each request variant
    // -----------------------------------------------------------------------

    #[test]
    fn roundtrip_parse() {
        let json = r#"{"op":"parse","text":"RETURN 1"}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Parse { .. }));
    }

    #[test]
    fn roundtrip_check() {
        let json = r#"{"op":"check","text":"RETURN 1","dialect":"gql_aligned"}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Check { .. }));
    }

    #[test]
    fn roundtrip_complete() {
        let json = r#"{"op":"complete","text":"RETURN 1","offset":6}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Complete { .. }));
    }

    #[test]
    fn roundtrip_hover() {
        let json = r#"{"op":"hover","text":"RETURN 1","offset":3}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Hover { .. }));
    }

    #[test]
    fn roundtrip_format() {
        let json = r#"{"op":"format","text":"return 1"}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Format { .. }));
    }

    #[test]
    fn roundtrip_rewrite() {
        let json = r#"{"op":"rewrite","text":"RETURN 1","fix_ids":["cy-fix.uppercase"]}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Rewrite { .. }));
    }

    #[test]
    fn roundtrip_plan() {
        let json = r#"{"op":"plan","text":"MATCH (n) RETURN n"}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Plan { .. }));
    }

    #[test]
    fn roundtrip_explain() {
        let json = r#"{"op":"explain","text":"MATCH (n) RETURN n"}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Explain { .. }));
    }

    #[test]
    fn roundtrip_schema_set() {
        let json = r#"{"op":"schema_set","schema_json":{"labels":["Person"]}}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::SchemaSet { .. }));
    }

    #[test]
    fn roundtrip_schema_clear() {
        let json = r#"{"op":"schema_clear"}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::SchemaClear));
    }

    #[test]
    fn roundtrip_shutdown() {
        let json = r#"{"op":"shutdown"}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, AgentRequest::Shutdown));
    }

    // -----------------------------------------------------------------------
    // Response serialisation roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn response_parse_serialises() {
        let resp = AgentResponse::Parse {
            cst_string: "RETURN 1".into(),
            syntax_errors: vec![],
        };
        let s = serde_json::to_string(&resp).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["op"], "parse");
        assert_eq!(v["cst_string"], "RETURN 1");
    }

    #[test]
    fn response_error_serialises() {
        let resp = AgentResponse::Error {
            message: "bad json".into(),
        };
        let s = serde_json::to_string(&resp).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["op"], "error");
    }

    // -----------------------------------------------------------------------
    // Dispatcher tests — each op with fixture source
    // -----------------------------------------------------------------------

    fn dispatch(json: &str) -> Value {
        let mut schema: Option<Arc<dyn SchemaProvider>> = None;
        let mut db = Database::new();
        let mut files = DialectFiles::new();
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        let resp = handle(req, &mut schema, &mut db, &mut files);
        serde_json::to_value(resp).unwrap()
    }

    fn dispatch_with_schema(json: &str, schema: &mut Option<Arc<dyn SchemaProvider>>) -> Value {
        let mut db = Database::new();
        let mut files = DialectFiles::new();
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        let resp = handle(req, schema, &mut db, &mut files);
        serde_json::to_value(resp).unwrap()
    }

    #[test]
    fn dispatch_parse_ok() {
        let v = dispatch(r#"{"op":"parse","text":"RETURN 1"}"#);
        assert_eq!(v["op"], "parse");
        assert!(v["cst_string"].as_str().is_some());
    }

    #[test]
    fn dispatch_parse_syntax_error() {
        let v = dispatch(r#"{"op":"parse","text":"RETURN !!!"}"#);
        assert_eq!(v["op"], "parse");
        // syntax_errors array present
        assert!(v["syntax_errors"].is_array());
    }

    #[test]
    fn dispatch_check_ok() {
        let v = dispatch(&format!(r#"{{"op":"check","text":"{SIMPLE}"}}"#));
        assert_eq!(v["op"], "check");
        assert!(v["diagnostics"].is_array());
    }

    #[test]
    fn dispatch_complete_returns_keywords() {
        let v = dispatch(r#"{"op":"complete","text":"","offset":0}"#);
        assert_eq!(v["op"], "complete");
        let items = v["items"].as_array().expect("items array");
        let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
        for kw in ["MATCH", "RETURN", "WHERE"] {
            assert!(labels.contains(&kw), "keyword {kw} missing: {labels:?}");
        }
        assert_eq!(v["deferred"], Value::Bool(false));
    }

    #[test]
    fn dispatch_complete_after_dollar_scans_parameters() {
        // "MATCH (n {name: $who}) RETURN $" — 31 bytes; offset 31
        // sits immediately after the trailing `$` so the trigger
        // classifier picks it up.
        let v =
            dispatch(r#"{"op":"complete","text":"MATCH (n {name: $who}) RETURN $","offset":31}"#);
        let items = v["items"].as_array().expect("items array");
        let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();
        assert!(
            labels.contains(&"who"),
            "parameter scan must surface $who; got {labels:?}"
        );
    }

    #[test]
    fn dispatch_hover_on_keyword_returns_markdown() {
        // Offset 0 is the M of MATCH.
        let v = dispatch(r#"{"op":"hover","text":"MATCH (n) RETURN n","offset":0}"#);
        assert_eq!(v["op"], "hover");
        let md = v["markdown"].as_str().unwrap_or("");
        assert!(
            md.contains("MATCH"),
            "MATCH hover must mention MATCH; got {md:?}"
        );
        assert_eq!(v["deferred"], Value::Bool(false));
    }

    #[test]
    fn dispatch_hover_on_variable_returns_binding_info() {
        // Offset 7 is the n inside MATCH (n).
        let v = dispatch(r#"{"op":"hover","text":"MATCH (n) RETURN n","offset":7}"#);
        let md = v["markdown"].as_str().unwrap_or("");
        assert!(
            md.contains("variable") && md.contains("`n`"),
            "variable hover must name n; got {md:?}"
        );
    }

    #[test]
    fn dispatch_hover_on_punctuation_returns_empty_markdown() {
        // Offset 6 lands on the `(` token — punctuation, not a
        // keyword or IDENT → engine returns empty markdown.
        let v = dispatch(r#"{"op":"hover","text":"MATCH (n) RETURN n","offset":6}"#);
        assert_eq!(v["markdown"], Value::String(String::new()));
        assert_eq!(v["deferred"], Value::Bool(false));
    }

    #[test]
    fn dispatch_hover_past_eof_is_safe() {
        // Offset past EOF must not panic — the engine clamps
        // defensively (rowan panics on out-of-range token_at_offset).
        let v = dispatch(r#"{"op":"hover","text":"MATCH (n) RETURN n","offset":999}"#);
        assert_eq!(v["op"], "hover");
        assert!(v["markdown"].is_string(), "markdown must be present");
    }

    #[test]
    fn dispatch_format_ok() {
        let v = dispatch(r#"{"op":"format","text":"return 1"}"#);
        assert_eq!(v["op"], "format");
        let formatted = v["formatted"].as_str().unwrap();
        assert!(formatted.contains("RETURN") || formatted.contains("return"));
    }

    #[test]
    fn dispatch_rewrite_no_fixes_echoes_text() {
        // No diagnostic in the workspace currently populates
        // `Diagnostic::fixes`, so the engine can't apply anything on
        // real inputs.  Pinning the "no-op echo" behaviour: the
        // resulting_text is the input, applied_edits is empty.
        let v = dispatch(r#"{"op":"rewrite","text":"RETURN 1","fix_ids":[]}"#);
        assert_eq!(v["op"], "rewrite");
        assert_eq!(v["resulting_text"], "RETURN 1");
        assert_eq!(v["applied_edits"], Value::Array(vec![]));
        assert_eq!(v["deferred"], Value::Bool(false));
    }

    #[test]
    fn dispatch_rewrite_unknown_fix_id_surfaces_reason() {
        // Request a fix id that does not exist.  The engine returns
        // unchanged text but flags the unknown id in
        // `deferred_reason` so the client can react.
        let v = dispatch(r#"{"op":"rewrite","text":"RETURN 1","fix_ids":["cy-fix.nope"]}"#);
        assert_eq!(v["resulting_text"], "RETURN 1");
        assert!(
            v["deferred_reason"]
                .as_str()
                .is_some_and(|s| s.contains("cy-fix.nope")),
            "rewrite must name the unknown fix_id; got {}",
            v["deferred_reason"]
        );
    }

    #[test]
    fn dispatch_plan_ok() {
        let v = dispatch(&format!(r#"{{"op":"plan","text":"{SIMPLE}"}}"#));
        assert_eq!(v["op"], "plan");
        assert!(v["plan_json"].is_object());
    }

    #[test]
    fn dispatch_explain_ok() {
        let v = dispatch(&format!(r#"{{"op":"explain","text":"{SIMPLE}"}}"#));
        assert_eq!(v["op"], "explain");
        assert!(v["markdown"].as_str().is_some());
    }

    #[test]
    fn dispatch_schema_set_and_clear() {
        let mut schema: Option<Arc<dyn SchemaProvider>> = None;

        let v = dispatch_with_schema(
            r#"{"op":"schema_set","schema_json":{"labels":["Person"],"rel_types":[]}}"#,
            &mut schema,
        );
        assert_eq!(v["op"], "schema_set");
        assert_eq!(v["ok"], true);
        assert!(schema.is_some());

        let v = dispatch_with_schema(r#"{"op":"schema_clear"}"#, &mut schema);
        assert_eq!(v["op"], "schema_clear");
        assert_eq!(v["ok"], true);
        assert!(schema.is_none());
    }

    #[test]
    fn dispatch_shutdown() {
        let v = dispatch(r#"{"op":"shutdown"}"#);
        assert_eq!(v["op"], "shutdown");
    }

    #[test]
    fn malformed_input_returns_error() {
        let mut schema: Option<Arc<dyn SchemaProvider>> = None;
        let mut db = Database::new();
        let mut files = DialectFiles::new();
        let bad = "not valid json {{{";
        let resp: Value = match serde_json::from_str::<AgentRequest>(bad) {
            Ok(req) => serde_json::to_value(handle(req, &mut schema, &mut db, &mut files)).unwrap(),
            Err(e) => serde_json::to_value(AgentResponse::Error {
                message: e.to_string(),
            })
            .unwrap(),
        };
        assert_eq!(resp["op"], "error");
    }

    #[test]
    fn unknown_op_returns_error() {
        let mut schema: Option<Arc<dyn SchemaProvider>> = None;
        let mut db = Database::new();
        let mut files = DialectFiles::new();
        let unknown = r#"{"op":"nonexistent","text":"x"}"#;
        let resp: Value = match serde_json::from_str::<AgentRequest>(unknown) {
            Ok(req) => serde_json::to_value(handle(req, &mut schema, &mut db, &mut files)).unwrap(),
            Err(e) => serde_json::to_value(AgentResponse::Error {
                message: e.to_string(),
            })
            .unwrap(),
        };
        assert_eq!(resp["op"], "error");
    }

    // -----------------------------------------------------------------------
    // resolve helper smoke test
    // -----------------------------------------------------------------------
    #[test]
    fn resolve_overlay_runs() {
        let overlay = resolve_overlay(SIMPLE);
        // Just check it doesn't panic and returns non-empty.
        assert!(!overlay.is_empty());
    }

    // -----------------------------------------------------------------------
    // DialectFiles / intern_file tests (spec §11.6)
    // -----------------------------------------------------------------------

    /// Same dialect, regardless of source, returns the same stable `FileId`.
    #[test]
    fn intern_file_same_dialect_same_fileid() {
        let mut db = Database::new();
        let mut files = DialectFiles::new();
        let id1 = intern_file(
            &mut db,
            &mut files,
            "MATCH (n) RETURN n".to_string(),
            Dialect::GqlAligned,
        );
        let id2 = intern_file(
            &mut db,
            &mut files,
            "RETURN 42".to_string(),
            Dialect::GqlAligned,
        );
        assert_eq!(
            id1, id2,
            "same dialect must reuse the single interned FileId"
        );
        assert_eq!(files.len(), 1, "exactly one FileId for the one dialect");
        // Source has been overwritten to the most recent request text.
        assert_eq!(db.source_of(id1).unwrap(), "RETURN 42");
    }

    /// Different dialects → different `FileId`s, bounded at N = 2 today.
    #[test]
    fn intern_file_per_dialect_distinct_fileids() {
        let mut db = Database::new();
        let mut files = DialectFiles::new();
        let src = "MATCH (n) RETURN n".to_string();
        let id_gql = intern_file(&mut db, &mut files, src.clone(), Dialect::GqlAligned);
        let id_oc = intern_file(&mut db, &mut files, src, Dialect::OpenCypherV9);
        assert_ne!(
            id_gql, id_oc,
            "each dialect must have its own stable FileId"
        );
        assert_eq!(files.len(), 2, "steady-state ceiling = N dialects = 2");

        // Re-issue requests on both dialects; the set does not grow.
        let again_gql = intern_file(
            &mut db,
            &mut files,
            "RETURN 1".to_string(),
            Dialect::GqlAligned,
        );
        let again_oc = intern_file(
            &mut db,
            &mut files,
            "RETURN 2".to_string(),
            Dialect::OpenCypherV9,
        );
        assert_eq!(again_gql, id_gql);
        assert_eq!(again_oc, id_oc);
        assert_eq!(files.len(), 2);
    }

    /// 1000 distinct source requests on a single dialect produce exactly one
    /// `FileId`.  Proves that request-count growth no longer mints new
    /// `FileId`s (spec §11.6 "memoization budget bounded by LRU caps, not
    /// request count").
    #[test]
    fn intern_file_1000_requests_single_fileid() {
        let mut db = Database::new();
        let mut files = DialectFiles::new();
        let first = intern_file(
            &mut db,
            &mut files,
            "RETURN 0".to_string(),
            Dialect::GqlAligned,
        );
        for i in 1..1000 {
            let id = intern_file(
                &mut db,
                &mut files,
                format!("RETURN {i}"),
                Dialect::GqlAligned,
            );
            assert_eq!(
                id, first,
                "request {i} allocated a new FileId; expected reuse"
            );
        }
        assert_eq!(
            files.len(),
            1,
            "exactly one FileId for the single dialect after 1000 requests"
        );
        // The next FileId the DB would hand out is FileId(1) — confirming
        // only FileId(0) was ever minted.
        assert!(db.is_open(first));
        let gap = cypher_db::FileId(first.0.wrapping_add(1));
        assert!(
            !db.is_open(gap),
            "no second FileId was minted by the 999 re-requests"
        );
    }

    /// `schema_set` on a dialect that already has an interned `FileId` must
    /// update diagnostics via `schema_digest` invalidation (bump
    /// `WorkspaceInputs`, not mint a new `FileId`).  We observe:
    ///
    /// 1. `check` on the dialect with no schema set → schema-free mode,
    ///    unknown-label checks don't fire.  Interns the dialect's `FileId`.
    /// 2. `schema_set` installs a schema that declares only `Person`.
    /// 3. `check` on the same text (`MATCH (n:Ghost) RETURN n`) now emits
    ///    `E3001` (unknown label) — the diagnostics set changed, and it
    ///    changed because `schema_digest` invalidated the sema memo on the
    ///    **same** `FileId`.  No `FileId` churn.
    #[test]
    fn schema_set_reuses_interned_fileid_and_refreshes_diagnostics() {
        let mut schema: Option<Arc<dyn SchemaProvider>> = None;
        let mut db = Database::new();
        let mut files = DialectFiles::new();

        // 1. Check without any schema — schema-free mode, no E3001.
        let before = serde_json::to_value(handle(
            AgentRequest::Check {
                text: "MATCH (n:Ghost) RETURN n".into(),
                dialect: Dialect::GqlAligned,
            },
            &mut schema,
            &mut db,
            &mut files,
        ))
        .unwrap();
        assert_eq!(before["op"], "check");
        let id_before = files
            .get(Dialect::GqlAligned)
            .expect("interned after check");
        assert_eq!(files.len(), 1);
        let before_codes: Vec<String> = before["diagnostics"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|d| d["code"].as_str().map(ToString::to_string))
            .collect();
        assert!(
            !before_codes.iter().any(|c| c == "E3001"),
            "schema-free mode must not emit E3001; got {before_codes:?}"
        );

        // 2. Install a schema that declares only `Person` — `Ghost` is unknown.
        let resp = handle(
            AgentRequest::SchemaSet {
                schema_json: serde_json::json!({
                    "labels": ["Person"],
                    "rel_types": [],
                }),
            },
            &mut schema,
            &mut db,
            &mut files,
        );
        assert!(matches!(resp, AgentResponse::SchemaSet { ok: true }));

        // 3. Check again — same text, same dialect.  The FileId must be
        //    reused; E3001 must now appear.
        let after = serde_json::to_value(handle(
            AgentRequest::Check {
                text: "MATCH (n:Ghost) RETURN n".into(),
                dialect: Dialect::GqlAligned,
            },
            &mut schema,
            &mut db,
            &mut files,
        ))
        .unwrap();
        let id_after = files
            .get(Dialect::GqlAligned)
            .expect("still interned after schema_set");
        assert_eq!(
            id_before, id_after,
            "schema_set must not churn the dialect's FileId"
        );
        assert_eq!(files.len(), 1, "no FileIds were minted by schema_set");

        let after_codes: Vec<String> = after["diagnostics"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|d| d["code"].as_str().map(ToString::to_string))
            .collect();
        assert!(
            after_codes.iter().any(|c| c == "E3001"),
            "schema_digest invalidation must let E3001 surface on the interned FileId; got {after_codes:?}"
        );
    }
}
