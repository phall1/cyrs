//! bench_incremental_edit — 1000 single-char edits across a 1k-line fixture,
//! with a sub-linear-scaling gate (spec §17.10; beads cy-y6a, cy-zv0).
//!
//! # What this measures
//!
//! An editor / agent making 1000 tiny edits at varying positions in a
//! 1000-line query.  After each edit we ask [`Database::analyse_file`] to
//! rebuild diagnostics — this is the exact path `textDocument/didChange`
//! takes once the LSP notification has been translated to an
//! `edit_file(TextEdit)` call.
//!
//! The ideal invariant is that steady-state per-edit reanalysis time is
//! *sub-linear* in file size.  cy-zv0 introduced the
//! `Database::edit_file(TextEdit)` API plus `cypher_syntax::incremental_reparse`,
//! but the underlying reparse is still a whole-file fallback (the API-first
//! tranche of Option A).  Until the smart sub-tree splicer lands in a
//! follow-up bead, the observed scaling ratio sits near 2.0 and the budget
//! is set to [`LINEAR_BUDGET`] accordingly.
//!
//! # Bench-harness hook required for a sub-linear ratio
//!
//! The API surface is now in place — `Database::edit_file` routes through
//! `cypher_syntax::incremental_reparse`.  What remains is the smart
//! implementation:
//!
//! 1. Identify the covering sub-tree for the edit range
//!    (`SyntaxNode::covering_element`).
//! 2. Re-lex and reparse only that sub-span.
//! 3. Splice the new green sub-tree into the old root via
//!    `GreenNode::replace_child`.
//!
//! Landing those three steps flips the ratio below 2.0 without any bench
//! or caller change.  Tracked as the cy-zv0 smart-path follow-up.
//!
//! # What the bench measures today
//!
//! After a short warmup, we compute:
//!
//! - `p50_1k` = median per-edit reanalyse time on a 1k-line fixture.
//! - `p50_2k` = median per-edit reanalyse time on a 2k-line fixture.
//!
//! The ratio `p50_2k / p50_1k` is printed.  The ratio must stay under
//! [`LINEAR_BUDGET`] — effectively "don't regress into super-linear"
//! — which catches e.g. a quadratic sema loop without blocking on
//! the incremental-edit driver gap above.
//!
//! # Edit shape
//!
//! Each round picks a byte offset inside the source, appends one
//! ASCII space character at that offset, and calls
//! [`Database::update_file`] with the new source.  Spaces are always
//! valid wherever tokens already had whitespace, so the fixture
//! stays parseable and we measure the pipeline under realistic churn
//! rather than under degenerate (always-the-same-source) conditions.
//!
//! Random positions are drawn from a deterministic LCG seeded with a
//! constant so runs are reproducible across machines and rebases.

#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use criterion::Criterion;

