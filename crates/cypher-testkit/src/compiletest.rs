//! Golden compiletest runner. Spec 0001 §17.6.
//!
//! A `rustc`-style golden-file runner.  Each test case is an input
//! `.cypher` file under one of the corpus directories listed below, paired
//! with expected output files produced by the front-end pipeline.
//!
//! # Corpus layout (`tests/ui/`)
//!
//! ```text
//! tests/ui/
//!   syntax/   — parser error-recovery & diagnostics (.cypher + .stderr)
//!   sema/     — semantic-analysis diagnostics (.cypher + .stderr)
//!   schema/   — schema-aware checks (.cypher + .schema.json + .stderr)
//!   dialect/  — dialect-mode differences (.cypher + .stderr)
//!   plan/     — plan-lowering output (.cypher + .plan.json)
//!   fmt/      — formatter round-trips (.cypher + .formatted.cypher)
//! ```
//!
//! Companion sidecar files:
//! - `.stderr`           — rendered diagnostic output (byte-exact match)
//! - `.ast.txt`          — pretty-printed AST (optional)
//! - `.hir.txt`          — pretty-printed HIR (optional)
//! - `.plan.json`        — serialised logical plan (optional)
//! - `.formatted.cypher` — formatter output (optional, used in `fmt/`)
//! - `.schema.json`      — schema fixture loaded before analysis (optional)
//!
//! # Running
//!
//! Tests are invoked via `cargo test`.  Each crate that owns a corpus
//! calls [`run_ui_corpus`] from its `tests/ui.rs` integration test.
//! Alternatively call [`run_corpus`] directly with a `Path` root.
//!
//! Regeneration: `cargo xtask bless` or `BLESS=1 cargo test --test ui`
//! writes the actual output back over the expected sidecar files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cypher_diag::{DiagnosticsSink, render_text_string};
use cypher_fmt::format;
use cypher_hir::{desugar::desugar_statement, lower::lower_statement};
use cypher_plan::{lower::lower_statement as plan_lower, pretty::pretty};
use cypher_schema::{
    Cardinality, EndpointDecl, FnCategories, FunctionSignature, ParamDecl, ProcMode,
    ProcedureSignature, PropertyDecl, PropertyType, ReturnTy, SchemaProvider, YieldDecl,
};
use cypher_sema::{DialectMode, SemaOptions, analyse};
use cypher_syntax::parse;
use smol_str::SmolStr;

/// Identifies which corpus sub-directory a test belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CorpusKind {
    /// `tests/ui/syntax/` — parser diagnostics.
    Syntax,
    /// `tests/ui/sema/` — semantic-analysis diagnostics.
    Sema,
    /// `tests/ui/schema/` — schema-aware checks.
    Schema,
    /// `tests/ui/dialect/` — dialect-mode differences.
    Dialect,
    /// `tests/ui/plan/` — plan-lowering output.
    Plan,
    /// `tests/ui/fmt/` — formatter round-trips.
    Fmt,
}

impl CorpusKind {
    /// Directory name relative to the `tests/ui/` root.
    #[must_use]
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Sema => "sema",
            Self::Schema => "schema",
            Self::Dialect => "dialect",
            Self::Plan => "plan",
            Self::Fmt => "fmt",
        }
    }
}

impl std::fmt::Display for CorpusKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.dir_name())
    }
}

/// A single golden-file test case.
///
/// Constructed by [`discover_corpus`]; consumed by per-kind driver functions.
#[derive(Debug, Clone)]
pub struct TestCase {
    /// Absolute path to the `.cypher` input file.
    pub input: PathBuf,
    /// Which corpus this case belongs to.
    pub kind: CorpusKind,
}

impl TestCase {
    /// Path to the companion `.stderr` sidecar (expected diagnostic output).
    #[must_use]
    pub fn stderr_path(&self) -> PathBuf {
        self.input.with_extension("stderr")
    }

