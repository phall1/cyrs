//! GQL ISO/IEC 39075:2024 conformance bootstrap harness — bead cy-0hj.
//!
//! Loads every `.feature` file under `tck/gql-iso-39075/features/`,
//! scans for `Scenario:` / `Scenario Outline:` blocks, extracts each
//! scenario's `When executing query:` code-block, and runs the query
//! through `cypher_db::Database` in `DialectMode::GqlAligned`.  The
//! result is a per-area parser-acceptance baseline written to
//! `tck/gql-iso-39075/baseline.md`.
//!
//! **This test never fails.**  Like the openCypher full-corpus harness,
//! it is a measurement, not a gate: its job is to produce a rolling
//! snapshot that the workspace can diff against to catch parser
//! regressions or improvements on the GQL-distinct surface.
//!
//! Initial pass-rate is expected to be low — the parser does not yet
//! implement most GQL-only constructs.  This bootstrap establishes the
//! harness + scenarios; future beads land the parser changes.
//!
//! Run with `cargo test -p cyrs-tck --features gql-iso --test gql_iso`.
//!
//! See `tck/gql-iso-39075/README.md` for scope, source citations, and
//! the in-scope vs out-of-scope split.

#![cfg(feature = "gql-iso")]
#![allow(clippy::uninlined_format_args)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cypher_db::{DialectMode, workspace::Database};

// ---------------------------------------------------------------------------
// Gherkin scanner — same shape as `tests/full.rs`.  Kept as a self-contained
// copy to avoid widening the crate's public surface (cy-0hj constraint).
// ---------------------------------------------------------------------------

struct ScenarioCase {
    feature: PathBuf,
    #[allow(dead_code)]
    name: String,
    query: String,
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

        if trimmed == "\"\"\"" {
            in_code_block = !in_code_block;
            if !in_code_block {
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

        if in_code_block {
            if let Phase::InQuery { query, .. } = &mut phase {
                if !query.is_empty() {
                    query.push('\n');
                }
                query.push_str(line);
            }
            continue;
        }

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
            query,
        } = &mut phase
        {
            flush(&mut cases, &rel, name, false, query, None);
            phase = Phase::Idle;
        }
    }

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
fn gql_iso_baseline() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let features_root = Path::new(&manifest_dir)
        .join("tck")
        .join("gql-iso-39075")
        .join("features");
    assert!(
        features_root.is_dir(),
        "GQL-ISO bootstrap corpus missing: {}",
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
        .join("gql-iso-39075")
        .join("baseline.md");
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
        "*Auto-generated by `cargo test -p cyrs-tck --features gql-iso --test gql_iso`.  \
         Do not hand-edit.*\n\n",
    );
    out.push_str(
        "This file records the rolling parser-acceptance rate of the\n\
         `cypher-db` front-end (in `DialectMode::GqlAligned`) against\n\
         the hand-authored GQL ISO bootstrap corpus under\n\
         `crates/cypher-tck/tck/gql-iso-39075/` (see `README.md` for\n\
         scope and ISO §-citations).  Every Scenario Outline is\n\
         expanded against its `Examples:` table, so the counts below\n\
         are per-example, not per-outline.\n\n\
         \"Accepted\" means the parser emits zero syntax errors for the\n\
         query extracted from the scenario's `When executing query:`\n\
         step; it does **not** assert runtime semantics (the frontend\n\
         does no execution, spec §1.3 N1).\n\n\
         The corpus is intentionally a *bootstrap* (cy-0hj): it pins\n\
         the GQL-distinct surface so future beads can land parser\n\
         changes against a stable set of scenarios.  Initial pass-rate\n\
         is therefore expected to be low — most GQL-only constructs\n\
         (`INSERT NODE`, `FILTER`, `REPEATABLE ELEMENTS`, `IS TYPED`,\n\
         `ANY SHORTEST`, etc.) are not yet implemented.\n\n",
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
           ISO/IEC 39075:2024 §.\n\
         - Consider gating regressions in CI once a sub-area\n\
           stabilises at 100 %.\n",
    );
    out
}
