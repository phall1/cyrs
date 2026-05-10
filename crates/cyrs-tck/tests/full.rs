//! Full openCypher TCK harness — spec 0001 §17.5, bead cy-p5q.
//!
//! Loads every `.feature` file under `tck/full/features/`, scans for
//! `Scenario:` / `Scenario Outline:` blocks, extracts each scenario's
//! `When executing query:` code-block, expands Scenario Outline
//! placeholders against their `Examples:` tables, and runs the query
//! through `cyrs_db::Database`.  The result is a per-area parser-
//! acceptance baseline written to `tck/full-baseline.md`.
//!
//! **This test never fails.**  It is a measurement, not a gate: its job
//! is to produce a rolling snapshot that the workspace can diff against
//! to catch parser regressions on the full corpus without blocking PRs
//! on the mountain of as-yet-untriaged scenarios.
//!
//! Run with `cargo test -p cyrs-tck --features full-tck`, or via
//! `cargo xtask tck-baseline` which wraps the same invocation.
//!
//! See `tck/full/VENDORED.md` for the vendored upstream pin, and
//! `crates/cyrs-tck/README.md` for operator-facing docs.

#![cfg(feature = "full-tck")]
#![allow(clippy::uninlined_format_args)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cyrs_db::{DialectMode, workspace::Database};

// ---------------------------------------------------------------------------
// Gherkin scanner
// ---------------------------------------------------------------------------

/// One extracted scenario's parseable query.
///
/// Scenario Outlines are expanded into one [`ScenarioCase`] per row of
/// their `Examples:` table.
struct ScenarioCase {
    /// Feature-file path, relative to `tck/full/features/`.
    feature: PathBuf,
    /// `Scenario:` / `Scenario Outline:` title.
    ///
    /// Retained so future work can emit per-scenario diagnostics; unused
    /// in the current baseline renderer.
    #[allow(dead_code)]
    name: String,
    /// The Cypher source extracted from the `When executing query:` step,
    /// with any `<placeholder>` tokens substituted from an Examples row.
    query: String,
}

/// Coarse per-area bucket keyed on the two top-level path components
/// under `features/` (e.g. `clauses/match`, `expressions/list`).
fn area_of(rel: &Path) -> String {
    let parts: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[0], parts[1])
    } else if let Some(first) = parts.first() {
        (*first).to_owned()
    } else {
        "<root>".to_owned()
    }
}

