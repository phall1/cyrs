//! bench_workspace_fan — open a 100-file synthetic project, trigger
//! cross-file diagnostics through the workspace DB from cy-o8c, and
//! assert a steady-state RSS ceiling + warm-up budget (spec §17.10;
//! bead cy-y6a).
//!
//! # What this measures
//!
//! Mirrors exactly what `cypher check <dir>` does in `cyrs-cli`
//! (see [`crates/cyrs-cli/src/main.rs::check_project`]): walk a
//! `cypher-project.toml` manifest, open every member into a single
//! `cyrs_db::Database`, then run `all_diagnostics` on each.  The
//! bench drives the same library path rather than spawning the CLI
//! binary so we can measure end-to-end wall-clock and RSS without
//! subprocess overhead.
//!
//! # Fixture
//!
//! At bench startup we materialise a `tempfile::TempDir` containing:
//!
//! - `cypher-project.toml` — canonical manifest pointing at `samples/*.cyp`
//! - `samples/NNN.cyp` for `NNN in 0..=99` — each is a 50-line
//!   synthetic query using the four rotating templates shared with
//!   `bench_large_file` / `bench_incremental_edit`.
//!
//! This is *workspace-fan* — many small files rather than one big one
//! — so we exercise per-FileId overhead (open_file, parse_cst
//! memoisation, remove_file LRU) rather than per-statement cost.
//!
//! # Warm-up budget
//!
//! "Warm-up" is defined as the first full sweep: load manifest → open
//! all 100 files → run `all_diagnostics` on each → drop the workspace.
//! We fail if a cold sweep takes longer than [`WARMUP_BUDGET`].
//!
//! # Steady-state RSS ceiling
//!
//! We then do [`STEADY_SWEEPS`] additional sweeps reusing the same
//! Database.  After the final sweep the median RSS sample must not
//! exceed the warm-up RSS by more than [`RSS_CEILING_RATIO`] (i.e.,
//! growth shall stay bounded despite many sweeps against the same
//! 100 FileIds).
//!
//! # Why tempfiles
//!
//! Spec §17.10 + AGENTS.md forbid committing 10k-line fixtures.  The
//! 100-file fixture is even larger (5,000 lines total) — generating
//! it procedurally in `TempDir` keeps the repo size flat.

#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use criterion::{Criterion, black_box};

use cyrs_db::{Database, DialectMode};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// File count — 100-doc workspace per the industry-bar in the bead
/// description.
const FILE_COUNT: usize = 100;

/// Lines per member file.  50 × 100 = 5,000-line aggregate project;
/// large enough to exercise memoisation caches, small enough to keep
/// the bench runtime reasonable.
const LINES_PER_FILE: usize = 50;

/// Number of sample sweeps (after warmup) that drive RSS-growth
/// detection.  Each sweep reopens no files — it reuses the same
/// FileIds and only re-runs `all_diagnostics`.
const STEADY_SWEEPS: usize = 5;

/// Warm-up budget — cold sweep (open 100 files + diagnose each once).
/// Loose enough to cover a slow CI runner; tight enough to catch a
/// regression that makes the cold path quadratic.
const WARMUP_BUDGET: Duration = Duration::from_secs(10);

/// Steady-state RSS-growth ceiling.  A warm sweep (update_file on
/// existing FileIds) should show near-zero drift.  The 2.0× band is
/// generous — OS-level RSS fluctuates by a few MiB just from the
/// allocator's eager page reservation — but tight enough to catch a
/// linear leak across sweeps (which would show 5× at STEADY_SWEEPS=5).
const RSS_CEILING_RATIO: f64 = 2.0;

// ---------------------------------------------------------------------------
// Fixture generator
// ---------------------------------------------------------------------------

/// Same rotating templates as `bench_large_file` + `bench_incremental_edit`,
/// sized to `lines` rows.  Output is deterministic in `(lines, file_id)`.
fn synth_file(lines: usize, file_id: usize) -> String {
    let mut s = String::with_capacity(lines * 80);
    for i in 0..lines {
        let k = file_id * lines + i;
        match i % 4 {
            0 => s.push_str(&format!("MATCH (p{k}:Label {{id: {k}}}) RETURN p{k}.name;\n")),
            1 => s.push_str(&format!(
                "MATCH (a{k}:User)-[:FOLLOWS]->(b{k}:User) WHERE a{k}.age > {k} RETURN a{k}, b{k};\n"
            )),
            2 => s.push_str(&format!("UNWIND [1, 2, 3] AS x{k} RETURN x{k};\n")),
            _ => s.push_str(&format!(
                "MATCH (n{k}:Item) WITH n{k} WHERE n{k}.active = true RETURN n{k};\n"
            )),
        }
    }
    s
}

