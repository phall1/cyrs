//! TCK harness — spec 0001 §17.5.
//!
//! Reads scenarios from `tck/v1.toml`, filters to those carrying at least one
//! v1 tag (per [`cypher_tck::v1_tags`]), then runs each through
//! `cypher_db::Database` and asserts the expected parse outcome.
//!
//! Run with:
//!   cargo test -p cypher-tck
//!
//! Tags covered: @MATCH, @OPTIONAL-MATCH, @WHERE, @RETURN, @WITH, @UNWIND,
//! @CREATE, @MERGE, @SET, @REMOVE, @DELETE, @EXPRESSIONS, @AGGREGATIONS,
//! @STRINGS, @LISTS, @MAPS, @PATTERNS, @NULL  (v1 supported)
//! @CALL-SUBQUERY, @LOAD-CSV  (must emit a parse error)
//!
//! The full vendored upstream TCK is exercised by a separate test gated
//! behind `--features full-tck`; see `tests/full.rs`.

use std::path::Path;

use cypher_db::{DialectMode, workspace::Database};
use cypher_tck::{Expected, v1_tags};
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

#[test]
fn tck_v1_scenarios() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let fixture_path = Path::new(&manifest_dir).join("tck").join("v1.toml");

    let raw = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", fixture_path.display()));

    let fixtures: Fixtures = toml::from_str(&raw)
        .unwrap_or_else(|e| panic!("parse error in {}: {e}", fixture_path.display()));

    let v1_tags = v1_tag_set();

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut ignored = 0usize;
    let mut failed = Vec::<String>::new();

    for scenario in &fixtures.scenario {
        // Filter: skip scenarios that share no tags with the v1 tag set.
        let matched_tags: Vec<&String> = scenario
            .tags
            .iter()
            .filter(|t| v1_tags.contains(t.as_str()))
            .collect();

        if matched_tags.is_empty() {
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
                        "FAIL [{}]: expected {}, got {} — query: {:?}",
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

    // Print a summary regardless of pass/fail.
    let run = total - ignored;
    println!("TCK v1: {passed}/{run} scenarios passed ({ignored} ignored)");
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
