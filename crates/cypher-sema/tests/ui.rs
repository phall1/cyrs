//! Compiletest-style golden corpus for `cypher-sema` (spec 0001 §17.6).
//!
//! Discovers every `.cypher` file under `tests/ui/sema/`, runs the full
//! sema pipeline against it, renders all diagnostics with
//! [`cypher_diag::render_text_string`], and compares the output byte-exactly
//! against the companion `.stderr` sidecar.
//!
//! # Blessing
//!
//! When a sidecar is missing or differs from actual output, the test fails
//! with a diff and a hint:
//!
//! ```text
//! HINT: run `cargo xtask bless -p cypher-sema` to regenerate sidecars
//! ```
//!
//! Setting `BLESS=1` (or running `cargo xtask bless -p cypher-sema`) writes
//! the actual output back over the sidecar so the next run passes.
//!
//! # Schema sidecars
//!
//! If a `.cypher` file is accompanied by a `.schema.json` sidecar, that
//! JSON is parsed into a [`JsonSchema`] and passed to the schema-aware pass.
//! The JSON format is documented in [`JsonSchema`].
//!
//! # Dialect annotation
//!
//! A comment `// dialect: OpenCypherV9` anywhere in the `.cypher` source
//! selects the OpenCypher v9 dialect; the default is GqlAligned.

#![allow(clippy::too_many_lines)]
#![allow(clippy::doc_markdown)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cypher_diag::{DiagnosticsSink, render_text_string};
use cypher_hir::{desugar::desugar_statement, lower::lower_statement};
use cypher_schema::{
    EndpointDecl, FunctionSignature, ParamDecl, ProcedureSignature, PropertyDecl, PropertyType,
    ReturnTy, SchemaProvider, YieldDecl,
};
use cypher_sema::{DialectMode, SemaOptions, analyse};
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Discover and run all `.cypher` cases under `tests/ui/sema/`.
///
/// Each case is a separate `#[test]`-like check within the same test
/// function. Using a loop rather than `#[test]` per file avoids N test-binary
/// restarts and keeps output compact. The entire suite fails at the end if
/// any case failed, collecting all diffs.
#[test]
fn ui_sema_corpus() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus_dir = manifest.join("tests").join("ui").join("sema");

    if !corpus_dir.exists() {
        return; // directory absent → corpus is empty, nothing to run
    }

    let bless = std::env::var("BLESS").is_ok_and(|v| v == "1");

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&corpus_dir)
        .expect("cannot read corpus dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("cypher"))
        .collect();
    entries.sort();

    let mut failures: Vec<String> = Vec::new();

    for cypher_path in &entries {
        let result = run_case(cypher_path, bless);
        if let Err(msg) = result {
            failures.push(msg);
        }
    }

    if !failures.is_empty() {
        let joined = failures.join("\n\n---\n\n");
        panic!(
            "{} UI test(s) failed:\n\n{}\n\nHINT: run `cargo xtask bless -p cypher-sema` \
             to regenerate sidecars",
            failures.len(),
            joined
        );
    }
}

// ---------------------------------------------------------------------------
// Per-case runner
// ---------------------------------------------------------------------------

fn run_case(cypher_path: &Path, bless: bool) -> Result<(), String> {
    let source = std::fs::read_to_string(cypher_path)
        .map_err(|e| format!("{}: cannot read: {e}", cypher_path.display()))?;

    // Determine dialect from source comment.
    let dialect = if source.contains("// dialect: OpenCypherV9") {
        DialectMode::OpenCypherV9
    } else {
        DialectMode::GqlAligned
    };

    // Load optional schema sidecar.
    let schema_path = cypher_path.with_extension("schema.json");
    let json_schema: Option<JsonSchema> = if schema_path.exists() {
        let raw = std::fs::read_to_string(&schema_path)
            .map_err(|e| format!("{}: cannot read schema: {e}", schema_path.display()))?;
        let js: JsonSchema = serde_json::from_str(&raw)
            .map_err(|e| format!("{}: invalid schema JSON: {e}", schema_path.display()))?;
        Some(js)
    } else {
        None
    };

    // Run pipeline.
    let stmt = lower_statement(&source);
    let stmt = desugar_statement(stmt);
    let mut sink = DiagnosticsSink::new();
    let opts = SemaOptions {
        dialect,
        warn_shadowing: true,
        ..Default::default()
    };

    let file_name = cypher_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown.cypher");

    if let Some(ref js) = json_schema {
        analyse(&stmt, Some(js as &dyn SchemaProvider), &opts, &mut sink);
    } else {
        analyse(&stmt, None, &opts, &mut sink);
    }

    let diags = sink.into_sorted();

    // Render all diagnostics into a single string.
    let actual: String = if diags.is_empty() {
        String::new()
    } else {
        diags
            .iter()
            .map(|d| render_text_string(file_name, &source, d))
            .collect::<String>()
    };

    // Compare/bless the `.stderr` sidecar.
    let stderr_path = cypher_path.with_extension("stderr");
    compare_or_bless(&stderr_path, &actual, bless, cypher_path)
}