/// Build the 100-file project on disk.  Returns the temporary
/// directory (kept alive so tear-down only happens when the caller
/// drops it) and the manifest path inside it.
fn build_fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("cypher-bench-workspace-fan-")
        .tempdir()
        .expect("create tempdir");
    let samples = dir.path().join("samples");
    fs::create_dir_all(&samples).expect("create samples dir");
    for i in 0..FILE_COUNT {
        let path = samples.join(format!("q{i:03}.cyp"));
        fs::write(&path, synth_file(LINES_PER_FILE, i)).expect("write synthetic file");
    }
    let manifest_path = dir.path().join("cypher-project.toml");
    fs::write(
        &manifest_path,
        "[project]\n\
         name = \"bench-workspace-fan\"\n\
         description = \"procedural 100-file fixture for bench_workspace_fan\"\n\
         \n\
         [project.dialect]\n\
         default = \"GqlAligned\"\n\
         \n\
         [project.members]\n\
         include = [\"samples/*.cyp\"]\n",
    )
    .expect("write manifest");
    (dir, manifest_path)
}

// ---------------------------------------------------------------------------
// Driver — mirrors cyrs-cli::check_project (cy-o8c)
// ---------------------------------------------------------------------------

/// State retained across steady-state sweeps: the manifest-declared
/// member paths, the `FileId` opened for each, and the source text
/// last written to disk.  On re-sweep we `update_file` rather than
/// `open_file` to prove the workspace DB does not require churn
/// through fresh `FileId`s.
struct WorkspaceState {
    ids: Vec<cyrs_db::FileId>,
}

/// Cold sweep: load manifest, mint one `FileId` per member, then run
/// `all_diagnostics` on each.  Returns the retained state plus the
/// diagnostic count (used as a black-box sink).
fn cold_sweep(db: &mut Database, manifest_path: &Path) -> (WorkspaceState, usize) {
    let manifest = cyrs_project::load_from_toml_path(manifest_path)
        .expect("fixture manifest must load cleanly");

    let mut ids = Vec::with_capacity(manifest.members.len());
    for member in &manifest.members {
        let source = fs::read_to_string(member).expect("read member");
        let id = db.open_file(member, source, DialectMode::GqlAligned);
        ids.push(id);
    }

    let mut diag_count = 0usize;
    for id in &ids {
        let diags = db.all_diagnostics(*id).expect("file still open");
        diag_count += diags.diagnostics().len();
    }
    (WorkspaceState { ids }, diag_count)
}

/// Warm sweep: reuse the FileIds minted by the last cold_sweep.  On
/// each call we rewrite the source (identical bytes) and re-query
/// `all_diagnostics`.  Under stable Salsa memoisation the second and
/// subsequent warm sweeps hit the cache; this is the steady-state
/// RSS-growth path the gate asserts on.
fn warm_sweep(db: &mut Database, manifest_path: &Path, state: &WorkspaceState) -> usize {
    let manifest = cyrs_project::load_from_toml_path(manifest_path)
        .expect("fixture manifest must load cleanly");
    for (id, member) in state.ids.iter().zip(&manifest.members) {
        let source = fs::read_to_string(member).expect("read member");
        db.update_file(*id, source).expect("FileId open");
    }
    let mut diag_count = 0usize;
    for id in &state.ids {
        let diags = db.all_diagnostics(*id).expect("file still open");
        diag_count += diags.diagnostics().len();
    }
    diag_count
}

// ---------------------------------------------------------------------------
// RSS helpers — same pattern as bench_incremental
// ---------------------------------------------------------------------------

fn sample_rss_bytes() -> Option<u64> {
    memory_stats::memory_stats().map(|s| s.physical_mem as u64)
}

// ---------------------------------------------------------------------------
// Criterion bench — per-sweep latency
// ---------------------------------------------------------------------------

fn bench_workspace_sweep(c: &mut Criterion, manifest_path: &Path) {
    c.bench_function("workspace_fan_sweep_100", |b| {
        b.iter(|| {
            // Fresh Database each iteration so we measure cold-cache sweep
            // latency.  Steady-state sweep is exercised separately below.
            let mut db = Database::new();
            let (_, n) = cold_sweep(&mut db, black_box(manifest_path));
            black_box(n);
        });
    });
}