    /// Path to the companion `.ast.txt` sidecar (optional).
    #[must_use]
    pub fn ast_path(&self) -> PathBuf {
        self.input.with_extension("ast.txt")
    }

    /// Path to the companion `.hir.txt` sidecar (optional).
    #[must_use]
    pub fn hir_path(&self) -> PathBuf {
        self.input.with_extension("hir.txt")
    }

    /// Path to the companion `.plan.json` sidecar (optional).
    #[must_use]
    pub fn plan_json_path(&self) -> PathBuf {
        self.input.with_extension("plan.json")
    }

    /// Path to the companion `.formatted.cypher` sidecar (optional).
    #[must_use]
    pub fn formatted_path(&self) -> PathBuf {
        self.input.with_extension("formatted.cypher")
    }

    /// Path to the companion `.schema.json` sidecar (optional).
    #[must_use]
    pub fn schema_json_path(&self) -> PathBuf {
        self.input.with_extension("schema.json")
    }

    /// Read the `.cypher` source text.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read.
    pub fn read_source(&self) -> std::io::Result<String> {
        std::fs::read_to_string(&self.input)
    }

    /// Read the `.schema.json` sidecar into a [`JsonSchema`] if it exists.
    ///
    /// Returns `None` when no schema sidecar is present (no-schema tests).
    ///
    /// # Errors
    /// Returns an error if the file exists but cannot be parsed.
    pub fn read_schema_json(&self) -> std::io::Result<Option<JsonSchema>> {
        let p = self.schema_json_path();
        if p.exists() {
            let s = std::fs::read_to_string(&p)?;
            let v: JsonSchema = serde_json::from_str(&s)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(Some(v))
        } else {
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// Discover
// ---------------------------------------------------------------------------

/// Discover all `.cypher` files under `root/tests/ui/<kind>/`.
///
/// Returns an empty vec (not an error) if the directory does not yet exist
/// so that the gate passes before corpus files are added.
///
/// # Errors
/// Returns an error only if the directory exists but cannot be read.
pub fn discover_corpus(root: &Path, kind: CorpusKind) -> std::io::Result<Vec<TestCase>> {
    let dir = root.join("tests").join("ui").join(kind.dir_name());
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut cases = Vec::new();
    collect_cypher_files(&dir, kind, &mut cases)?;
    cases.sort_by(|a, b| a.input.cmp(&b.input));
    Ok(cases)
}

fn collect_cypher_files(
    dir: &Path,
    kind: CorpusKind,
    out: &mut Vec<TestCase>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_cypher_files(&path, kind, out)?;
        } else if is_input_cypher(&path) {
            out.push(TestCase { input: path, kind });
        }
    }
    Ok(())
}

/// Returns `true` iff `path` is a primary `.cypher` input (not a sidecar).
///
/// Sidecars like `.formatted.cypher` also end in `.cypher` but their file
/// stem ends in `.formatted`, making them companion files rather than inputs.
fn is_input_cypher(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    // Must end in .cypher but NOT in .formatted.cypher (a sidecar).
    name.ends_with(".cypher") && !name.ends_with(".formatted.cypher")
}

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// Outcome of running a single golden-file test case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// All sidecars matched (or were blessed).
    Pass,
    /// Byte-level mismatch; diff is in `details`.
    Fail { details: String },
    /// A sidecar file was expected but is missing.
    MissingSidecar { path: PathBuf },
}

// ---------------------------------------------------------------------------
// Per-kind drivers
// ---------------------------------------------------------------------------

/// Run a syntax corpus case.
///
/// Invokes `cypher_syntax::parse`, formats syntax errors as diagnostics,
/// and compares against `.stderr`.
///
/// # Errors
/// Returns an error if the input file or sidecar cannot be read/written.
pub fn run_syntax_case(case: &TestCase, bless: bool) -> std::io::Result<Outcome> {
    let source = case.read_source()?;
    let file_name = file_name(case);

    let parsed = parse(&source);
    let actual: String = parsed.errors().iter().fold(String::new(), |mut acc, e| {
        use std::fmt::Write as _;
        let _ = write!(
            acc,
            "error[E{:04}]: {}\n --> {}:{}\n\n",
            e.code,
            e.message,
            file_name,
            u32::from(e.offset)
        );
        acc
    });

    compare_or_bless(&case.stderr_path(), &actual, bless, &case.input)
}

