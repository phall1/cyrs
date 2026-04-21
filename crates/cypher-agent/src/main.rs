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
//! ## v1 deferrals
//!
//! `complete`, `hover`, and `rewrite` accept requests and return a
//! well-formed response, but the underlying engine is deferred to v2.  Each
//! response carries `deferred: true` and `deferred_reason` so callers can
//! detect the deferral programmatically rather than by inspecting whether
//! an empty list was "no matches" or "not implemented".  Body fields
//! (`items`, `markdown`, `applied_edits`, …) are populated with empty /
//! identity values so clients that ignore the flag continue to work.

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use cypher_db::{Database, DialectMode, FileId};
use cypher_diag::json as diag_json;
use cypher_fmt::{FormatOptions, format_with};
use cypher_hir::desugar::desugar_statement;
use cypher_hir::lower::lower_statement as hir_lower;
use cypher_plan::lower::lower_statement as plan_lower;
use cypher_plan::pretty::pretty as plan_pretty;
use cypher_schema::SchemaProvider;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
// v1 deferral reasons — surfaced on `complete`/`hover`/`rewrite` responses
// so callers can distinguish "no matches" from "engine not yet implemented"
// (spec §15.2; see top-level docs).
// ---------------------------------------------------------------------------

const DEFERRAL_COMPLETE: &str =
    "v1: completion engine deferred (spec §15.2 complete); planned for v2.";
const DEFERRAL_HOVER: &str = "v1: hover engine deferred (spec §15.2 hover); planned for v2.";
const DEFERRAL_REWRITE: &str =
    "v1: fix-application engine deferred (spec §15.2 rewrite); planned for v2.";

// ---------------------------------------------------------------------------
// FileCache — source+dialect interning (spec §15.X)
// ---------------------------------------------------------------------------

/// Maximum number of interned files kept in the cache before eviction.
const FILE_CACHE_CEILING: usize = 64;

/// FNV-1a 64-bit inline hasher (same algorithm as cypher-db/inputs.rs).
fn fnv1a(data: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Cache key: FNV-1a digest of `source` bytes followed by dialect discriminant byte.
fn cache_key(source: &str, dialect: Dialect) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    // hash source bytes first
    let mut h = OFFSET;
    for &b in source.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(PRIME);
    }
    // mix in dialect discriminant
    let d: u8 = match dialect {
        Dialect::GqlAligned => 0,
        Dialect::OpenCypherV9 => 1,
    };
    h ^= u64::from(d);
    h = h.wrapping_mul(PRIME);
    let _ = fnv1a; // suppress dead-code warning on the standalone fn
    h
}

/// In-session `FileId` intern cache with LRU eviction.
///
/// Maps `(source_digest, dialect)` → `FileId`.  When the ceiling is reached,
/// the least-recently-used entry is evicted and `Database::remove_file` is
/// called so the Salsa cache releases that slot.
struct FileCache {
    /// key → `FileId` lookup
    map: std::collections::HashMap<u64, FileId>,
    /// LRU order: front = least-recently-used, back = most-recently-used.
    order: VecDeque<u64>,
}

impl FileCache {
    fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Look up an existing `FileId` for `key`, promoting it in the LRU order.
    fn get(&mut self, key: u64) -> Option<FileId> {
        if let Some(&id) = self.map.get(&key) {
            // Promote: move key to the back (most-recently-used).
            if let Some(pos) = self.order.iter().position(|&k| k == key) {
                self.order.remove(pos);
            }
            self.order.push_back(key);
            Some(id)
        } else {
            None
        }
    }

    /// Insert a new `(key, id)` pair.  Returns the evicted key+id if the
    /// ceiling was exceeded; caller must call `db.remove_file(evicted_id)`.
    fn insert(&mut self, key: u64, id: FileId) -> Option<(u64, FileId)> {
        let evicted = if self.map.len() >= FILE_CACHE_CEILING {
            // Pop LRU entry (front of deque).
            if let Some(old_key) = self.order.pop_front() {
                let old_id = self.map.remove(&old_key);
                old_id.map(|eid| (old_key, eid))
            } else {
                None
            }
        } else {
            None
        };
        self.map.insert(key, id);
        self.order.push_back(key);
        evicted
    }

    /// Number of cached entries.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }

