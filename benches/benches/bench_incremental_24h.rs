//! bench_incremental_24h — 24-hour RSS soak (spec §11.6, §17.10; bead cy-wcv).
//!
//! # What this covers
//!
//! Long-horizon sibling of `bench_incremental` (cy-bh5) and
//! `bench_incremental_edit` (cy-y6a).  Drives open/edit/close cycles
//! continuously for `CY_SOAK_HOURS` hours and samples resident-set size
//! (RSS) on a fixed schedule, then asserts that the steady-state trend is
//! a plateau (no slow leak).
//!
//! Two modes run sequentially — both are required to keep the soak honest:
//!
//! 1. **Agent single-FileId.**  One long-lived `FileId` re-edited forever.
//!    Exercises the Salsa per-query LRU caps (cy-31b) under sustained
//!    revision churn: the memo tables should converge to a bounded
//!    working set.
//! 2. **LSP FileId churn.**  `didClose`-driven eviction (cy-it7): a
//!    fresh `FileId` is opened, analysed, and dropped each cycle.
//!    Exercises the per-FileId reclamation path; a leaked FileId table
//!    here would show up as monotonic RSS growth over 8+ hours.
//!
//! # Horizon configuration
//!
//! - `CY_SOAK_HOURS` env var selects the total soak duration (float; fractional
//!   hours allowed for smoke runs, e.g. `CY_SOAK_HOURS=0.25`).  Default: 24.
//!   The weekly CI job sets this to 4, which is enough to see the plateau while
//!   staying under the GitHub Actions 6h ceiling.  A full 24h run is
//!   operator-triggered via the `workflow_dispatch` input on
//!   `.github/workflows/nightly-benches.yml`.
//! - The per-hour slope gate is *only enforced* when `CY_SOAK_HOURS >= 4`.
//!   Shorter runs are smoke-only: they exercise wiring and produce a
//!   representative baseline but their slope is too noisy to gate.
//!
//! # The slope gate
//!
//! Spec §11.6 fixes the steady-state-RSS bound at ±10 % over the lifetime
//! of the process.  This bench tightens that into a *rate*: if the linear
//! regression of RSS samples over the last 8 h has slope ≥
//! [`LEAK_SLOPE_KIB_PER_HR`] KiB/hr, that is a slow leak and the bench
//! exits non-zero.  The 100 KiB/hr threshold was chosen so:
//!
//! - A genuine 0 %/hr process (no leak) lands at 0 ± noise — every run
//!   observed < 10 KiB/hr of jitter on steady-state idle.
//! - A 1 MiB/day leak (≈ 43 KiB/hr) clears the gate at the safe side.
//! - A 10 MiB/day leak (≈ 428 KiB/hr) trips it decisively in ~4 h.
//!
//! # Sampling cadence
//!
//! RSS is sampled every `RSS_SAMPLE_INTERVAL_LONG` for long runs (≥ 1 h)
//! and every `RSS_SAMPLE_INTERVAL_SHORT` for smoke runs (< 1 h).  We
//! store `(elapsed_hours, rss_bytes)` pairs and regress a line over the
//! sliding "last 8 hours" window.
//!
//! # Why `harness = false`
//!
//! Criterion measures time-per-iter, not absolute memory vs wall-clock.
//! This bench is a memory-evolution trace, so we use a custom `main`
//! that still respects `cargo bench --no-run` (the gate only requires
//! the binary to *build*).  A 24h bench cannot be part of
//! `cargo xtask gate`; see the bead hard constraints.

#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use std::env;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cyrs_db::{Database, DialectMode};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default soak horizon, in hours.  Full 24h bound matches the bead.
const DEFAULT_SOAK_HOURS: f64 = 24.0;

/// Hours required to enable the slope gate.  The weekly CI job runs at
/// `CY_SOAK_HOURS=4`; anything shorter is a wiring smoke-test.
const GATE_MIN_HOURS: f64 = 4.0;

/// Warm-up window before we start collecting gated samples.  Matches
/// `bench_incremental`'s 1k-edits baseline intent at a wall-clock scale:
/// for a 4h run we discard the first hour, for 24h the first 4h.
const WARMUP_FRACTION: f64 = 0.25;