/// Walk `root` depth-first and yield every `.feature` file path.
fn walk_features(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("feature") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Scanner state: each Scenario / Scenario Outline moves through
/// `Idle` → `InScenario` → `InQuery` → `AfterQuery` → `InExamples`
/// (the last only for outlines).
enum Phase {
    Idle,
    InScenario {
        name: String,
        outline: bool,
    },
    InQuery {
        name: String,
        outline: bool,
        query: String,
    },
    AfterQuery {
        name: String,
        outline: bool,
        query: String,
    },
    InExamples {
        name: String,
        query: String,
        header: Option<Vec<String>>,
        rows: Vec<Vec<String>>,
    },
}

/// Extract every scenario's parseable query from one `.feature` file.
///
/// The scanner is deliberately small — it handles the Gherkin subset
/// the openCypher TCK actually uses (§17.5 has no need for full
/// Gherkin):
///
/// - `Scenario:` / `Scenario Outline:` headers start a block.
/// - `When executing query:` introduces a triple-quoted code-block.
/// - `Examples:` introduces a pipe-delimited table whose header names
///   substitute `<placeholder>` tokens in the query.
/// - Lines starting with `#` or `@` (tags) are ignored.
fn scan_feature(feature_path: &Path, features_root: &Path) -> Vec<ScenarioCase> {
    let Ok(src) = std::fs::read_to_string(feature_path) else {
        return Vec::new();
    };
    let rel = feature_path
        .strip_prefix(features_root)
        .unwrap_or(feature_path)
        .to_path_buf();

    let mut cases = Vec::new();

    let mut phase = Phase::Idle;
    let mut in_code_block = false;

    // Helper: flush a completed block into `cases` (Scenario = one case,
    // Scenario Outline = one case per Examples row).
    let flush = |cases: &mut Vec<ScenarioCase>,
                 rel: &Path,
                 name: &str,
                 outline: bool,
                 query: &str,
                 examples: Option<(&Vec<String>, &Vec<Vec<String>>)>| {
        if outline {
            if let Some((header, rows)) = examples {
                for row in rows {
                    let mut q = query.to_owned();
                    for (h, v) in header.iter().zip(row.iter()) {
                        q = q.replace(&format!("<{}>", h), v);
                    }
                    cases.push(ScenarioCase {
                        feature: rel.to_path_buf(),
                        name: name.to_owned(),
                        query: q,
                    });
                }
            }
            // Scenario Outline with no Examples → skip.
        } else {
            cases.push(ScenarioCase {
                feature: rel.to_path_buf(),
                name: name.to_owned(),
                query: query.to_owned(),
            });
        }
    };

    for raw_line in src.lines() {
        let line = raw_line;
        let trimmed = line.trim();

        // Triple-quote handling is independent of phase — a `"""` flips
        // the code-block state.
        if trimmed == "\"\"\"" {
            in_code_block = !in_code_block;
            if !in_code_block {
                // Code-block just closed: if we were inside a query, promote
                // to AfterQuery.
                phase = match std::mem::replace(&mut phase, Phase::Idle) {
                    Phase::InQuery {
                        name,
                        outline,
                        query,
                    } => Phase::AfterQuery {
                        name,
                        outline,
                        query,
                    },
                    other => other,
                };
            }
            continue;
        }

        // Inside a code-block every non-delimiter line is content.
        if in_code_block {
            if let Phase::InQuery { query, .. } = &mut phase {
                if !query.is_empty() {
                    query.push('\n');
                }
                query.push_str(line);
            }
            continue;
        }

        // Skip comments and tag lines outside of code blocks.  An empty
        // line additionally terminates a pending Examples block.
        //
        // cy-41u: empty lines MUST NOT drop `AfterQuery` or other
        // in-progress phases; a `Scenario Outline` routinely separates its
        // query block from the `Examples:` table with a blank line. Only
        // `InExamples` (and only when the table has already begun) gets
        // flushed on empty-line.
        if trimmed.starts_with('#') || trimmed.starts_with('@') || trimmed.is_empty() {
            if trimmed.is_empty()
                && matches!(
                    &phase,
                    Phase::InExamples {
                        header: Some(_),
                        ..
                    }
                )
                && let Phase::InExamples {
                    name,
                    query,
                    header: Some(h),
                    rows,
                } = std::mem::replace(&mut phase, Phase::Idle)
            {
                flush(&mut cases, &rel, &name, true, &query, Some((&h, &rows)));
            }
            continue;
        }

        // New Scenario header — flushes any pending Examples table first.
        if let Some(title) = trimmed.strip_prefix("Scenario Outline:") {
            if let Phase::InExamples {
                name,
                query,
                header: Some(h),
                rows,
            } = std::mem::replace(&mut phase, Phase::Idle)
            {
                flush(&mut cases, &rel, &name, true, &query, Some((&h, &rows)));
            }
            phase = Phase::InScenario {
                name: title.trim().to_owned(),
                outline: true,
            };
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("Scenario:") {
            if let Phase::InExamples {
                name,
                query,
                header: Some(h),
                rows,
            } = std::mem::replace(&mut phase, Phase::Idle)
            {
                flush(&mut cases, &rel, &name, true, &query, Some((&h, &rows)));
            }
            phase = Phase::InScenario {
                name: title.trim().to_owned(),
                outline: false,
            };
            continue;
        }

        // Transition into a query block on the `When executing query:` step.
        if trimmed == "When executing query:" || trimmed.starts_with("When executing query:") {
            phase = match std::mem::replace(&mut phase, Phase::Idle) {
                Phase::InScenario { name, outline } | Phase::AfterQuery { name, outline, .. } => {
                    Phase::InQuery {
                        name,
                        outline,
                        query: String::new(),
                    }
                }
                other => other,
            };
            continue;
        }

        // Examples: table follows a Scenario Outline; only the header +
        // data rows are meaningful.
        if trimmed.starts_with("Examples:") {
            phase = match std::mem::replace(&mut phase, Phase::Idle) {
                Phase::AfterQuery {
                    name,
                    outline: true,
                    query,
                } => Phase::InExamples {
                    name,
                    query,
                    header: None,
                    rows: Vec::new(),
                },
                other => other,
            };
            continue;
        }

        // Inside an Examples table, `| a | b | c |` rows feed the
        // expansion.
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            if let Phase::InExamples { header, rows, .. } = &mut phase {
                let cells: Vec<String> = trimmed
                    .trim_matches('|')
                    .split('|')
                    .map(|c| c.trim().to_owned())
                    .collect();
                if header.is_none() {
                    *header = Some(cells);
                } else {
                    rows.push(cells);
                }
            }
            continue;
        }

        // End of a non-outline scenario: a new non-table, non-query line
        // after a closed query block means we can flush the plain
        // Scenario now (there will be no Examples).
        if let Phase::AfterQuery {
            name,
            outline: false,
            query,
        } = &mut phase
        {
            flush(&mut cases, &rel, name, false, query, None);
            phase = Phase::Idle;
        }
    }

    // End-of-file flush.
    match phase {
        Phase::AfterQuery {
            name,
            outline: false,
            query,
        } => flush(&mut cases, &rel, &name, false, &query, None),
        Phase::InExamples {
            name,
            query,
            header: Some(h),
            rows,
        } => flush(&mut cases, &rel, &name, true, &query, Some((&h, &rows))),
        _ => {}
    }

    cases
}

// ---------------------------------------------------------------------------
// Parser acceptance check
// ---------------------------------------------------------------------------

fn parse_ok(query: &str) -> bool {
    // Brand-new `Database` per query keeps the Salsa interner from
    // sharing state across unrelated inputs — matches the v1 harness.
    let mut db = Database::new();
    let id = db.open_file(
        Path::new("tck.cyp"),
        query.to_owned(),
        DialectMode::GqlAligned,
    );
    let Ok(out) = db.parse_cst(id) else {
        return false;
    };
    out.parse().errors().is_empty()
}

// ---------------------------------------------------------------------------
// Per-area aggregation
// ---------------------------------------------------------------------------

#[derive(Default)]
struct AreaStats {
    total: usize,
    accepted: usize,
}

// ---------------------------------------------------------------------------
// Baseline emitter
// ---------------------------------------------------------------------------

#[test]
fn tck_full_baseline() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let features_root = Path::new(&manifest_dir)
        .join("tck")
        .join("full")
        .join("features");
    assert!(
        features_root.is_dir(),
        "vendored corpus missing: {}",
        features_root.display()
    );

    let files = walk_features(&features_root);
    let mut cases: Vec<ScenarioCase> = Vec::new();
    for f in &files {
        cases.extend(scan_feature(f, &features_root));
    }

    let mut areas: BTreeMap<String, AreaStats> = BTreeMap::new();
    for case in &cases {
        let area = area_of(&case.feature);
        let stats = areas.entry(area).or_default();
        stats.total += 1;
        if parse_ok(&case.query) {
            stats.accepted += 1;
        }
    }

    let total: usize = areas.values().map(|s| s.total).sum();
    let accepted: usize = areas.values().map(|s| s.accepted).sum();

    let baseline_path = Path::new(&manifest_dir)
        .join("tck")
        .join("full-baseline.md");
    let body = render_baseline(&areas, total, accepted, files.len());
    std::fs::write(&baseline_path, body)
        .unwrap_or_else(|e| panic!("write {}: {e}", baseline_path.display()));

    println!(
        "TCK full: parser accepts {}/{} scenarios ({:.1} %) across {} feature files → {}",
        accepted,
        total,
        pct(accepted, total),
        files.len(),
        baseline_path.display(),
    );
}

