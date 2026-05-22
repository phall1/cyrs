//! GQL ISO/IEC 39075:2024 conformance harness — beads cy-0hj, cy-1x7o.
//!
//! Loads every `.feature` file under `tck/gql-iso-39075/features/`,
//! scans for `Scenario:` / `Scenario Outline:` blocks, extracts each
//! scenario's `When executing query:` code-block, and runs the query
//! through `cyrs_db::Database` in `DialectMode::GqlAligned`.
//!
//! Two reports come out of one scan:
//!
//! * [`gql_iso_baseline`] — per-area parser-acceptance rate, written to
//!   `tck/gql-iso-39075/baseline.md`.  **Never fails** — it is a
//!   measurement, not a gate.
//!
//! * [`gql_coverage_baseline`] — grammar-coverage tracking (cy-1x7o).
//!   Each scenario carries a `@covers:` tag naming the GQL.g4 parser
//!   productions it exercises.  The harness validates every name
//!   against the vendored rule manifest (`tck/opengql-grammar/
//!   rules.json`) and writes `tck/gql-iso-39075/coverage.md`: how many
//!   of the 574 parser productions are reached by a passing scenario,
//!   plus the uncovered-production worklist.  **This test fails** if a
//!   `@covers:` tag names a production that is not a real parser rule,
//!   or if a scenario carries no `@covers:` tag at all — typos and
//!   omissions are bugs, not silent drift.
//!
//! Run with `cargo xtask gql-coverage` (or `cargo test -p cyrs-tck
//! --features gql-iso --test gql_iso`).
//!
//! See `tck/gql-iso-39075/README.md` for scope, the `@covers:`
//! convention, and ISO §-citations.

#![cfg(feature = "gql-iso")]
#![allow(clippy::uninlined_format_args)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cyrs_db::{DialectMode, workspace::Database};

// ---------------------------------------------------------------------------
// Gherkin scanner — same shape as `tests/full.rs`.  Kept as a self-contained
// copy to avoid widening the crate's public surface (cy-0hj constraint).
// Extended for cy-1x7o to carry each scenario's `@covers:` tag list.
// ---------------------------------------------------------------------------

struct ScenarioCase {
    feature: PathBuf,
    name: String,
    query: String,
    /// GQL.g4 parser productions this scenario claims to exercise,
    /// parsed from its `@covers:` Gherkin tag.
    covers: Vec<String>,
}

/// Coarse per-area bucket.  For the GQL bootstrap we use the single
/// top-level path component under `features/` (e.g. `clauses`,
/// `values`); the corpus is too small for the two-level scheme used by
/// the openCypher full-corpus harness.
fn area_of(rel: &Path) -> String {
    let parts: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    parts
        .first()
        .map_or_else(|| "<root>".to_owned(), |s| (*s).to_owned())
}

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

/// Extract every `@covers:` production name from a Gherkin tag line.
///
/// A tag line may carry several space-separated tags; only `@covers:`
/// tags contribute, and a single `@covers:` tag may list several
/// comma-separated production names, e.g.
/// `@covers:matchStatement,returnStatement`.
fn parse_covers_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tag in line.split_whitespace() {
        if let Some(list) = tag.strip_prefix("@covers:") {
            for name in list.split(',') {
                let name = name.trim();
                if !name.is_empty() {
                    out.push(name.to_owned());
                }
            }
        }
    }
    out
}