/// Trailing window over which the slope gate is computed.  Spec §11.6
/// asks for a plateau; we enforce it over the most recent 8 hours
/// (or `min(8h, post-warmup duration)` for shorter runs).
const SLOPE_WINDOW_HOURS: f64 = 8.0;

/// Leak-detection threshold.  See module docs for the derivation.
const LEAK_SLOPE_KIB_PER_HR: f64 = 100.0;

/// RSS sample interval for `CY_SOAK_HOURS >= 1.0` — 5 minutes matches the
/// bead acceptance criteria.
const RSS_SAMPLE_INTERVAL_LONG: Duration = Duration::from_secs(5 * 60);

/// RSS sample interval for short smoke runs (`CY_SOAK_HOURS < 1.0`) —
/// 60 s keeps enough points for a visible trend without dominating the
/// bench's own wall-clock.
const RSS_SAMPLE_INTERVAL_SHORT: Duration = Duration::from_secs(60);

/// Representative multi-clause source, identical to `bench_incremental`
/// so the two benches compare apples-to-apples on per-edit cost.
const BASE_QUERY: &str = "\
MATCH (n:Person {name: $name})-[r:KNOWS*1..3]->(m:Person)
WHERE m.age > 30 AND m.active = true
WITH n, m, r, count(*) AS hops
ORDER BY hops ASC
LIMIT 100
RETURN n.name AS source, m.name AS target, hops
";

// ---------------------------------------------------------------------------
// Env-var plumbing
// ---------------------------------------------------------------------------

/// Resolve `CY_SOAK_HOURS`.  Invalid values fall back to the default
/// with a warning on stderr; we never silently change shape under the
/// operator.
fn soak_hours() -> f64 {
    match env::var("CY_SOAK_HOURS") {
        Ok(raw) => match raw.trim().parse::<f64>() {
            Ok(h) if h > 0.0 && h.is_finite() => h,
            _ => {
                eprintln!(
                    "bench_incremental_24h: CY_SOAK_HOURS={raw:?} not a positive finite \
                     float, falling back to {DEFAULT_SOAK_HOURS}h"
                );
                DEFAULT_SOAK_HOURS
            }
        },
        Err(_) => DEFAULT_SOAK_HOURS,
    }
}

// ---------------------------------------------------------------------------
// RSS sampling
// ---------------------------------------------------------------------------

fn sample_rss_bytes() -> Option<u64> {
    memory_stats::memory_stats().map(|s| s.physical_mem as u64)
}

// ---------------------------------------------------------------------------
// Edit shape — identical to bench_incremental so regressions localise
// ---------------------------------------------------------------------------

#[inline]
fn edit_source(round: u64) -> String {
    let mut s = String::with_capacity(BASE_QUERY.len() + 24);
    s.push_str(BASE_QUERY);
    s.push_str("// edit-");
    s.push_str(&format!("{round:010}"));
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// Time-series + linear regression
// ---------------------------------------------------------------------------

/// One (`elapsed_hours`, `rss_bytes`) sample.
#[derive(Clone, Copy, Debug)]
struct RssSample {
    hours: f64,
    bytes: u64,
}

/// Ordinary least-squares slope of `y = a + b*x` over `samples`.  Returns
/// `None` if fewer than two distinct x values (degenerate regression).
/// Result is in `bytes / hour`.
fn ols_slope_bytes_per_hr(samples: &[RssSample]) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }
    let n = samples.len() as f64;
    let sum_x: f64 = samples.iter().map(|s| s.hours).sum();
    let sum_y: f64 = samples.iter().map(|s| s.bytes as f64).sum();
    let mean_x = sum_x / n;
    let mean_y = sum_y / n;

    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for s in samples {
        let dx = s.hours - mean_x;
        num += dx * (s.bytes as f64 - mean_y);
        den += dx * dx;
    }
    if den.abs() < f64::EPSILON {
        return None;
    }
    Some(num / den)
}

// ---------------------------------------------------------------------------
// The soak driver
// ---------------------------------------------------------------------------

/// How this round's edit should be applied to the database.
enum Mode {
    /// Single long-lived FileId — agent-style.
    Agent,
    /// Fresh FileId per cycle with explicit remove — LSP-style churn.
    LspChurn,
}

impl Mode {
    fn label(&self) -> &'static str {
        match self {
            Mode::Agent => "agent single-FileId",
            Mode::LspChurn => "LSP FileId churn",
        }
    }
}

