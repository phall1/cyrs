//! bench_incremental — long-horizon incremental reanalysis + RSS stability
//! (spec §11.6, §17.10; bead cy-bh5).
//!
//! # What this bench covers
//!
//! Two independent long-horizon workloads, each driving 10,000 edits:
//!
//! 1. **Agent mode** — a single `FileId` is re-edited 10,000 times via
//!    `Database::update_file`.  Exercises the Salsa per-query LRU caps
//!    (cy-31b) and the agent-side FileId interning (cy-ax5): the DB should
//!    not accumulate memoised state without bound as revisions churn.
//! 2. **LSP file-churn mode** — 10,000 open → analyse → close cycles, each
//!    opening a fresh `FileId` and evicting it via `Database::remove_file`.
//!    Exercises the `didClose`-driven eviction path (cy-it7) and the
//!    Salsa LRU caps: dropped FileIds must not keep their memoised state
//!    pinned once the LRU window has slid past them.
//!
//! # How the RSS gate works
//!
//! After the first [`WARMUP_EDITS`] edits we sample current resident-set
//! size and record it as the baseline.  After the remaining
//! (`TOTAL_EDITS − WARMUP_EDITS`) edits we sample again and compare the
//! delta against [`RSS_TOLERANCE`].  Drift beyond ±10% panics the bench,
//! which causes `cargo bench` to exit non-zero and the PR gate (CI
//! `bench-pr` job, spec §17.10) to fail.
//!
//! # Why `harness = false` + custom `main`
//!
//! Criterion measures time, not memory.  Using a custom main lets us:
//!
//! * run criterion timing benches for the per-edit reanalysis latency in
//!   each mode — these feed into the existing 10 % time-regression gate; and
//! * run the RSS-stability gate exactly once per invocation, after the
//!   timing benches have finished warming up the allocator.
//!
//! If the RSS gate fails we `std::process::exit(1)` so the non-zero exit
//! propagates to CI even when criterion's own summary would have been
//! clean.
//!
//! # RSS noise
//!
//! glibc and the system malloc reserve pages opportunistically; a single
//! RSS sample can jitter by a few MiB.  The 10 % band is intentionally
//! generous relative to the per-edit working set (parse tree + HIR + plan
//! on a 7-clause query is ~10 KiB; 10 k edits is ~100 MiB of *churn* but
//! only a few MiB of *live* set once LRU eviction kicks in).  We also
//! sample RSS three times with short sleeps and keep the median to damp
//! transient GC/compaction blips.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Duration;

use criterion::Criterion;

use cyrs_db::{Database, DialectMode};

// ---------------------------------------------------------------------------
// Workload configuration
// ---------------------------------------------------------------------------

/// Total edits per long-horizon workload.  Spec §17.10 fixes this at 10,000.
const TOTAL_EDITS: usize = 10_000;

/// Warmup window after which we sample the baseline RSS.  Spec §11.6
/// defines the baseline as "the first 1,000 edits".
const WARMUP_EDITS: usize = 1_000;

/// Drift tolerance.  Spec §11.6: steady-state RSS must stay within ±10 % of
/// the warmup-window baseline; ≥10 % drift is a blocking regression.
const RSS_TOLERANCE: f64 = 0.10;

/// How many RSS samples to median-filter at each checkpoint.
const RSS_SAMPLE_COUNT: usize = 3;

/// Sleep between successive RSS samples.  Short enough not to dominate
/// bench runtime, long enough to let the allocator settle after bulk
/// drops.
const RSS_SAMPLE_GAP: Duration = Duration::from_millis(10);

/// Representative multi-clause Cypher query.  Matches the shape used by
/// the other benches so timing comparisons stay apples-to-apples.
const BASE_QUERY: &str = "\
MATCH (n:Person {name: $name})-[r:KNOWS*1..3]->(m:Person)
WHERE m.age > 30 AND m.active = true
WITH n, m, r, count(*) AS hops
ORDER BY hops ASC
LIMIT 100
RETURN n.name AS source, m.name AS target, hops
";