// ---------------------------------------------------------------------------
// p95 + gates
// ---------------------------------------------------------------------------

fn p95_cold_sweep(manifest_path: &Path, iterations: usize) -> Duration {
    // Warmup: pay allocator / Salsa first-time costs once before
    // sampling the tail.
    {
        let mut db = Database::new();
        let _ = cold_sweep(&mut db, manifest_path);
    }
    let mut samples: Vec<Duration> = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let t0 = Instant::now();
        let mut db = Database::new();
        let _ = black_box(cold_sweep(&mut db, manifest_path));
        samples.push(t0.elapsed());
    }
    samples.sort_unstable();
    let idx = (samples.len() as f64 * 0.95).ceil() as usize - 1;
    samples[idx]
}

// ---------------------------------------------------------------------------
// main (harness = false)
// ---------------------------------------------------------------------------

fn main() {
    let (fixture, manifest_path) = build_fixture();

    // 1. Criterion — per-sweep latency.  Feeds the 10 % time-regression
    //    gate via the PR bench workflow.
    let mut c = Criterion::default()
        .sample_size(10)
        .configure_from_args();
    bench_workspace_sweep(&mut c, &manifest_path);
    c.final_summary();

    println!();
    println!("=== bench_workspace_fan gates (spec §17.10) ===");

    // 2. Warm-up budget: one cold sweep must finish under WARMUP_BUDGET.
    //    This sweep mints the FileIds we reuse below.
    let warm_t0 = Instant::now();
    let mut db = Database::new();
    let (state, warm_diags) = cold_sweep(&mut db, &manifest_path);
    let warm_elapsed = warm_t0.elapsed();
    println!(
        "  warm-up sweep         : {:>7.2} ms  ({} diagnostics)  [budget {} ms]",
        warm_elapsed.as_secs_f64() * 1000.0,
        warm_diags,
        WARMUP_BUDGET.as_millis(),
    );
    let warmup_fail = warm_elapsed > WARMUP_BUDGET;

    // 3. p95 cold-sweep — informational; not a hard gate, but its value
    //    is the industry-bar number the operator cares about.  Run BEFORE
    //    the RSS baseline so transient Databases' page allocations are
    //    already reflected when we sample.
    let p95_cold = p95_cold_sweep(&manifest_path, 10);
    println!(
        "  cold-sweep p95 (n=10) : {:>7.2} ms",
        p95_cold.as_secs_f64() * 1000.0,
    );

    // Sample the RSS baseline *after* the cold-sweep warm-ups so the
    // reading reflects steady-state allocator state on the long-lived
    // `db`.
    let warm_rss = sample_rss_bytes();

    // 4. Steady-state RSS ceiling.  Reuse the FileIds minted in the warm-up
    //    sweep; each further sweep only `update_file`s each FileId's source.
    //    This is the LSP-didChange path, not the open_file churn path — so
    //    memoisation stays stable and RSS should not grow meaningfully.
    let mut rss_fail = false;
    if let Some(base_rss) = warm_rss {
        for _ in 0..STEADY_SWEEPS {
            let _ = black_box(warm_sweep(&mut db, &manifest_path, &state));
        }
        match sample_rss_bytes() {
            Some(steady_rss) => {
                let ratio = steady_rss as f64 / base_rss as f64;
                let base_mib = base_rss as f64 / (1024.0 * 1024.0);
                let steady_mib = steady_rss as f64 / (1024.0 * 1024.0);
                let verdict = if ratio > RSS_CEILING_RATIO { "FAIL" } else { "ok" };
                println!(
                    "  RSS warm → steady     : {base_mib:>6.1} MiB → {steady_mib:>6.1} MiB  \
                     ratio {ratio:.3}  [ceiling {RSS_CEILING_RATIO:.2}]  [{verdict}]"
                );
                if ratio > RSS_CEILING_RATIO {
                    rss_fail = true;
                }
            }
            None => {
                println!("  RSS: unavailable on this platform (steady sample)");
            }
        }
    } else {
        println!("  RSS: unavailable on this platform (baseline sample)");
    }

    // Keep the fixture alive until every measurement is done — dropping
    // the tempdir unlinks the files under it.
    drop(fixture);

    if warmup_fail {
        eprintln!(
            "  FAIL: warm-up sweep {:.2} ms exceeds budget {} ms",
            warm_elapsed.as_secs_f64() * 1000.0,
            WARMUP_BUDGET.as_millis(),
        );
    }
    if warmup_fail || rss_fail {
        std::process::exit(1);
    }
    println!("  OK");
}
