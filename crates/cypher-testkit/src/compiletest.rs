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
//! Tests are invoked via `cargo test -p cypher-testkit`.  Each test
//! corresponds to a [`TestCase`].  [`run_corpus`] discovers all `.cypher`
//! files under a corpus directory and executes them.
//!
//! Regeneration: `cargo xtask bless` calls [`bless_corpus`] which writes
//! the actual output back over the expected sidecar files.
//!
//! # Status
//!
//! This is the v1 scaffold.  The runner stubs compile and lay down the
//! directory conventions; the actual invocation of pipeline passes lands
//! alongside the passes themselves (spec §17.6 defers corpus population to
//! later beads).

use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;

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
/// Constructed by [`discover_corpus`]; consumed by [`run_case`].
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

    /// Read the `.schema.json` sidecar as a JSON value, if it exists.
    ///
    /// Returns `None` when no schema sidecar is present (no-schema tests).
    ///
    /// # Errors
    /// Returns an error if the file exists but cannot be parsed.
    pub fn read_schema_json(&self) -> std::io::Result<Option<JsonValue>> {
        let p = self.schema_json_path();
        if p.exists() {
            let s = std::fs::read_to_string(&p)?;
            let v: JsonValue = serde_json::from_str(&s)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(Some(v))
        } else {
            Ok(None)
        }
    }
}

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
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("cypher") {
            cases.push(TestCase { input: path, kind });
        }
    }
    cases.sort_by(|a, b| a.input.cmp(&b.input));
    Ok(cases)
}

/// Outcome of running a single golden-file test case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// All sidecars matched.
    Pass,
    /// Byte-level mismatch; diff is in `details`.
    Fail { details: String },
    /// A sidecar file was expected but is missing.
    MissingSidecar { path: PathBuf },
}

/// Run a single golden-file [`TestCase`] against the front-end pipeline.
///
/// This is a stub that always returns [`Outcome::Pass`].  Real invocation
/// of the pipeline passes lands alongside the passes themselves (spec
/// §17.6; population is a follow-up bead).
#[must_use]
pub fn run_case(_case: &TestCase) -> Outcome {
    // TODO(spec §17.6): invoke db_with_source, collect diagnostics/plan/ast,
    // diff against sidecars, return Fail with pretty diff on mismatch.
    Outcome::Pass
}

/// Write actual pipeline output back over the expected sidecar files.
///
/// Called by `cargo xtask bless`.  This is a stub; real blessing lands
/// alongside the pipeline invocation above.
///
/// # Errors
/// Returns an error if any sidecar write fails.
pub fn bless_case(_case: &TestCase) -> std::io::Result<()> {
    // TODO(spec §17.6): write actual outputs to sidecar paths.
    Ok(())
}

/// Run the entire corpus for a given [`CorpusKind`].
///
/// Discovers all cases under `root/tests/ui/<kind>/`, runs each, collects
/// failures, and returns them.  An empty failure list means all cases
/// passed.
///
/// # Errors
/// Returns an error only if corpus discovery fails.
pub fn run_corpus(root: &Path, kind: CorpusKind) -> std::io::Result<Vec<(TestCase, Outcome)>> {
    let cases = discover_corpus(root, kind)?;
    let failures = cases
        .into_iter()
        .map(|c| {
            let outcome = run_case(&c);
            (c, outcome)
        })
        .filter(|(_, o)| *o != Outcome::Pass)
        .collect();
    Ok(failures)
}

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
        // No tests/ui/syntax subdir — should silently return empty.
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
}