/// Run a sema corpus case.
///
/// Invokes `cypher_hir::lower_statement + desugar_statement` then
/// `cypher_sema::analyse`, renders all diagnostics, and compares against
/// `.stderr`.  Respects `// dialect: OpenCypherV9` source comments and
/// optional `.schema.json` sidecars.
///
/// # Errors
/// Returns an error if the input file or sidecar cannot be read/written.
pub fn run_sema_case(case: &TestCase, bless: bool) -> std::io::Result<Outcome> {
    let source = case.read_source()?;
    let file_name = file_name(case);

    let dialect = if source.contains("// dialect: OpenCypherV9") {
        DialectMode::OpenCypherV9
    } else {
        DialectMode::GqlAligned
    };

    let json_schema = case.read_schema_json()?;

    let stmt = lower_statement(&source);
    let stmt = desugar_statement(stmt);
    let mut sink = DiagnosticsSink::new();
    let opts = SemaOptions {
        dialect,
        warn_shadowing: true,
        ..Default::default()
    };

    if let Some(ref js) = json_schema {
        analyse(&stmt, Some(js as &dyn SchemaProvider), &opts, &mut sink);
    } else {
        analyse(&stmt, None, &opts, &mut sink);
    }

    let diags = sink.into_sorted();

    let actual: String = if diags.is_empty() {
        String::new()
    } else {
        diags
            .iter()
            .map(|d| render_text_string(file_name, &source, d))
            .collect::<String>()
    };

    compare_or_bless(&case.stderr_path(), &actual, bless, &case.input)
}

/// Run a schema corpus case.
///
/// Schema tests are sema tests with a required `.schema.json` sidecar.
/// Delegates to [`run_sema_case`] (the schema is loaded automatically via
/// the `.schema.json` companion).
///
/// # Errors
/// Returns an error if the input file or sidecar cannot be read/written.
pub fn run_schema_case(case: &TestCase, bless: bool) -> std::io::Result<Outcome> {
    // Schema cases are sema cases that happen to have a .schema.json sidecar.
    run_sema_case(case, bless)
}

/// Run a dialect corpus case.
///
/// Dialect tests are sema tests annotated with `// dialect: ...`.
/// Delegates to [`run_sema_case`].
///
/// # Errors
/// Returns an error if the input file or sidecar cannot be read/written.
pub fn run_dialect_case(case: &TestCase, bless: bool) -> std::io::Result<Outcome> {
    run_sema_case(case, bless)
}

/// Run a plan corpus case.
///
/// Invokes the full HIR pipeline then `cypher_plan::lower_statement`, renders
/// the plan as text, and compares against `.stderr`.
///
/// # Errors
/// Returns an error if the input file or sidecar cannot be read/written.
pub fn run_plan_case(case: &TestCase, bless: bool) -> std::io::Result<Outcome> {
    let source = case.read_source()?;

    let hir_stmt = lower_statement(&source);
    let hir_stmt = desugar_statement(hir_stmt);
    let plan_stmt = plan_lower(&hir_stmt);
    let actual = pretty(&plan_stmt);

    compare_or_bless(&case.stderr_path(), &actual, bless, &case.input)
}

/// Run a fmt corpus case.
///
/// Invokes `cypher_fmt::format` and compares the result against
/// `.formatted.cypher`.
///
/// # Errors
/// Returns an error if the input file or sidecar cannot be read/written.
pub fn run_fmt_case(case: &TestCase, bless: bool) -> std::io::Result<Outcome> {
    let source = case.read_source()?;
    let actual = format(&source);

    let sidecar = case.formatted_path();
    compare_or_bless(&sidecar, &actual, bless, &case.input)
}

