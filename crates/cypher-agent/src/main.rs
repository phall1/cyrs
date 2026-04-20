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
//! | `complete`     | `text`, `offset`, `dialect?`            | `items` (stub: `[]`)                   |
//! | `hover`        | `text`, `offset`, `dialect?`            | `markdown`, `range` (stub: empty)      |
//! | `format`       | `text`                                  | `formatted`                            |
//! | `rewrite`      | `text`, `fix_ids`                       | `applied_edits`, `resulting_text`      |
//! | `plan`         | `text`, `dialect?`                      | `plan_json`                            |
//! | `explain`      | `text`, `dialect?`                      | `markdown`                             |
//! | `schema_set`   | `schema_json`                           | `ok: true`                             |
//! | `schema_clear` | —                                       | `ok: true`                             |
//! | `shutdown`     | —                                       | (exits loop)                           |

#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use cypher_db::{Database, DialectMode};
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
    /// complete response (stub)
    Complete { items: Vec<Value> },
    /// hover response (stub)
    Hover { markdown: String, range: [u32; 2] },
    /// format response
    Format { formatted: String },
    /// rewrite response (stub)
    Rewrite {
        applied_edits: Vec<Value>,
        resulting_text: String,
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

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<AgentRequest>(&line) {
            Ok(req) => handle(req, &mut session_schema),
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

fn handle(
    req: AgentRequest,
    session_schema: &mut Option<Arc<dyn SchemaProvider>>,
) -> AgentResponse {
    match req {
        // ------------------------------------------------------------------
        // parse: CST pretty-print + syntax errors
        // ------------------------------------------------------------------
        AgentRequest::Parse { text, dialect } => {
            let mut db = Database::new();
            let id = db.open_file(Path::new("_"), text, dialect.into());
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
            let mut db = Database::new();
            if let Some(schema) = session_schema.clone() {
                db.set_schema(Some(schema));
            }
            let id = db.open_file(Path::new("_"), text, dialect.into());
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
        // complete: stub (v1 — no completion engine yet)
        // ------------------------------------------------------------------
        AgentRequest::Complete {
            text: _,
            offset: _,
            dialect: _,
        } => AgentResponse::Complete { items: vec![] },

        // ------------------------------------------------------------------
        // hover: stub (v1 — no hover engine yet)
        // ------------------------------------------------------------------
        AgentRequest::Hover {
            text: _,
            offset: _,
            dialect: _,
        } => AgentResponse::Hover {
            markdown: String::new(),
            range: [0, 0],
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
        // rewrite: stub (v1 — no fix application engine yet)
        // ------------------------------------------------------------------
        AgentRequest::Rewrite { text, fix_ids: _ } => AgentResponse::Rewrite {
            applied_edits: vec![],
            resulting_text: text,
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
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        let resp = handle(req, &mut schema);
        serde_json::to_value(resp).unwrap()
    }

    fn dispatch_with_schema(json: &str, schema: &mut Option<Arc<dyn SchemaProvider>>) -> Value {
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        let resp = handle(req, schema);
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
    fn dispatch_complete_stub() {
        let v = dispatch(r#"{"op":"complete","text":"RETURN 1","offset":3}"#);
        assert_eq!(v["op"], "complete");
        assert_eq!(v["items"], Value::Array(vec![]));
    }

    #[test]
    fn dispatch_hover_stub() {
        let v = dispatch(r#"{"op":"hover","text":"RETURN 1","offset":3}"#);
        assert_eq!(v["op"], "hover");
        assert!(v["markdown"].is_string());
    }

    #[test]
    fn dispatch_format_ok() {
        let v = dispatch(r#"{"op":"format","text":"return 1"}"#);
        assert_eq!(v["op"], "format");
        let formatted = v["formatted"].as_str().unwrap();
        assert!(formatted.contains("RETURN") || formatted.contains("return"));
    }

    #[test]
    fn dispatch_rewrite_stub() {
        let v = dispatch(r#"{"op":"rewrite","text":"RETURN 1","fix_ids":[]}"#);
        assert_eq!(v["op"], "rewrite");
        assert_eq!(v["resulting_text"], "RETURN 1");
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
        let bad = "not valid json {{{";
        let resp: Value = match serde_json::from_str::<AgentRequest>(bad) {
            Ok(req) => serde_json::to_value(handle(req, &mut schema)).unwrap(),
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
        let unknown = r#"{"op":"nonexistent","text":"x"}"#;
        let resp: Value = match serde_json::from_str::<AgentRequest>(unknown) {
            Ok(req) => serde_json::to_value(handle(req, &mut schema)).unwrap(),
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
}