#[allow(clippy::cast_precision_loss)]
fn pct(num: usize, denom: usize) -> f64 {
    if denom == 0 {
        0.0
    } else {
        // Counts are in the thousands at most — precision loss is
        // structurally impossible on any plausible TCK corpus.
        (num as f64 / denom as f64) * 100.0
    }
}

fn render_baseline(
    areas: &BTreeMap<String, AreaStats>,
    total: usize,
    accepted: usize,
    file_count: usize,
) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    out.push_str("# openCypher TCK — Full-Corpus Parser Baseline\n\n");
    out.push_str(
        "*Auto-generated by `cargo test -p cyrs-tck --features full-tck` \
         (equivalent: `cargo xtask tck-baseline`).  Do not hand-edit.*\n\n",
    );
    out.push_str(
        "This file records the rolling parser-acceptance rate of the\n\
         `cyrs-db` front-end against the vendored openCypher TCK corpus\n\
         under `crates/cyrs-tck/tck/full/` (see `VENDORED.md` for the\n\
         pinned upstream revision).  Every Scenario Outline is expanded\n\
         against its `Examples:` table, so the counts below are\n\
         per-example, not per-outline.\n\n\
         \"Accepted\" means the parser emits zero syntax errors for the\n\
         query extracted from the scenario's `When executing query:`\n\
         step; it does **not** yet assert that the query's runtime result\n\
         matches the TCK's `Then` expectations (the frontend does no\n\
         execution, spec §1.3 N1).  Scenarios whose expected outcome is\n\
         `Expected::Error` are still untriaged (`Expected::Ignored`), so\n\
         acceptance is not yet equivalent to pass-rate.\n\n",
    );
    out.push_str("## Summary\n\n");
    out.push_str("| Metric | Value |\n");
    out.push_str("|---|---|\n");
    let _ = writeln!(out, "| Feature files scanned | **{file_count}** |");
    let _ = writeln!(
        out,
        "| Scenario cases (outline rows expanded) | **{total}** |"
    );
    let _ = writeln!(
        out,
        "| Accepted by parser | **{}** ({:.1} %) |",
        accepted,
        pct(accepted, total),
    );
    let _ = writeln!(
        out,
        "| Rejected by parser | **{}** ({:.1} %) |",
        total - accepted,
        pct(total - accepted, total),
    );
    out.push('\n');

    out.push_str("## Per-area pass counts\n\n");
    out.push_str("| Area | Accepted | Total | % |\n");
    out.push_str("|---|---|---|---|\n");
    for (area, stats) in areas {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {:.1} % |",
            area,
            stats.accepted,
            stats.total,
            pct(stats.accepted, stats.total),
        );
    }
    out.push('\n');
    out.push_str(
        "## Next steps\n\n\
         - Triage scenarios one area at a time by classifying their\n\
           `Expected` outcome (`Supported` / `Error`).  Once triaged,\n\
           `accepted` becomes directly comparable to `expected`.\n\
         - Gate regressions on this number in CI once a sub-area\n\
           stabilises at 100 %.\n",
    );
    out
}