// ---------------------------------------------------------------------------
// run_corpus — the main entry point used by each crate's tests/ui.rs
// ---------------------------------------------------------------------------

/// Run the entire corpus for a given [`CorpusKind`] rooted at `crate_root`.
///
/// `crate_root` should be `Path::new(env!("CARGO_MANIFEST_DIR"))` from the
/// calling crate's test harness.  The `crate_name` argument is used only in
/// the bless-hint message.
///
/// Reads the `BLESS` environment variable; if set to `"1"` every sidecar is
/// overwritten with the actual output instead of compared.
///
/// Panics at the end of the run if any cases failed, with a collected diff
/// report and a bless hint.
pub fn run_ui_corpus(kind: CorpusKind, crate_root: &Path, crate_name: &str) {
    let bless = std::env::var("BLESS").is_ok_and(|v| v == "1");

    let cases = discover_corpus(crate_root, kind).expect("corpus discovery failed");
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let result = run_case(case, bless);
        match result {
            Ok(Outcome::Pass) => {}
            Ok(Outcome::Fail { details }) => {
                failures.push(format!("{}: mismatch\n{}", case.input.display(), details));
            }
            Ok(Outcome::MissingSidecar { path }) => {
                failures.push(format!(
                    "{}: missing sidecar `{}`",
                    case.input.display(),
                    path.display()
                ));
            }
            Err(e) => {
                failures.push(format!("{}: I/O error: {e}", case.input.display()));
            }
        }
    }

    if !failures.is_empty() {
        let joined = failures.join("\n\n---\n\n");
        panic!(
            "{} UI test(s) failed:\n\n{}\n\nHINT: run `cargo xtask bless -p {crate_name}` \
             to regenerate sidecars",
            failures.len(),
            joined
        );
    }
}

/// Run a single [`TestCase`], dispatching to the appropriate per-kind driver.
///
/// # Errors
/// Returns an I/O error if the input or sidecar cannot be read/written.
pub fn run_case(case: &TestCase, bless: bool) -> std::io::Result<Outcome> {
    match case.kind {
        CorpusKind::Syntax => run_syntax_case(case, bless),
        CorpusKind::Sema => run_sema_case(case, bless),
        CorpusKind::Schema => run_schema_case(case, bless),
        CorpusKind::Dialect => run_dialect_case(case, bless),
        CorpusKind::Plan => run_plan_case(case, bless),
        CorpusKind::Fmt => run_fmt_case(case, bless),
    }
}

/// Run the entire corpus for a given [`CorpusKind`] and return failures.
///
/// An empty failure list means all cases passed.
/// Reads the `BLESS` env var automatically.
///
/// # Errors
/// Returns an error only if corpus discovery fails.
pub fn run_corpus(root: &Path, kind: CorpusKind) -> std::io::Result<Vec<(TestCase, Outcome)>> {
    let bless = std::env::var("BLESS").is_ok_and(|v| v == "1");
    let cases = discover_corpus(root, kind)?;
    let mut failures = Vec::new();
    for c in cases {
        let outcome = run_case(&c, bless)?;
        if outcome != Outcome::Pass {
            failures.push((c, outcome));
        }
    }
    Ok(failures)
}