/// Run one mode for `total` wall-clock time, sampling RSS every
/// `sample_interval`.  Returns the full sample trace.
fn run_soak(mode: &Mode, total: Duration, sample_interval: Duration) -> Vec<RssSample> {
    let start = Instant::now();
    let mut next_sample_at = start + sample_interval;
    let mut samples: Vec<RssSample> = Vec::new();

    // Take an immediate baseline sample at t=0 so short smoke runs still
    // produce >=2 points.
    if let Some(b) = sample_rss_bytes() {
        samples.push(RssSample {
            hours: 0.0,
            bytes: b,
        });
    }

    match mode {
        Mode::Agent => {
            let mut db = Database::new();
            let id = db.open_file(
                Path::new("soak_agent.cyp"),
                edit_source(0),
                DialectMode::GqlAligned,
            );
            let _ = black_box(db.analyse_file(id));

            let mut round: u64 = 1;
            while start.elapsed() < total {
                db.update_file(id, edit_source(round))
                    .expect("agent FileId stays open through the soak");
                let _ = black_box(db.analyse_file(id));
                round = round.wrapping_add(1);

                let now = Instant::now();
                if now >= next_sample_at {
                    if let Some(b) = sample_rss_bytes() {
                        samples.push(RssSample {
                            hours: (now - start).as_secs_f64() / 3600.0,
                            bytes: b,
                        });
                    }
                    // Drift-free schedule: advance by whole intervals so
                    // samples land at 0, 5m, 10m, …, not at the mercy of
                    // per-edit jitter.
                    while next_sample_at <= now {
                        next_sample_at += sample_interval;
                    }
                }
            }
        }
        Mode::LspChurn => {
            let mut db = Database::new();
            let mut path_buf = PathBuf::from("soak-lsp-000000.cyp");

            let mut round: u64 = 0;
            while start.elapsed() < total {
                path_buf.set_file_name(format!("soak-lsp-{round:010}.cyp"));
                let id = db.open_file(
                    path_buf.as_path(),
                    edit_source(round),
                    DialectMode::GqlAligned,
                );
                let _ = black_box(db.analyse_file(id));
                db.remove_file(id)
                    .expect("fresh FileId is always open at close time");
                round = round.wrapping_add(1);

                let now = Instant::now();
                if now >= next_sample_at {
                    if let Some(b) = sample_rss_bytes() {
                        samples.push(RssSample {
                            hours: (now - start).as_secs_f64() / 3600.0,
                            bytes: b,
                        });
                    }
                    while next_sample_at <= now {
                        next_sample_at += sample_interval;
                    }
                }
            }
        }
    }

    // Always take a final sample — the last completed interval may not
    // have fired before the loop exited.
    if let Some(b) = sample_rss_bytes() {
        let h = start.elapsed().as_secs_f64() / 3600.0;
        match samples.last() {
            Some(last) if (h - last.hours).abs() < f64::EPSILON => {}
            _ => samples.push(RssSample { hours: h, bytes: b }),
        }
    }

    samples
}

// ---------------------------------------------------------------------------
// Reporting + gate
// ---------------------------------------------------------------------------

struct ModeReport {
    mode: &'static str,
    samples: Vec<RssSample>,
    slope_bytes_per_hr_tail: Option<f64>,
    tail_start_hours: f64,
}

/// Fit the trailing `SLOPE_WINDOW_HOURS` (or everything past warmup,
/// whichever is shorter) and print per-mode stats.
fn analyse(samples: Vec<RssSample>, mode: &'static str, total_hours: f64) -> ModeReport {
    let warmup_hours = total_hours * WARMUP_FRACTION;
    let window_hours = SLOPE_WINDOW_HOURS.min(total_hours - warmup_hours).max(0.0);
    let tail_start = (total_hours - window_hours).max(warmup_hours);
    let tail: Vec<RssSample> = samples
        .iter()
        .copied()
        .filter(|s| s.hours >= tail_start)
        .collect();

    let slope = ols_slope_bytes_per_hr(&tail);
    ModeReport {
        mode,
        samples,
        slope_bytes_per_hr_tail: slope,
        tail_start_hours: tail_start,
    }
}