/// Compare `actual` against the sidecar at `path`. If `bless`, write instead.
fn compare_or_bless(
    sidecar: &Path,
    actual: &str,
    bless: bool,
    case_path: &Path,
) -> Result<(), String> {
    if bless {
        std::fs::write(sidecar, actual)
            .map_err(|e| format!("{}: bless write failed: {e}", sidecar.display()))?;
        return Ok(());
    }

    if !sidecar.exists() {
        return Err(format!(
            "{}: missing sidecar `{}`\n  actual output:\n{}",
            case_path.display(),
            sidecar.display(),
            indent(actual),
        ));
    }

    let expected = std::fs::read_to_string(sidecar)
        .map_err(|e| format!("{}: cannot read sidecar: {e}", sidecar.display()))?;

    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{}: sidecar mismatch\n{}",
            case_path.display(),
            unified_diff(&expected, actual),
        ))
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn unified_diff(expected: &str, actual: &str) -> String {
    // Simple line-by-line diff — avoids a heavy dep.
    let exp_lines: Vec<&str> = expected.lines().collect();
    let act_lines: Vec<&str> = actual.lines().collect();

    let mut out = String::from("--- expected\n+++ actual\n");
    let max = exp_lines.len().max(act_lines.len());
    for i in 0..max {
        match (exp_lines.get(i), act_lines.get(i)) {
            (Some(e), Some(a)) if e == a => {
                out.push(' ');
                out.push_str(e);
                out.push('\n');
            }
            (Some(e), Some(a)) => {
                out.push('-');
                out.push_str(e);
                out.push('\n');
                out.push('+');
                out.push_str(a);
                out.push('\n');
            }
            (Some(e), None) => {
                out.push('-');
                out.push_str(e);
                out.push('\n');
            }
            (None, Some(a)) => {
                out.push('+');
                out.push_str(a);
                out.push('\n');
            }
            (None, None) => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// JsonSchema — minimal SchemaProvider loaded from a `.schema.json` sidecar.
//
// JSON shape:
// {
//   "labels": ["Person", "Movie"],
//   "rel_types": ["ACTED_IN", "KNOWS"],
//   "node_properties": {
//     "Person": [{"name": "age", "type": "Int", "required": false}]
//   },
//   "rel_properties": {
//     "KNOWS": [{"name": "since", "type": "Int", "required": false}]
//   },
//   "rel_endpoints": {
//     "KNOWS": [{"source": "Person", "target": "Person"}]
//   },
//   "functions": [
//     {"name": "my_fn", "params": ["x"], "return_type": "Any", "variadic": null}
//   ],
//   "procedures": [
//     {"name": "db.labels", "params": [], "yield_columns": ["label"]}
//   ]
// }
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct JsonSchema {
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    rel_types: Vec<String>,
    #[serde(default)]
    node_properties: BTreeMap<String, Vec<JsonPropDecl>>,
    #[serde(default)]
    rel_properties: BTreeMap<String, Vec<JsonPropDecl>>,
    #[serde(default)]
    rel_endpoints: BTreeMap<String, Vec<JsonEndpoint>>,
    #[serde(default)]
    functions: Vec<JsonFnDecl>,
    #[serde(default)]
    procedures: Vec<JsonProcDecl>,
}

#[derive(Debug, serde::Deserialize)]
struct JsonPropDecl {
    name: String,
    #[serde(rename = "type", default = "default_prop_type")]
    ty: String,
    #[serde(default)]
    required: bool,
}

fn default_prop_type() -> String {
    "Any".to_string()
}

#[derive(Debug, serde::Deserialize)]
struct JsonEndpoint {
    source: String,
    target: String,
}

#[derive(Debug, serde::Deserialize)]
struct JsonFnDecl {
    name: String,
    #[serde(default)]
    params: Vec<String>,
    #[serde(default = "default_return_any")]
    return_type: String,
    variadic: Option<String>,
}

fn default_return_any() -> String {
    "Any".to_string()
}

#[derive(Debug, serde::Deserialize)]
struct JsonProcDecl {
    name: String,
    #[serde(default)]
    params: Vec<String>,
    #[serde(default)]
    yield_columns: Vec<String>,
}

fn parse_prop_type(s: &str) -> PropertyType {
    match s {
        "String" => PropertyType::String,
        "Int" => PropertyType::Int,
        "Float" => PropertyType::Float,
        "Bool" => PropertyType::Bool,
        "Date" => PropertyType::Date,
        "Datetime" => PropertyType::Datetime,
        _ => PropertyType::Any,
    }
}

impl SchemaProvider for JsonSchema {
    fn labels(&self) -> Vec<SmolStr> {
        self.labels.iter().map(SmolStr::new).collect()
    }

    fn relationship_types(&self) -> Vec<SmolStr> {
        self.rel_types.iter().map(SmolStr::new).collect()
    }

    fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>> {
        if !self.labels.iter().any(|l| l == label) {
            return None;
        }
        let props = self.node_properties.get(label).map_or_else(Vec::new, |ps| {
            ps.iter()
                .map(|p| PropertyDecl {
                    name: SmolStr::new(&p.name),
                    ty: parse_prop_type(&p.ty),
                    required: p.required,
                })
                .collect()
        });
        Some(props)
    }

    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<PropertyDecl>> {
        if !self.rel_types.iter().any(|r| r == rel_type) {
            return None;
        }
        let props = self
            .rel_properties
            .get(rel_type)
            .map_or_else(Vec::new, |ps| {
                ps.iter()
                    .map(|p| PropertyDecl {
                        name: SmolStr::new(&p.name),
                        ty: parse_prop_type(&p.ty),
                        required: p.required,
                    })
                    .collect()
            });
        Some(props)
    }

    fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl> {
        self.rel_endpoints
            .get(rel_type)
            .map(|eps| {
                eps.iter()
                    .map(|e| EndpointDecl {
                        from: SmolStr::new(&e.source),
                        to: SmolStr::new(&e.target),
                        cardinality: cypher_schema::Cardinality::ManyToMany,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn inverse_of(&self, _rel_type: &str) -> Option<SmolStr> {
        None
    }

    fn function(&self, name: &str) -> Option<FunctionSignature> {
        self.functions.iter().find(|f| f.name == name).map(|f| {
            let params: Vec<ParamDecl> = f
                .params
                .iter()
                .map(|p| ParamDecl {
                    name: SmolStr::new(p),
                    ty: PropertyType::Any,
                    default: None,
                })
                .collect();
            let variadic = f.variadic.as_ref().map(|_| ParamDecl {
                name: SmolStr::new("args"),
                ty: PropertyType::Any,
                default: None,
            });
            FunctionSignature {
                name: SmolStr::new(&f.name),
                params,
                variadic,
                return_ty: ReturnTy::Constant(parse_prop_type(&f.return_type)),
                categories: cypher_schema::FnCategories {
                    pure: true,
                    aggregate: false,
                    deterministic: true,
                },
            }
        })
    }

    fn procedure(&self, name: &str) -> Option<ProcedureSignature> {
        self.procedures.iter().find(|p| p.name == name).map(|p| {
            let params: Vec<ParamDecl> = p
                .params
                .iter()
                .map(|param| ParamDecl {
                    name: SmolStr::new(param),
                    ty: PropertyType::Any,
                    default: None,
                })
                .collect();
            let yields: Vec<YieldDecl> = p
                .yield_columns
                .iter()
                .map(|col| YieldDecl {
                    name: SmolStr::new(col),
                    ty: PropertyType::Any,
                })
                .collect();
            ProcedureSignature {
                name: SmolStr::new(&p.name),
                params,
                yields,
                mode: cypher_schema::ProcMode::Read,
            }
        })
    }

    fn schema_digest(&self) -> [u8; 32] {
        // Content-address by serialising labels + rel_types. Not cryptographic;
        // just needs to change when the schema changes.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.labels.hash(&mut h);
        self.rel_types.hash(&mut h);
        let v = h.finish();
        let mut digest = [0u8; 32];
        digest[..8].copy_from_slice(&v.to_le_bytes());
        digest
    }
}