// ---------------------------------------------------------------------------
// Bless a single case — write actual output over the sidecar.
/// Write actual pipeline output back over the expected sidecar files.
///
/// Called by `cargo xtask bless`.
///
/// # Errors
/// Returns an error if any sidecar write fails.
pub fn bless_case(case: &TestCase) -> std::io::Result<()> {
    run_case(case, true)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn file_name(case: &TestCase) -> &str {
    case.input
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown.cypher")
}

fn compare_or_bless(
    sidecar: &Path,
    actual: &str,
    bless: bool,
    case_path: &Path,
) -> std::io::Result<Outcome> {
    if bless {
        std::fs::write(sidecar, actual)?;
        return Ok(Outcome::Pass);
    }

    if !sidecar.exists() {
        return Ok(Outcome::MissingSidecar {
            path: sidecar.to_path_buf(),
        });
    }

    let expected = std::fs::read_to_string(sidecar)?;
    if actual == expected {
        Ok(Outcome::Pass)
    } else {
        let diff = unified_diff(&expected, actual);
        let details = format!("{}\n{}", case_path.display(), diff);
        Ok(Outcome::Fail { details })
    }
}

fn unified_diff(expected: &str, actual: &str) -> String {
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

/// A minimal [`SchemaProvider`] loaded from a `.schema.json` sidecar file.
///
/// Used by [`run_sema_case`] and [`run_schema_case`] when a `.schema.json`
/// companion exists next to the `.cypher` input.
#[derive(Debug, serde::Deserialize)]
pub struct JsonSchema {
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
                        cardinality: Cardinality::ManyToMany,
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
                categories: FnCategories {
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
                mode: ProcMode::Read,
            }
        })
    }

    fn schema_digest(&self) -> [u8; 32] {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discover_empty_dir_returns_empty_vec() {
        let tmp = TempDir::new().unwrap();
        let cases = discover_corpus(tmp.path(), CorpusKind::Syntax).unwrap();
        assert!(cases.is_empty());
    }

    #[test]
    fn discover_missing_dir_returns_empty_vec() {
        let tmp = TempDir::new().unwrap();
        let cases = discover_corpus(tmp.path(), CorpusKind::Fmt).unwrap();
        assert!(cases.is_empty());
    }

    #[test]
    fn discover_finds_cypher_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("tests").join("ui").join("syntax");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("basic.cypher"), "MATCH (n) RETURN n").unwrap();
        std::fs::write(dir.join("unrelated.txt"), "ignored").unwrap();

        let cases = discover_corpus(tmp.path(), CorpusKind::Syntax).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].input.file_name().unwrap(), "basic.cypher");
    }

    #[test]
    fn run_corpus_no_failures_on_empty() {
        let tmp = TempDir::new().unwrap();
        let failures = run_corpus(tmp.path(), CorpusKind::Sema).unwrap();
        assert!(failures.is_empty());
    }

    #[test]
    fn corpus_kind_dir_names() {
        assert_eq!(CorpusKind::Syntax.dir_name(), "syntax");
        assert_eq!(CorpusKind::Sema.dir_name(), "sema");
        assert_eq!(CorpusKind::Schema.dir_name(), "schema");
        assert_eq!(CorpusKind::Dialect.dir_name(), "dialect");
        assert_eq!(CorpusKind::Plan.dir_name(), "plan");
        assert_eq!(CorpusKind::Fmt.dir_name(), "fmt");
    }

    #[test]
    fn run_syntax_case_clean_input_produces_empty_stderr() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("tests").join("ui").join("syntax");
        std::fs::create_dir_all(&dir).unwrap();
        let cypher = dir.join("clean.cypher");
        let stderr = dir.join("clean.stderr");
        std::fs::write(&cypher, "MATCH (n) RETURN n").unwrap();
        std::fs::write(&stderr, "").unwrap();

        let case = TestCase {
            input: cypher,
            kind: CorpusKind::Syntax,
        };
        let outcome = run_syntax_case(&case, false).unwrap();
        assert_eq!(outcome, Outcome::Pass);
    }

    #[test]
    fn run_fmt_case_pass_when_already_formatted() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("tests").join("ui").join("fmt");
        std::fs::create_dir_all(&dir).unwrap();
        let cypher = dir.join("clean.cypher");
        std::fs::write(&cypher, "MATCH (n) RETURN n").unwrap();
        let case = TestCase {
            input: cypher,
            kind: CorpusKind::Fmt,
        };
        // Bless first to create the sidecar.
        run_fmt_case(&case, true).unwrap();
        // Now compare.
        let outcome = run_fmt_case(&case, false).unwrap();
        assert_eq!(outcome, Outcome::Pass);
    }
}