enum Phase {
    Idle,
    InScenario {
        name: String,
        outline: bool,
        covers: Vec<String>,
    },
    InQuery {
        name: String,
        outline: bool,
        covers: Vec<String>,
        query: String,
    },
    AfterQuery {
        name: String,
        outline: bool,
        covers: Vec<String>,
        query: String,
    },
    InExamples {
        name: String,
        covers: Vec<String>,
        query: String,
        header: Option<Vec<String>>,
        rows: Vec<Vec<String>>,
    },
}

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
    // Tags accumulated since the last scenario; consumed when the next
    // `Scenario:` / `Scenario Outline:` header is seen.
    let mut pending_covers: Vec<String> = Vec::new();

    let flush = |cases: &mut Vec<ScenarioCase>,
                 rel: &Path,
                 name: &str,
                 outline: bool,
                 covers: &[String],
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
                        covers: covers.to_vec(),
                    });
                }
            }
        } else {
            cases.push(ScenarioCase {
                feature: rel.to_path_buf(),
                name: name.to_owned(),
                query: query.to_owned(),
                covers: covers.to_vec(),
            });
        }
    };

    for raw_line in src.lines() {
        let line = raw_line;
        let trimmed = line.trim();

        if trimmed == "\"\"\"" {
            in_code_block = !in_code_block;
            if !in_code_block {
                phase = match std::mem::replace(&mut phase, Phase::Idle) {
                    Phase::InQuery {
                        name,
                        outline,
                        covers,
                        query,
                    } => Phase::AfterQuery {
                        name,
                        outline,
                        covers,
                        query,
                    },
                    other => other,
                };
            }
            continue;
        }

        if in_code_block {
            if let Phase::InQuery { query, .. } = &mut phase {
                if !query.is_empty() {
                    query.push('\n');
                }
                query.push_str(line);
            }
            continue;
        }

        // Gherkin tag line — accumulate `@covers:` productions for the
        // scenario that follows.  Other tags are ignored.
        if trimmed.starts_with('@') {
            pending_covers.extend(parse_covers_line(trimmed));
            continue;
        }

        if trimmed.starts_with('#') || trimmed.is_empty() {
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
                    covers,
                    query,
                    header: Some(h),
                    rows,
                } = std::mem::replace(&mut phase, Phase::Idle)
            {
                flush(
                    &mut cases,
                    &rel,
                    &name,
                    true,
                    &covers,
                    &query,
                    Some((&h, &rows)),
                );
            }
            continue;
        }

        if let Some(title) = trimmed.strip_prefix("Scenario Outline:") {
            if let Phase::InExamples {
                name,
                covers,
                query,
                header: Some(h),
                rows,
            } = std::mem::replace(&mut phase, Phase::Idle)
            {
                flush(
                    &mut cases,
                    &rel,
                    &name,
                    true,
                    &covers,
                    &query,
                    Some((&h, &rows)),
                );
            }
            phase = Phase::InScenario {
                name: title.trim().to_owned(),
                outline: true,
                covers: std::mem::take(&mut pending_covers),
            };
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("Scenario:") {
            if let Phase::InExamples {
                name,
                covers,
                query,
                header: Some(h),
                rows,
            } = std::mem::replace(&mut phase, Phase::Idle)
            {
                flush(
                    &mut cases,
                    &rel,
                    &name,
                    true,
                    &covers,
                    &query,
                    Some((&h, &rows)),
                );
            }
            phase = Phase::InScenario {
                name: title.trim().to_owned(),
                outline: false,
                covers: std::mem::take(&mut pending_covers),
            };
            continue;
        }

        if trimmed == "When executing query:" || trimmed.starts_with("When executing query:") {
            phase = match std::mem::replace(&mut phase, Phase::Idle) {
                Phase::InScenario {
                    name,
                    outline,
                    covers,
                }
                | Phase::AfterQuery {
                    name,
                    outline,
                    covers,
                    ..
                } => Phase::InQuery {
                    name,
                    outline,
                    covers,
                    query: String::new(),
                },
                other => other,
            };
            continue;
        }

        if trimmed.starts_with("Examples:") {
            phase = match std::mem::replace(&mut phase, Phase::Idle) {
                Phase::AfterQuery {
                    name,
                    outline: true,
                    covers,
                    query,
                } => Phase::InExamples {
                    name,
                    covers,
                    query,
                    header: None,
                    rows: Vec::new(),
                },
                other => other,
            };
            continue;
        }

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

        if let Phase::AfterQuery {
            name,
            outline: false,
            covers,
            query,
        } = &mut phase
        {
            flush(&mut cases, &rel, name, false, covers, query, None);
            phase = Phase::Idle;
        }
    }

    match phase {
        Phase::AfterQuery {
            name,
            outline: false,
            covers,
            query,
        } => flush(&mut cases, &rel, &name, false, &covers, &query, None),
        Phase::InExamples {
            name,
            covers,
            query,
            header: Some(h),
            rows,
        } => flush(
            &mut cases,
            &rel,
            &name,
            true,
            &covers,
            &query,
            Some((&h, &rows)),
        ),
        _ => {}
    }

    cases
}