/// Produce a unique source text per edit round.
///
/// Appending a trailing line-comment preserves the AST shape across
/// rounds while still forcing `parse_cst` to re-fire (Salsa keys off the
/// source-text hash via `options_digest`).  We do the allocation outside
/// the measured loop.
#[inline]
fn edit_source(round: usize) -> String {
    let mut s = String::with_capacity(BASE_QUERY.len() + 16);
    s.push_str(BASE_QUERY);
    s.push_str("// edit-");
    // Format as fixed-width so the string length is constant across rounds;
    // a growing length would bias the parse-cost sample.
    s.push_str(&format!("{round:06}"));
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// RSS sampling
// ---------------------------------------------------------------------------

/// Sample current physical-resident-set size in bytes.  Returns `None` if
/// the platform does not support the query (the gate is then skipped with
/// a visible warning).
fn sample_rss_bytes() -> Option<u64> {
    memory_stats::memory_stats().map(|s| s.physical_mem as u64)
}

/// Median of [`RSS_SAMPLE_COUNT`] samples taken [`RSS_SAMPLE_GAP`] apart.
fn rss_median() -> Option<u64> {
    let mut samples = Vec::with_capacity(RSS_SAMPLE_COUNT);
    for i in 0..RSS_SAMPLE_COUNT {
        if i > 0 {
            std::thread::sleep(RSS_SAMPLE_GAP);
        }
        samples.push(sample_rss_bytes()?);
    }
    samples.sort_unstable();
    Some(samples[samples.len() / 2])
}

/// Result of one long-horizon workload.
struct GateOutcome {
    mode: &'static str,
    baseline_bytes: Option<u64>,
    steady_bytes: Option<u64>,
}

impl GateOutcome {
    /// Returns `Some(drift)` if both samples are available, else `None`.
    fn drift(&self) -> Option<f64> {
        let base = self.baseline_bytes? as f64;
        let steady = self.steady_bytes? as f64;
        if base <= 0.0 {
            return None;
        }
        Some((steady - base) / base)
    }
}

// ---------------------------------------------------------------------------
// Workloads
// ---------------------------------------------------------------------------

/// Agent mode: one long-lived `FileId`, 10 k edits via `update_file`.
fn run_agent_workload() -> GateOutcome {
    let mut db = Database::new();
    let id = db.open_file(
        Path::new("agent.cyp"),
        edit_source(0),
        DialectMode::GqlAligned,
    );
    // Warm the analysis pipeline once so first-edit fixed costs don't skew
    // the baseline sample.
    let _ = black_box(db.analyse_file(id));

    for round in 1..=WARMUP_EDITS {
        db.update_file(id, edit_source(round))
            .expect("FileId stays open throughout the workload");
        let _ = black_box(db.analyse_file(id));
    }
    let baseline = rss_median();

    for round in (WARMUP_EDITS + 1)..=TOTAL_EDITS {
        db.update_file(id, edit_source(round))
            .expect("FileId stays open throughout the workload");
        let _ = black_box(db.analyse_file(id));
    }
    let steady = rss_median();

    GateOutcome {
        mode: "agent single-FileId",
        baseline_bytes: baseline,
        steady_bytes: steady,
    }
}

/// LSP mode: 10 k open → analyse → close cycles, each with a fresh `FileId`.
fn run_lsp_workload() -> GateOutcome {
    let mut db = Database::new();
    let mut path_buf = PathBuf::from("lsp-churn.cyp");

    let mut churn_once = |db: &mut Database, round: usize| {
        path_buf.set_file_name(format!("lsp-{round:06}.cyp"));
        let id = db.open_file(
            path_buf.as_path(),
            edit_source(round),
            DialectMode::GqlAligned,
        );
        let _ = black_box(db.analyse_file(id));
        db.remove_file(id)
            .expect("fresh FileId is always open at close time");
    };

    for round in 0..WARMUP_EDITS {
        churn_once(&mut db, round);
    }
    let baseline = rss_median();

    for round in WARMUP_EDITS..TOTAL_EDITS {
        churn_once(&mut db, round);
    }
    let steady = rss_median();

    GateOutcome {
        mode: "LSP FileId churn",
        baseline_bytes: baseline,
        steady_bytes: steady,
    }
}

// ---------------------------------------------------------------------------
// Criterion timing benches (per-edit reanalysis latency)
//
// These feed the existing §17.10 time-regression gate.  They are small
// and independent of the RSS workloads above.
// ---------------------------------------------------------------------------

fn bench_agent_per_edit(c: &mut Criterion) {
    let mut db = Database::new();
    let id = db.open_file(
        Path::new("agent.cyp"),
        edit_source(0),
        DialectMode::GqlAligned,
    );
    let _ = db.analyse_file(id);

    // Round counter is shared across iterations so each iter produces a
    // distinct source and cache-misses cleanly.
    let mut round: usize = 1;
    c.bench_function("incremental_agent_edit", |b| {
        b.iter(|| {
            db.update_file(id, edit_source(round)).unwrap();
            let _ = black_box(db.analyse_file(id));
            round = round.wrapping_add(1);
        });
    });
}

fn bench_lsp_per_edit(c: &mut Criterion) {
    let mut db = Database::new();
    let mut round: usize = 0;

    c.bench_function("incremental_lsp_churn_edit", |b| {
        b.iter(|| {
            let path = PathBuf::from(format!("lsp-{round:06}.cyp"));
            let id = db.open_file(path.as_path(), edit_source(round), DialectMode::GqlAligned);
            let _ = black_box(db.analyse_file(id));
            db.remove_file(id).unwrap();
            round = round.wrapping_add(1);
        });
    });
}

// ---------------------------------------------------------------------------
// Main — custom harness (harness = false in Cargo.toml)
// ---------------------------------------------------------------------------

fn main() {
    // 1. Criterion timing benches — feeds the existing 10 % time-regression
    //    gate in .github/workflows/bench.yml.
    let mut c = Criterion::default().configure_from_args();
    bench_agent_per_edit(&mut c);
    bench_lsp_per_edit(&mut c);
    c.final_summary();

    // 2. Long-horizon RSS gate — spec §11.6.
    let outcomes = [run_agent_workload(), run_lsp_workload()];

    let mut any_failed = false;
    println!();
    println!(
        "=== bench_incremental RSS gate (spec §11.6, ±{:.0} %) ===",
        RSS_TOLERANCE * 100.0
    );
    for o in &outcomes {
        match (o.baseline_bytes, o.steady_bytes, o.drift()) {
            (Some(base), Some(steady), Some(drift)) => {
                let verdict = if drift.abs() > RSS_TOLERANCE {
                    "FAIL"
                } else {
                    "ok"
                };
                println!(
                    "  [{verdict}] {mode:<22} baseline={base_mib:>6.1} MiB  \
                     steady={steady_mib:>6.1} MiB  drift={drift_pct:+.2} %",
                    verdict = verdict,
                    mode = o.mode,
                    base_mib = base as f64 / (1024.0 * 1024.0),
                    steady_mib = steady as f64 / (1024.0 * 1024.0),
                    drift_pct = drift * 100.0,
                );
                if drift.abs() > RSS_TOLERANCE {
                    any_failed = true;
                    eprintln!(
                        "  bench_incremental: {} RSS drift {:+.2}% exceeds ±{:.0}% band",
                        o.mode,
                        drift * 100.0,
                        RSS_TOLERANCE * 100.0,
                    );
                }
            }
            _ => {
                println!(
                    "  [skip] {mode:<22} RSS unavailable on this platform",
                    mode = o.mode
                );
            }
        }
    }

    if any_failed {
        // Non-zero exit propagates to CI even though criterion itself is happy.
        std::process::exit(1);
    }
}