use cypher_db::{Database, DialectMode};
use cypher_syntax::{TextEdit, TextSize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Primary fixture size — 1,000 lines matches the industry-bar editor
/// workload in the bead description.
const LINES_1K: usize = 1_000;

/// Secondary fixture size — 2× the primary, used to compute the
/// sub-linear scaling ratio.
const LINES_2K: usize = 2_000;

/// Number of edits in each gate run.  1k is enough to get a stable
/// median without ballooning the bench runtime.
const EDITS: usize = 1_000;

/// Warmup edits before we start sampling — pays the first-time Salsa
/// input-interning + allocator-reserve costs so the measured window
/// reflects steady-state behaviour.
const WARMUP_EDITS: usize = 50;

/// Linear-or-better scaling ceiling.  At exactly 2× lines, a fully linear
/// per-edit cost yields ratio 2.0.  cy-zv0 landed the `edit_file` API but
/// the underlying `incremental_reparse` is still a whole-file fallback,
/// so in practice the ratio sits near 2.0.  The budget is tightened from
/// 2.5× (cy-y6a) to 2.0× to catch any super-linear regression; pushing
/// below requires the smart sub-tree splicer tracked as a cy-zv0 follow-up.
const LINEAR_BUDGET: f64 = 2.0;

// ---------------------------------------------------------------------------
// Deterministic fixture + RNG
// ---------------------------------------------------------------------------

/// Produce an `n`-line synthetic Cypher source.  Every line is a
/// self-contained statement so the parser produces exactly `n`
/// STATEMENT nodes and edits stay locally bounded.
fn synth_source(n: usize) -> String {
    let mut s = String::with_capacity(n * 64);
    for i in 0..n {
        // Four rotating templates, exactly like bench_large_file, so the
        // two benches compare apples-to-apples.
        match i % 4 {
            0 => s.push_str(&format!("MATCH (p{i}:Label {{id: {i}}}) RETURN p{i}.name;\n")),
            1 => s.push_str(&format!(
                "MATCH (a{i}:User)-[:FOLLOWS]->(b{i}:User) WHERE a{i}.age > {i} RETURN a{i}, b{i};\n"
            )),
            2 => s.push_str(&format!("UNWIND [1, 2, 3] AS x{i} RETURN x{i};\n")),
            _ => s.push_str(&format!(
                "MATCH (n{i}:Item) WITH n{i} WHERE n{i}.active = true RETURN n{i};\n"
            )),
        }
    }
    s
}

/// Classic 64-bit LCG (Numerical Recipes).  Deterministic; no `rand`
/// dependency.  Returns the next state + a 32-bit draw.
fn lcg_next(state: u64) -> (u64, u32) {
    let next = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((next), (next >> 33) as u32)
}

/// Build a single-character insertion [`TextEdit`] at a (deterministic)
/// byte offset inside a source of length `source_len`.  We always insert
/// a single space at a char boundary so the resulting source is valid
/// UTF-8 and still parses.
///
/// The bench fixture is ASCII-only, so every byte is a char boundary;
/// `TextEdit::apply` handles the edge case internally if a future
/// fixture ever includes multi-byte text.
fn pick_edit(source_len: usize, rng_state: &mut u64) -> TextEdit {
    let (next, draw) = lcg_next(*rng_state);
    *rng_state = next;
    let pos = (draw as usize) % source_len.max(1);
    TextEdit::insert(TextSize::new(pos as u32), " ")
}

/// Bytes a `TextEdit` would add to the source (net). Used by the bench
/// driver to keep a running source-length estimate so edit offsets stay
/// in range without re-reading the full source from the DB each edit.
fn edit_net_bytes(edit: &TextEdit) -> isize {
    let old_len = u32::from(edit.range.end()) - u32::from(edit.range.start());
    edit.replacement.len() as isize - old_len as isize
}

// ---------------------------------------------------------------------------
// Core driver
// ---------------------------------------------------------------------------

/// Run `EDITS` single-character edits against a source of `n` lines;
/// return the median per-edit wall-clock latency.
///
/// Uses the cy-zv0 `Database::edit_file(TextEdit)` API rather than
/// `update_file(String)` so the bench exercises the incremental-edit
/// path exactly as `textDocument/didChange` will.  Today both paths
/// bottom out on a whole-file reparse; the numbers diverge once the
/// smart path lands.
fn median_edit_latency(lines: usize) -> Duration {
    let source = synth_source(lines);
    let mut source_len = source.len() as isize;

    let mut db = Database::new();
    let id = db.open_file(
        Path::new("incremental_edit.cyp"),
        source,
        DialectMode::GqlAligned,
    );
    // Prime the pipeline so first-edit fixed costs don't skew samples.
    let _ = black_box(db.analyse_file(id));

    let mut rng_state: u64 = 0x1234_5678_9abc_def0;

    // Warmup — edits whose timings we discard.
    for _ in 0..WARMUP_EDITS {
        let edit = pick_edit(source_len as usize, &mut rng_state);
        source_len += edit_net_bytes(&edit);
        db.edit_file(id, &edit)
            .expect("FileId stays open through the workload");
        let _ = black_box(db.analyse_file(id));
    }

    // Measured window.
    let mut samples: Vec<Duration> = Vec::with_capacity(EDITS);
    for _ in 0..EDITS {
        let edit = pick_edit(source_len as usize, &mut rng_state);
        source_len += edit_net_bytes(&edit);
        let t0 = Instant::now();
        db.edit_file(id, &edit)
            .expect("FileId stays open through the workload");
        let _ = black_box(db.analyse_file(id));
        samples.push(t0.elapsed());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

// ---------------------------------------------------------------------------
// Criterion bench — per-edit reanalysis latency, 1k-line fixture
// ---------------------------------------------------------------------------

fn bench_single_char_edit_1k(c: &mut Criterion) {
    let mut db = Database::new();
    let source = synth_source(LINES_1K);
    let mut source_len = source.len() as isize;
    let id = db.open_file(
        Path::new("incremental_edit_1k.cyp"),
        source,
        DialectMode::GqlAligned,
    );
    let _ = db.analyse_file(id);
    let mut rng_state: u64 = 0x1234_5678_9abc_def0;

    c.bench_function("incremental_edit_1k", |b| {
        b.iter(|| {
            let edit = pick_edit(source_len as usize, &mut rng_state);
            source_len += edit_net_bytes(&edit);
            db.edit_file(id, &edit).unwrap();
            let _ = black_box(db.analyse_file(id));
        });
    });
}

// ---------------------------------------------------------------------------
// Main (harness = false)
// ---------------------------------------------------------------------------

fn main() {
    // 1. Criterion — feeds the 10% time-regression gate in CI.
    let mut c = Criterion::default()
        .sample_size(10) // each sample does a full update + analyse round
        .configure_from_args();
    bench_single_char_edit_1k(&mut c);
    c.final_summary();

    // 2. Super-linear-regression gate.  See module docs for why the budget
    //    is 2.5× (current arch is ~2.0×) and what would move it below 2.
    println!();
    println!(
        "=== bench_incremental_edit scaling gate (spec §17.10, ratio < {LINEAR_BUDGET:.2}) ==="
    );

    let p50_1k = median_edit_latency(LINES_1K);
    let p50_2k = median_edit_latency(LINES_2K);
    let ratio = p50_2k.as_secs_f64() / p50_1k.as_secs_f64().max(f64::MIN_POSITIVE);

    let p50_1k_us = p50_1k.as_secs_f64() * 1e6;
    let p50_2k_us = p50_2k.as_secs_f64() * 1e6;
    println!(
        "  1k-line median per-edit reanalyse: {p50_1k_us:>8.2} us ({EDITS} edits)"
    );
    println!(
        "  2k-line median per-edit reanalyse: {p50_2k_us:>8.2} us ({EDITS} edits)"
    );
    println!(
        "  scaling ratio (2k / 1k)         : {ratio:>8.3}  (budget {LINEAR_BUDGET:.2})"
    );

    if ratio > LINEAR_BUDGET {
        eprintln!(
            "  FAIL: per-edit reanalyse scales ratio {ratio:.3} ≥ {LINEAR_BUDGET:.2}× when \
             fixture doubled — super-linear regression."
        );
        std::process::exit(1);
    }
    println!("  OK (see module docs for the sub-linear follow-up bead)");
}