// ---------------------------------------------------------------------------
// Parser acceptance check — runs in GqlAligned mode (the dialect this
// corpus targets).
// ---------------------------------------------------------------------------

fn parse_ok(query: &str) -> bool {
    let mut db = Database::new();
    let id = db.open_file(
        Path::new("gql-iso.cyp"),
        query.to_owned(),
        DialectMode::GqlAligned,
    );
    let Ok(out) = db.parse_cst(id) else {
        return false;
    };
    out.parse().errors().is_empty()
}

// ---------------------------------------------------------------------------
// GQL.g4 rule manifest — the set of parser production names.
//
// Read straight from the deterministic one-rule-per-line `rules.json`
// emitted by `cargo xtask gql-rules`; no JSON dependency needed.
// ---------------------------------------------------------------------------

fn parser_productions(grammar_dir: &Path) -> BTreeSet<String> {
    let path = grammar_dir.join("rules.json");
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut set = BTreeSet::new();
    for line in json.lines() {
        // Each rule renders as: `{"name": "X", "kind": "parser", ...}`.
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("{\"name\": \"") else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        let name = &rest[..end];
        if rest[end..].contains("\"kind\": \"parser\"") {
            set.insert(name.to_owned());
        }
    }
    assert!(
        !set.is_empty(),
        "no parser productions parsed from {} — has the rules.json format changed?",
        path.display(),
    );
    set
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
// Common scan
// ---------------------------------------------------------------------------

fn corpus_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    Path::new(&manifest_dir).join("tck").join("gql-iso-39075")
}

fn scan_all(features_root: &Path) -> (Vec<PathBuf>, Vec<ScenarioCase>) {
    let files = walk_features(features_root);
    let mut cases: Vec<ScenarioCase> = Vec::new();
    for f in &files {
        cases.extend(scan_feature(f, features_root));
    }
    (files, cases)
}

// ---------------------------------------------------------------------------
// Baseline emitter (parser-acceptance rate) — never fails.
// ---------------------------------------------------------------------------

#[test]
fn gql_iso_baseline() {
    let corpus = corpus_root();
    let features_root = corpus.join("features");
    assert!(
        features_root.is_dir(),
        "GQL-ISO bootstrap corpus missing: {}",
        features_root.display()
    );

    let (files, cases) = scan_all(&features_root);

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

    let baseline_path = corpus.join("baseline.md");
    let body = render_baseline(&areas, total, accepted, files.len());
    std::fs::write(&baseline_path, body)
        .unwrap_or_else(|e| panic!("write {}: {e}", baseline_path.display()));

    println!(
        "GQL-ISO bootstrap: parser accepts {}/{} scenarios ({:.1} %) across {} feature files → {}",
        accepted,
        total,
        pct(accepted, total),
        files.len(),
        baseline_path.display(),
    );
}

// ---------------------------------------------------------------------------
// Coverage emitter (grammar-production coverage) — bead cy-1x7o.
//
// FAILS if a `@covers:` tag names an unknown production or a scenario
// carries no `@covers:` tag.  Otherwise writes `coverage.md`.
// ---------------------------------------------------------------------------