    /// Return the LRU (oldest) key, if any.
    #[cfg(test)]
    fn lru_key(&self) -> Option<u64> {
        self.order.front().copied()
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
    /// complete: source + offset → completion items (stub in v1)
    Complete {
        #[allow(dead_code)]
        text: String,
        #[allow(dead_code)]
        offset: u32,
        #[serde(default)]
        #[allow(dead_code)]
        dialect: Dialect,
    },
    /// hover: source + offset → hover markdown (stub in v1)
    Hover {
        #[allow(dead_code)]
        text: String,
        #[allow(dead_code)]
        offset: u32,
        #[serde(default)]
        #[allow(dead_code)]
        dialect: Dialect,
    },
    /// format: source → formatted source
    Format { text: String },
    /// rewrite: source + `fix_ids` → applied edits + resulting text (stub in v1)
    Rewrite {
        text: String,
        #[serde(default)]
        #[allow(dead_code)]
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
                .map(|p| cypher_schema::PropertyDecl {
                    name: smol_str::SmolStr::new(&p.name),
                    ty: parse_prop_type(&p.ty),
                    required: p.required,
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
                    .map(|p| cypher_schema::PropertyDecl {
                        name: smol_str::SmolStr::new(&p.name),
                        ty: parse_prop_type(&p.ty),
                        required: p.required,
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
    // Shared database and FileId intern cache (spec §15.X).
    let mut db = Database::new();
    let mut file_cache = FileCache::new();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<AgentRequest>(&line) {
            Ok(req) => handle(req, &mut session_schema, &mut db, &mut file_cache),
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

/// Intern `source` + `dialect` into the `FileCache`, returning a stable `FileId`.
///
/// On cache hit the existing `FileId` is returned immediately (LRU promoted).
/// On cache miss a new file is opened in `db`, inserted into the cache, and if
/// the ceiling was exceeded the evicted file is removed from `db` to release
/// its Salsa cache slot.
fn intern_file(
    db: &mut Database,
    cache: &mut FileCache,
    source: String,
    dialect: Dialect,
) -> FileId {
    let key = cache_key(&source, dialect);
    if let Some(id) = cache.get(key) {
        return id;
    }
    // Cache miss: open a new file.
    let id = db.open_file(Path::new("_"), source, dialect.into());
    // Insert and evict LRU if over ceiling.
    if let Some((_evicted_key, evicted_id)) = cache.insert(key, id) {
        let _ = db.remove_file(evicted_id); // ignore UnknownFileId (shouldn't happen)
    }
    id
}

fn handle(
    req: AgentRequest,
    session_schema: &mut Option<Arc<dyn SchemaProvider>>,
    db: &mut Database,
    file_cache: &mut FileCache,
) -> AgentResponse {
    match req {
        // ------------------------------------------------------------------
        // parse: CST pretty-print + syntax errors
        // ------------------------------------------------------------------
        AgentRequest::Parse { text, dialect } => {
            let id = intern_file(db, file_cache, text, dialect);
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
            let id = intern_file(db, file_cache, text, dialect);
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
        // complete: v1 deferral — completion engine ships in v2.  Callers
        // should key off `deferred` rather than `items.is_empty()`.
        // ------------------------------------------------------------------
        AgentRequest::Complete {
            text: _,
            offset: _,
            dialect: _,
        } => AgentResponse::Complete {
            items: vec![],
            deferred: true,
            deferred_reason: DEFERRAL_COMPLETE.to_owned(),
        },

        // ------------------------------------------------------------------
        // hover: v1 deferral — hover engine ships in v2.
        // ------------------------------------------------------------------
        AgentRequest::Hover {
            text: _,
            offset: _,
            dialect: _,
        } => AgentResponse::Hover {
            markdown: String::new(),
            range: [0, 0],
            deferred: true,
            deferred_reason: DEFERRAL_HOVER.to_owned(),
        },

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
        // rewrite: v1 deferral — fix-application engine ships in v2.
        // We echo the input text unchanged and return an empty edit list
        // so callers that ignore `deferred` still get a sensible response.
        // ------------------------------------------------------------------
        AgentRequest::Rewrite { text, fix_ids: _ } => AgentResponse::Rewrite {
            applied_edits: vec![],
            resulting_text: text,
            deferred: true,
            deferred_reason: DEFERRAL_REWRITE.to_owned(),
        },

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
        // schema_set: parse JSON into an in-session schema
        // ------------------------------------------------------------------
        AgentRequest::SchemaSet { schema_json } => {
            match serde_json::from_value::<AgentSchema>(schema_json) {
                Ok(schema) => {
                    *session_schema = Some(Arc::new(schema));
                    AgentResponse::SchemaSet { ok: true }
                }
                Err(e) => AgentResponse::Error {
                    message: format!("invalid schema_json: {e}"),
                },
            }
        }

        // ------------------------------------------------------------------
        // schema_clear: drop the in-session schema
        // ------------------------------------------------------------------
        AgentRequest::SchemaClear => {
            *session_schema = None;
            AgentResponse::SchemaClear { ok: true }
        }

        // ------------------------------------------------------------------
        // shutdown
        // ------------------------------------------------------------------
        AgentRequest::Shutdown => AgentResponse::Shutdown,
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
        let mut cache = FileCache::new();
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        let resp = handle(req, &mut schema, &mut db, &mut cache);
        serde_json::to_value(resp).unwrap()
    }

    fn dispatch_with_schema(json: &str, schema: &mut Option<Arc<dyn SchemaProvider>>) -> Value {
        let mut db = Database::new();
        let mut cache = FileCache::new();
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        let resp = handle(req, schema, &mut db, &mut cache);
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
    fn dispatch_complete_deferred() {
        let v = dispatch(r#"{"op":"complete","text":"RETURN 1","offset":3}"#);
        assert_eq!(v["op"], "complete");
        assert_eq!(v["items"], Value::Array(vec![]));
        assert_eq!(v["deferred"], Value::Bool(true));
        assert!(
            v["deferred_reason"]
                .as_str()
                .is_some_and(|s| s.contains("v1") && s.contains("v2")),
            "complete deferred_reason must name both v1 and v2: got {}",
            v["deferred_reason"]
        );
    }

    #[test]
    fn dispatch_hover_deferred() {
        let v = dispatch(r#"{"op":"hover","text":"RETURN 1","offset":3}"#);
        assert_eq!(v["op"], "hover");
        assert!(v["markdown"].is_string());
        assert_eq!(v["deferred"], Value::Bool(true));
        assert!(
            v["deferred_reason"]
                .as_str()
                .is_some_and(|s| s.contains("v1") && s.contains("v2")),
            "hover deferred_reason must name both v1 and v2: got {}",
            v["deferred_reason"]
        );
    }

    #[test]
    fn dispatch_format_ok() {
        let v = dispatch(r#"{"op":"format","text":"return 1"}"#);
        assert_eq!(v["op"], "format");
        let formatted = v["formatted"].as_str().unwrap();
        assert!(formatted.contains("RETURN") || formatted.contains("return"));
    }

    #[test]
    fn dispatch_rewrite_deferred() {
        let v = dispatch(r#"{"op":"rewrite","text":"RETURN 1","fix_ids":[]}"#);
        assert_eq!(v["op"], "rewrite");
        assert_eq!(v["resulting_text"], "RETURN 1");
        assert_eq!(v["deferred"], Value::Bool(true));
        assert!(
            v["deferred_reason"]
                .as_str()
                .is_some_and(|s| s.contains("v1") && s.contains("v2")),
            "rewrite deferred_reason must name both v1 and v2: got {}",
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
        let mut cache = FileCache::new();
        let bad = "not valid json {{{";
        let resp: Value = match serde_json::from_str::<AgentRequest>(bad) {
            Ok(req) => serde_json::to_value(handle(req, &mut schema, &mut db, &mut cache)).unwrap(),
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
        let mut cache = FileCache::new();
        let unknown = r#"{"op":"nonexistent","text":"x"}"#;
        let resp: Value = match serde_json::from_str::<AgentRequest>(unknown) {
            Ok(req) => serde_json::to_value(handle(req, &mut schema, &mut db, &mut cache)).unwrap(),
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
    // FileCache / intern_file tests (spec §15.X)
    // -----------------------------------------------------------------------

    /// Same source + same dialect → same `FileId` (cache hit).
    #[test]
    fn file_cache_same_source_same_dialect_same_id() {
        let mut db = Database::new();
        let mut cache = FileCache::new();
        let source = "MATCH (n) RETURN n".to_string();
        let id1 = intern_file(&mut db, &mut cache, source.clone(), Dialect::GqlAligned);
        let id2 = intern_file(&mut db, &mut cache, source, Dialect::GqlAligned);
        assert_eq!(id1, id2, "same source + dialect must reuse the same FileId");
        assert_eq!(cache.len(), 1, "only one entry in cache");
    }

    /// Same source + different dialect → different `FileId`.
    #[test]
    fn file_cache_same_source_different_dialect_different_id() {
        let mut db = Database::new();
        let mut cache = FileCache::new();
        let source = "MATCH (n) RETURN n".to_string();
        let id_gql = intern_file(&mut db, &mut cache, source.clone(), Dialect::GqlAligned);
        let id_oc = intern_file(&mut db, &mut cache, source, Dialect::OpenCypherV9);
        assert_ne!(
            id_gql, id_oc,
            "different dialects must produce different FileIds"
        );
        assert_eq!(cache.len(), 2, "two entries in cache");
    }

    /// After 65 unique sources the first entry is evicted (ceiling = 64).
    #[test]
    fn file_cache_evicts_at_ceiling() {
        let mut db = Database::new();
        let mut cache = FileCache::new();

        // Insert FILE_CACHE_CEILING unique sources.
        let mut first_key = 0u64;
        for i in 0..FILE_CACHE_CEILING {
            let src = format!("RETURN {i}");
            let key = cache_key(&src, Dialect::GqlAligned);
            if i == 0 {
                first_key = key;
            }
            intern_file(&mut db, &mut cache, src, Dialect::GqlAligned);
        }
        assert_eq!(cache.len(), FILE_CACHE_CEILING);
        // The first key is still LRU (it was inserted first and never re-accessed).
        assert_eq!(cache.lru_key(), Some(first_key));

        // Inserting one more unique entry triggers eviction of the first key.
        let extra = format!("RETURN {FILE_CACHE_CEILING}");
        intern_file(&mut db, &mut cache, extra, Dialect::GqlAligned);

        // Still at ceiling.
        assert_eq!(cache.len(), FILE_CACHE_CEILING);
        // First key was evicted.
        assert!(
            cache.lru_key() != Some(first_key),
            "first entry must have been evicted"
        );
    }
}