/// Regression guard for cy-41u: the six CASE shapes added by the bead
/// all parse cleanly. If this ever fails, the TCK Conditional2
/// scenarios that depend on CASE will regress silently because the
/// harness just counts accept/reject — this test surfaces the root
/// cause.
#[test]
fn probe_case_scenarios() {
    let probes: &[(&str, &str)] = &[
        ("generic", "RETURN CASE WHEN 1 = 1 THEN 'yes' ELSE 'no' END"),
        (
            "simple_when_int_neg",
            "RETURN CASE -10 WHEN -10 THEN 'a' ELSE 'b' END AS r",
        ),
        (
            "simple_when_int_0",
            "RETURN CASE 0 WHEN 0 THEN 'a' ELSE 'b' END AS r",
        ),
        (
            "simple_when_bool",
            "RETURN CASE true WHEN true THEN 'a' ELSE 'b' END AS r",
        ),
        (
            "simple_when_float",
            "RETURN CASE 10.1 WHEN 10.1 THEN 'a' ELSE 'b' END AS r",
        ),
        (
            "full_case_from_feature",
            "RETURN CASE -10\n    WHEN -10 THEN 'minus ten'\n    WHEN 0 THEN 'zero'\n    ELSE 'other'\n  END AS result",
        ),
    ];
    let mut any_fail = false;
    for (name, q) in probes {
        let ok = parse_ok(q);
        println!("probe {name}: {}", if ok { "OK" } else { "FAIL" });
        if !ok {
            println!("  query: {q:?}");
            any_fail = true;
        }
    }
    assert!(!any_fail, "one or more CASE probes failed");
}
