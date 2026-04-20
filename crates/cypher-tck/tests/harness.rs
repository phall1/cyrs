//! TCK harness — spec 0001 §17.5.
//!
//! Reads scenarios from `tck/v1.toml`, filters to those carrying at least one
//! v1 tag (per [`cypher_tck::v1_gates`]), then runs each through
//! `cypher_db::Database` and asserts the expected parse outcome.
//!
//! Run with:
//!   cargo test -p cypher-tck
//!
//! Tags covered: @MATCH, @OPTIONAL-MATCH, @WHERE, @RETURN, @WITH, @UNWIND,
//! @CREATE, @MERGE, @SET, @REMOVE, @DELETE, @EXPRESSIONS, @AGGREGATIONS,
//! @STRINGS, @LISTS, @MAPS, @PATTERNS, @NULL  (v1 green)
//! @CALL-SUBQUERY, @LOAD-CSV  (v1 red — must emit a parse error)

use std::path::Path;

use cypher_db::{DialectMode, workspace::Database};
use cypher_tck::v1_gates;
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the set of v1 tags (with leading `@`).
fn v1_tag_set() -> std::collections::HashSet<String> {
    v1_gates().into_iter().map(|g| g.tag.to_owned()).collect()
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

        // Scenarios marked `ignore = true` are acknowledged as known parser
        // bugs.  They are counted but not run.
        if scenario.ignore {
            ignored += 1;
            println!(
                "  IGNORED [{}]: {}",
                scenario.name,
                scenario.note.as_deref().unwrap_or("no note"),
            );
            continue;
        }

        // Determine whether the scenario expects a parse-ok or parse-error
        // from the fixture's `outcome` field.
        //
        // The v1 gate (`Expected::Green` / `Expected::Red`) signals which
        // *overall tag* is expected to be conformant, but does not override the
        // per-scenario outcome: a tag can be "green" (fully conformant) while
        // individual scenarios under that tag test parser *rejection* of
        // malformed input (negative tests).
        let expected_ok = scenario.outcome == Outcome::Ok;

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
