//! TCK harness — spec 0001 §17.5.
//!
//! Reads scenarios from `tck/v1.toml` and `tck/embedder-m23.toml`,
//! filters to those carrying at least one of the corresponding tag set
//! (per [`cypher_tck::v1_tags`] / [`cypher_tck::embedder_m23_tag`]),
//! then runs each through `cypher_db::Database` and asserts the
//! expected parse outcome.
//!
//! Run with:
//!   cargo test -p cyrs-tck
//!
//! Tags covered: @MATCH, @OPTIONAL-MATCH, @WHERE, @RETURN, @WITH, @UNWIND,
//! @CREATE, @MERGE, @SET, @REMOVE, @DELETE, @EXPRESSIONS, @AGGREGATIONS,
//! @STRINGS, @LISTS, @MAPS, @PATTERNS, @NULL  (v1 supported)
//! @CALL-SUBQUERY, @LOAD-CSV  (must emit a parse error)
//! @M23                       (embedder M23 curated subset, bead cy-emb6)
//!
//! The full vendored upstream TCK is exercised by a separate test gated
//! behind `--features full-tck`; see `tests/full.rs`.

use std::path::Path;

use cypher_db::{DialectMode, workspace::Database};
use cypher_tck::{Expected, embedder_m23_tag, v1_tags};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Fixture types (mirrors tck/v1.toml structure)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Fixtures {
    scenario: Vec<Scenario>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    tags: Vec<String>,
    query: String,
    outcome: Outcome,
    /// When `true` the scenario is skipped (counted as ignored) rather than
    /// run.  Use this to acknowledge a known parser bug without failing the
    /// harness.  Always pair with a `note` explaining which bead to file.
    #[serde(default)]
    ignore: bool,
    /// Human-readable note on why the scenario is ignored / which bead to
    /// file.
    #[allow(dead_code)]
    note: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Ok,
    Error,
}

impl Scenario {
    /// Map the on-disk `outcome` / `ignore` fields to the library's
    /// per-scenario [`Expected`] outcome (bead cy-p5q).  The v1 TOML
    /// format is kept stable for backward-compatibility with
    /// `xtask tree-sitter-parity`; this mapping happens at load time.
    fn expected(&self) -> Expected {
        if self.ignore {
            Expected::Ignored
        } else {
            match self.outcome {
                Outcome::Ok => Expected::Supported,
                Outcome::Error => Expected::Error,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the set of v1 tags (with leading `@`).
fn v1_tag_set() -> std::collections::HashSet<&'static str> {
    v1_tags().iter().copied().collect()
}

/// Parse a query with `Database` and return `true` if it had no parse errors.
fn parse_ok(query: &str) -> bool {
    let mut db = Database::new();
    let id = db.open_file(
        Path::new("tck.cyp"),
        query.to_owned(),
        DialectMode::GqlAligned,
    );
    let out = db.parse_cst(id).expect("file must be open");
    out.parse().errors().is_empty()
}

// ---------------------------------------------------------------------------
// Harness entry point
// ---------------------------------------------------------------------------

/// Run a single fixture file under the given tag-filter and return
/// any FAIL lines.  Empty result → all matched scenarios passed.
///
/// `tag_filter` is the closure used to decide whether a scenario's
/// tag set qualifies it for inclusion.  This factoring lets us share
/// the runner between `tck/v1.toml` (multi-tag whitelist) and
/// `tck/embedder-m23.toml` (single `@M23` tag).
fn run_fixture<F>(label: &str, fixture_path: &Path, tag_filter: F) -> Vec<String>
where
    F: Fn(&[String]) -> bool,
{
    let raw = std::fs::read_to_string(fixture_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", fixture_path.display()));

    let fixtures: Fixtures = toml::from_str(&raw)
        .unwrap_or_else(|e| panic!("parse error in {}: {e}", fixture_path.display()));

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut ignored = 0usize;
    let mut failed = Vec::<String>::new();

    for scenario in &fixtures.scenario {
        if !tag_filter(&scenario.tags) {
            continue;
        }

        total += 1;

        // Per-scenario Expected (bead cy-p5q): `ignore = true` maps to
        // `Expected::Ignored`, `outcome = "ok"` to `Expected::Supported`,
        // `outcome = "error"` to `Expected::Error`.
        match scenario.expected() {
            Expected::Ignored => {
                ignored += 1;
                println!(
                    "  IGNORED [{}]: {}",
                    scenario.name,
                    scenario.note.as_deref().unwrap_or("no note"),
                );
            }
            expected => {
                let expected_ok = expected == Expected::Supported;
                let is_ok = parse_ok(&scenario.query);
                let pass = expected_ok == is_ok;

                if pass {
                    passed += 1;
                } else {
                    failed.push(format!(
                        "FAIL [{}/{}]: expected {}, got {} — query: {:?}",
                        label,
                        scenario.name,
                        if expected_ok {
                            "parse-ok"
                        } else {
                            "parse-error"
                        },
                        if is_ok { "parse-ok" } else { "parse-error" },
                        scenario.query,
                    ));
                }
            }
        }
    }

    let run = total - ignored;
    println!("TCK {label}: {passed}/{run} scenarios passed ({ignored} ignored)");
    failed
}

#[test]
fn tck_v1_scenarios() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let fixture_path = Path::new(&manifest_dir).join("tck").join("v1.toml");

    let v1 = v1_tag_set();
    let failed = run_fixture("v1", &fixture_path, |tags| {
        tags.iter().any(|t| v1.contains(t.as_str()))
    });

    for line in &failed {
        eprintln!("{line}");
    }

    assert!(
        failed.is_empty(),
        "{} scenario(s) failed:\n{}",
        failed.len(),
        failed.join("\n"),
    );
}

/// Embedder M23 curated subset gate (bead cy-emb6, embedder-issue 0006).
///
/// Runs `tck/embedder-m23.toml`, filtering to scenarios carrying the
/// `@M23` tag.  The fixture starts as a hand-curated subset of M23
/// fundamentals that all parse cleanly today; embedders are expected
/// to extend it with the scenarios their legacy parser passes.  See
/// the file header for the add-only ratchet policy.
#[test]
fn tck_embedder_m23_scenarios() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let fixture_path = Path::new(&manifest_dir)
        .join("tck")
        .join("embedder-m23.toml");

    let m23 = embedder_m23_tag();
    let failed = run_fixture("embedder-m23", &fixture_path, |tags| {
        tags.iter().any(|t| t == m23)
    });

    for line in &failed {
        eprintln!("{line}");
    }

    assert!(
        failed.is_empty(),
        "{} scenario(s) failed:\n{}",
        failed.len(),
        failed.join("\n"),
    );
}