#[test]
fn gql_coverage_baseline() {
    let corpus = corpus_root();
    let features_root = corpus.join("features");
    let grammar_dir = corpus
        .parent()
        .expect("corpus has a parent")
        .join("opengql-grammar");
    assert!(
        features_root.is_dir(),
        "GQL-ISO bootstrap corpus missing: {}",
        features_root.display()
    );

    let (files, cases) = scan_all(&features_root);
    let productions = parser_productions(&grammar_dir);

    // --- Validate every @covers: tag (typo / omission gate). -------------
    let mut errors: Vec<String> = Vec::new();
    for case in &cases {
        if case.covers.is_empty() {
            errors.push(format!(
                "scenario `{}` in {} has no `@covers:` tag",
                case.name,
                case.feature.display(),
            ));
            continue;
        }
        for name in &case.covers {
            if !productions.contains(name) {
                errors.push(format!(
                    "scenario `{}` in {}: `@covers:{}` is not a GQL.g4 parser production",
                    case.name,
                    case.feature.display(),
                    name,
                ));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "GQL coverage harness found {} bad `@covers:` tag(s):\n  {}\n\
         Every scenario must carry `@covers:` naming real parser productions \
         from tck/opengql-grammar/rules.json (regenerate with `cargo xtask gql-rules`).",
        errors.len(),
        errors.join("\n  "),
    );

    // --- Compute coverage: a production is covered if at least one -------
    // --- *passing* scenario lists it. -----------------------------------
    let mut covered_by: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut passing = 0usize;
    for case in &cases {
        if !parse_ok(&case.query) {
            continue;
        }
        passing += 1;
        let area_file = case.feature.display().to_string();
        for name in &case.covers {
            covered_by
                .entry(name.clone())
                .or_default()
                .insert(area_file.clone());
        }
    }

    let covered: BTreeSet<String> = covered_by.keys().cloned().collect();
    let uncovered: Vec<String> = productions.difference(&covered).cloned().collect();

    let coverage_path = corpus.join("coverage.md");
    let body = render_coverage(
        productions.len(),
        &covered_by,
        &uncovered,
        cases.len(),
        passing,
        files.len(),
    );
    std::fs::write(&coverage_path, body)
        .unwrap_or_else(|e| panic!("write {}: {e}", coverage_path.display()));

    println!(
        "GQL grammar coverage: {}/{} parser productions reached by a passing scenario ({:.1} %) → {}",
        covered.len(),
        productions.len(),
        pct(covered.len(), productions.len()),
        coverage_path.display(),
    );
}

#[allow(clippy::cast_precision_loss)]
fn pct(num: usize, denom: usize) -> f64 {
    if denom == 0 {
        0.0
    } else {
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
    out.push_str("# GQL ISO/IEC 39075:2024 — Bootstrap Conformance Baseline\n\n");
    out.push_str(
        "*Auto-generated by `cargo xtask gql-coverage` (`cargo test -p cyrs-tck \
         --features gql-iso --test gql_iso`).  Do not hand-edit.*\n\n",
    );
    out.push_str(
        "This file records the rolling parser-acceptance rate of the\n\
         `cyrs-db` front-end (in `DialectMode::GqlAligned`) against\n\
         the hand-authored GQL ISO bootstrap corpus under\n\
         `crates/cyrs-tck/tck/gql-iso-39075/` (see `README.md` for\n\
         scope and ISO §-citations).  Every Scenario Outline is\n\
         expanded against its `Examples:` table, so the counts below\n\
         are per-example, not per-outline.\n\n\
         \"Accepted\" means the parser emits zero syntax errors for the\n\
         query extracted from the scenario's `When executing query:`\n\
         step; it does **not** assert runtime semantics (the frontend\n\
         does no execution, spec §1.3 N1).\n\n\
         For grammar-production coverage (which of the 574 GQL.g4\n\
         parser productions a passing scenario reaches) see the sibling\n\
         `coverage.md`.\n\n",
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
         - Land parser support for the GQL-distinct constructs covered\n\
           above (one bead per area is a reasonable cadence).\n\
         - Grow the corpus: every new GQL-only construct that lands in\n\
           the parser should arrive with a scenario here citing its\n\
           ISO/IEC 39075:2024 § and a `@covers:` tag.\n\
         - Consider gating regressions in CI once a sub-area\n\
           stabilises at 100 %.\n",
    );
    out
}

fn render_coverage(
    total_productions: usize,
    covered_by: &BTreeMap<String, BTreeSet<String>>,
    uncovered: &[String],
    scenarios: usize,
    passing: usize,
    file_count: usize,
) -> String {
    use std::fmt::Write;

    let covered = covered_by.len();
    let mut out = String::new();
    out.push_str("# GQL ISO/IEC 39075:2024 — Grammar Coverage\n\n");
    out.push_str("*Auto-generated by `cargo xtask gql-coverage`.  Do not hand-edit.*\n\n");
    out.push_str(
        "This file tracks how much of the vendored ISO/IEC 39075:2024\n\
         reference grammar (`tck/opengql-grammar/GQL.g4`, manifest in\n\
         `rules.json`) the cyrs front-end parses.  A parser production\n\
         is **covered** when at least one conformance scenario that\n\
         lists it in a `@covers:` tag is *accepted* by the parser in\n\
         `DialectMode::GqlAligned` (zero syntax errors).\n\n\
         Coverage is a parser-*acceptance* measure: it shows the query\n\
         surface cyrs accepts, not runtime semantics (the front-end\n\
         does no execution, spec §1.3 N1).  The uncovered list below is\n\
         the worklist for growing the corpus — every entry is a\n\
         candidate for a new ISO-§-cited scenario.\n\n",
    );

    out.push_str("## Summary\n\n");
    out.push_str("| Metric | Value |\n");
    out.push_str("|---|---|\n");
    let _ = writeln!(
        out,
        "| GQL.g4 parser productions | **{total_productions}** |"
    );
    let _ = writeln!(
        out,
        "| Covered (≥1 passing scenario) | **{}** ({:.1} %) |",
        covered,
        pct(covered, total_productions),
    );
    let _ = writeln!(
        out,
        "| Uncovered | **{}** ({:.1} %) |",
        uncovered.len(),
        pct(uncovered.len(), total_productions),
    );
    let _ = writeln!(out, "| Feature files | **{file_count}** |");
    let _ = writeln!(
        out,
        "| Scenarios (passing / total) | **{passing} / {scenarios}** |"
    );
    out.push('\n');

    out.push_str("## Covered productions\n\n");
    out.push_str("| Production | Covered by |\n");
    out.push_str("|---|---|\n");
    for (name, files) in covered_by {
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let _ = writeln!(out, "| `{}` | {} |", name, files.join(", "));
    }
    out.push('\n');

    out.push_str("## Uncovered productions (worklist)\n\n");
    let _ = writeln!(
        out,
        "{} parser productions are not yet reached by any passing\n\
         scenario.  Each is a candidate for a new ISO-§-cited\n\
         `.feature` scenario tagged `@covers:<production>`.\n",
        uncovered.len(),
    );
    out.push_str("```\n");
    // Wrap the comma-joined list at ~76 columns for reviewable diffs.
    let mut col = 0usize;
    for (i, name) in uncovered.iter().enumerate() {
        let token = if i + 1 == uncovered.len() {
            name.clone()
        } else {
            format!("{name}, ")
        };
        if col + token.len() > 76 && col > 0 {
            out.push('\n');
            col = 0;
        }
        out.push_str(&token);
        col += token.len();
    }
    if col > 0 {
        out.push('\n');
    }
    out.push_str("```\n");
    out
}