fn print_report(report: &ModeReport, total_hours: f64) {
    let last = report.samples.last();
    let first = report.samples.first();
    println!();
    println!(
        "--- {} — {} samples over {:.2} h ---",
        report.mode,
        report.samples.len(),
        total_hours
    );
    if let (Some(f), Some(l)) = (first, last) {
        println!(
            "  t=0.00 h  RSS = {:>7.1} MiB",
            f.bytes as f64 / (1024.0 * 1024.0)
        );
        println!(
            "  t={:>4.2} h  RSS = {:>7.1} MiB  (Δ = {:+.1} MiB)",
            l.hours,
            l.bytes as f64 / (1024.0 * 1024.0),
            (l.bytes as f64 - f.bytes as f64) / (1024.0 * 1024.0),
        );
    }
    match report.slope_bytes_per_hr_tail {
        Some(s) => {
            let kib_per_hr = s / 1024.0;
            println!(
                "  tail slope (from t={:.2} h): {:+.2} KiB/hr  (gate: |slope| < {:.0} KiB/hr)",
                report.tail_start_hours, kib_per_hr, LEAK_SLOPE_KIB_PER_HR
            );
        }
        None => println!(
            "  tail slope: insufficient samples past warmup (tail_start={:.2} h)",
            report.tail_start_hours
        ),
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let total_hours = soak_hours();
    let total = Duration::from_secs_f64(total_hours * 3600.0);
    let sample_interval = if total_hours >= 1.0 {
        RSS_SAMPLE_INTERVAL_LONG
    } else {
        RSS_SAMPLE_INTERVAL_SHORT
    };
    let gate_enabled = total_hours >= GATE_MIN_HOURS;

    println!("=== bench_incremental_24h (spec §11.6, §17.10, bead cy-wcv) ===");
    println!(
        "  CY_SOAK_HOURS   = {total_hours}  (default {DEFAULT_SOAK_HOURS}, gate min {GATE_MIN_HOURS})"
    );
    println!(
        "  sample interval = {}s  (leak gate threshold: {LEAK_SLOPE_KIB_PER_HR:.0} KiB/hr over \
         last {SLOPE_WINDOW_HOURS:.0} h)",
        sample_interval.as_secs()
    );
    println!("  slope gate      = {}", if gate_enabled { "ENABLED" } else { "skipped (smoke)" });

    // Platform guard — if the host cannot report RSS we cannot meaningfully
    // gate.  Still run the workloads so the build + wiring is exercised.
    if sample_rss_bytes().is_none() {
        eprintln!(
            "bench_incremental_24h: memory_stats unavailable on this platform; gate skipped"
        );
    }

    // Agent mode first — the LRU caps are the more common leak vector,
    // so we want its samples first in the report.
    let agent_samples = run_soak(&Mode::Agent, total, sample_interval);
    let agent = analyse(agent_samples, Mode::Agent.label(), total_hours);
    print_report(&agent, total_hours);

    let lsp_samples = run_soak(&Mode::LspChurn, total, sample_interval);
    let lsp = analyse(lsp_samples, Mode::LspChurn.label(), total_hours);
    print_report(&lsp, total_hours);

    if !gate_enabled {
        println!();
        println!(
            "bench_incremental_24h: CY_SOAK_HOURS={total_hours} < {GATE_MIN_HOURS}; \
             slope gate skipped (smoke-only run)"
        );
        return;
    }

    // Slope gate — any mode slope magnitude above the leak threshold is a
    // blocking regression.
    let mut failed = false;
    for report in [&agent, &lsp] {
        match report.slope_bytes_per_hr_tail {
            Some(slope) => {
                let kib = slope / 1024.0;
                if kib.abs() >= LEAK_SLOPE_KIB_PER_HR {
                    eprintln!(
                        "FAIL: {} tail slope {:+.2} KiB/hr ≥ {:.0} KiB/hr — slow leak",
                        report.mode, kib, LEAK_SLOPE_KIB_PER_HR
                    );
                    failed = true;
                }
            }
            None => {
                eprintln!(
                    "FAIL: {} produced no tail samples — gate cannot run; investigate \
                     sampler or soak duration",
                    report.mode
                );
                failed = true;
            }
        }
    }

    if failed {
        std::process::exit(1);
    }
    println!();
    println!("PASS: both modes within ±{LEAK_SLOPE_KIB_PER_HR:.0} KiB/hr over last {SLOPE_WINDOW_HOURS:.0} h");
}
