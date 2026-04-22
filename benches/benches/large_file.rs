//! bench_large_file — 10k-line synthetic Cypher, parse + HIR-lower +
//! diagnose (spec §17.10; bead cy-y6a.1).
//!
//! # What this measures
//!
//! A deterministic, synthetic 10,000-line Cypher source exercising four
//! clause templates (MATCH/property, MATCH/relationship/WHERE, UNWIND,
//! MATCH/WITH) and provides three end-to-end latency benchmarks plus
//! p95 gates:
//!
//! 1. **bench_parse_10k** — `cypher_syntax::parse` on the full source.
//! 2. **bench_hir_lower_10k** — iterate every statement, `lower_statement`
//!    + `desugar_statement` (each call re-parses internally, matching the
//!    public API shape in `cypher-hir`).
//! 3. **bench_diagnose_10k** — `Database::new()` → `open_file` → full
//!    `all_diagnostics` round-trip (parse + HIR lower + sema).
//!
//! # Measured p95 (local MacBook, 2026-04-21) and committed budgets
//!
//! The three p95 budgets live in `benches/large_file.budget.toml` and
//! are re-read at runtime; update both the TOML and this doc-comment
//! when re-baselining.
//!
//! | Bench                | measured p95 | budget (×1.2) |
//! |----------------------|--------------|---------------|
//! | bench_parse_10k      |   28.56 ms   |    35 ms      |
//! | bench_hir_lower_10k  |   57.10 ms   |    70 ms      |
//! | bench_diagnose_10k   |   54.00 ms   |    65 ms      |
//!
//! These are *nightly-only* gates: the large-file workload is too slow
//! for the PR-blocking bench job.
//!
//! # Determinism
//!
//! The source generator is a pure function of a counter — no `rand`, no
//! time-based seed, no environment input.  Two runs on the same commit
//! produce byte-identical sources.  Budget re-baselining is therefore
//! a deliberate operator action (commit a new TOML and update the
//! rustdoc numbers above) rather than noise-driven drift.
//!
//! # Templates (must parse & lower cleanly — no ERROR nodes)
//!
//! - `MATCH (pN:Label {id: N}) RETURN pN.name;`
//! - `MATCH (aN:User)-[:FOLLOWS]->(bN:User) WHERE aN.age > N RETURN aN, bN;`
//! - `UNWIND [1, 2, 3] AS xN RETURN xN;`
//! - `MATCH (nN:Item) WITH nN WHERE nN.active = true RETURN nN;`
//!
//! Each variable name is suffixed with the counter (`N`) so no two
//! adjacent statements share a binding — purely cosmetic for realism,
//! since each statement is terminated by `;`.

use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use criterion::Criterion;

use cypher_db::{Database, DialectMode};
use cypher_hir::desugar::desugar_statement;
use cypher_hir::lower::lower_statement;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Target line count for the synthetic source.  Each line = one `;`-
/// terminated statement, so line count == statement count.
const TARGET_LINES: usize = 10_000;

/// High-N sample count for the p95 gate (per bench).  500 samples at
/// ~300ms each = ~150s for the diagnose bench; tune if that's too slow.
const GATE_ITERATIONS: usize = 50;

/// Path (relative to the benches/ crate root) to the checked-in budget
/// TOML.  Parsed at bench start by a 15-line hand-rolled parser so we
/// don't need a `toml` dependency (constraint: no new deps beyond
/// criterion).
const BUDGET_PATH: &str = "large_file.budget.toml";

// ---------------------------------------------------------------------------
// Deterministic 10k-line synthetic Cypher source
// ---------------------------------------------------------------------------

/// Return ~10,000 lines of syntactically valid Cypher.  Purely
/// deterministic: output is a function of `TARGET_LINES` and the
/// hard-coded templates below.  Each statement is terminated by `;`
/// so the parser produces exactly `TARGET_LINES` STATEMENT nodes.
fn synthetic_10k() -> String {
    let mut out = String::with_capacity(TARGET_LINES * 80);
    for i in 0..TARGET_LINES {
        match i % 4 {
            0 => {
                // MATCH with property map literal.
                out.push_str(&format!(
                    "MATCH (p{i}:Label {{id: {i}}}) RETURN p{i}.name;\n"
                ));
            }
            1 => {
                // MATCH with relationship + WHERE predicate.
                out.push_str(&format!(
                    "MATCH (a{i}:User)-[:FOLLOWS]->(b{i}:User) \
                     WHERE a{i}.age > {i} RETURN a{i}, b{i};\n"
                ));
            }
            2 => {
                // UNWIND over a list literal.
                out.push_str(&format!("UNWIND [1, 2, 3] AS x{i} RETURN x{i};\n"));
            }
            _ => {
                // MATCH → WITH → WHERE pipeline.
                out.push_str(&format!(
                    "MATCH (n{i}:Item) WITH n{i} WHERE n{i}.active = true RETURN n{i};\n"
                ));
            }
        }
    }
    out
}

/// Split the synthetic source into individual statement strings
/// (trailing `;` retained, trailing newline retained).  Used by the
/// HIR-lower bench so each `lower_statement` call sees one statement.
fn statements_of(src: &str) -> Vec<&str> {
    // Each line is exactly one statement in our generator; split on `\n`
    // and discard any empty trailing entry.
    src.split_inclusive('\n').filter(|s| !s.trim().is_empty()).collect()
}

// ---------------------------------------------------------------------------
// Budget file parser — minimal TOML-compatible subset
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Budgets {
    parse_ms: u64,
    hir_lower_ms: u64,
    diagnose_ms: u64,
}

/// Hand-rolled parser for a flat, 3-key `key = N` TOML file.  Keeps the
/// bench deps limited to criterion (spec §17.10 + AGENTS §10).
fn load_budgets() -> Budgets {
    let path = Path::new(BUDGET_PATH);
    let raw = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("bench_large_file: failed to read {BUDGET_PATH}: {e}")
    });
    let mut parse_ms = None;
    let mut hir_lower_ms = None;
    let mut diagnose_ms = None;
    for line in raw.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (k, v) = match line.split_once('=') {
            Some(p) => p,
            None => continue,
        };
        let key = k.trim();
        let val: u64 = v.trim().parse().unwrap_or_else(|_| {
            panic!("bench_large_file: non-integer value for {key} in {BUDGET_PATH}")
        });
        match key {
            "parse_ms" => parse_ms = Some(val),
            "hir_lower_ms" => hir_lower_ms = Some(val),
            "diagnose_ms" => diagnose_ms = Some(val),
            _ => {}
        }
    }
    Budgets {
        parse_ms: parse_ms.expect("parse_ms missing from budget file"),
        hir_lower_ms: hir_lower_ms.expect("hir_lower_ms missing from budget file"),
        diagnose_ms: diagnose_ms.expect("diagnose_ms missing from budget file"),
    }
}

// ---------------------------------------------------------------------------
// Benches
// ---------------------------------------------------------------------------

fn bench_parse_10k(c: &mut Criterion, src: &str) {
    c.bench_function("parse_10k", |b| {
        b.iter(|| {
            let parse = cypher_syntax::parse(black_box(src));
            black_box(parse.syntax());
        });
    });
}

fn bench_hir_lower_10k(c: &mut Criterion, src: &str) {
    let statements = statements_of(src);
    c.bench_function("hir_lower_10k", |b| {
        b.iter(|| {
            for stmt in &statements {
                let lowered = lower_statement(black_box(stmt));
                black_box(desugar_statement(lowered));
            }
        });
    });
}

fn bench_diagnose_10k(c: &mut Criterion, src: &str) {
    c.bench_function("diagnose_10k", |b| {
        b.iter(|| {
            let mut db = Database::new();
            let id = db.open_file(
                Path::new("/tmp/bench_large_file.cyp"),
                black_box(src.to_owned()),
                DialectMode::GqlAligned,
            );
            let diags = db.all_diagnostics(id).expect("open file present");
            black_box(diags);
        });
    });
}

// ---------------------------------------------------------------------------
// p95 gates
// ---------------------------------------------------------------------------

fn sample_p95(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    let idx = (samples.len() as f64 * 0.95).ceil() as usize - 1;
    samples[idx]
}

fn p95_parse(src: &str) -> Duration {
    // One warmup to prime allocator / CPU caches.
    let _ = cypher_syntax::parse(src);
    let mut samples = Vec::with_capacity(GATE_ITERATIONS);
    for _ in 0..GATE_ITERATIONS {
        let t0 = Instant::now();
        black_box(cypher_syntax::parse(black_box(src)).syntax());
        samples.push(t0.elapsed());
    }
    sample_p95(&mut samples)
}

fn p95_hir_lower(src: &str) -> Duration {
    let statements = statements_of(src);
    // Warmup.
    for s in &statements {
        let _ = desugar_statement(lower_statement(s));
    }
    let mut samples = Vec::with_capacity(GATE_ITERATIONS);
    for _ in 0..GATE_ITERATIONS {
        let t0 = Instant::now();
        for stmt in &statements {
            black_box(desugar_statement(lower_statement(black_box(stmt))));
        }
        samples.push(t0.elapsed());
    }
    sample_p95(&mut samples)
}

fn p95_diagnose(src: &str) -> Duration {
    // Warmup: pays the first-time Salsa input-interning cost so the
    // tail samples reflect steady-state round-trip rather than cold
    // start.
    {
        let mut db = Database::new();
        let id = db.open_file(
            Path::new("/tmp/bench_large_file_warmup.cyp"),
            src.to_owned(),
            DialectMode::GqlAligned,
        );
        let _ = db.all_diagnostics(id).expect("open file present");
    }
    let mut samples = Vec::with_capacity(GATE_ITERATIONS);
    for _ in 0..GATE_ITERATIONS {
        let t0 = Instant::now();
        let mut db = Database::new();
        let id = db.open_file(
            Path::new("/tmp/bench_large_file.cyp"),
            src.to_owned(),
            DialectMode::GqlAligned,
        );
        let diags = db.all_diagnostics(id).expect("open file present");
        black_box(diags);
        samples.push(t0.elapsed());
    }
    sample_p95(&mut samples)
}

// ---------------------------------------------------------------------------
// Assertions on the synthetic source
// ---------------------------------------------------------------------------

/// Confirm the generator produces a clean parse (no ERROR / syntax-
/// errors).  If this ever regresses — e.g. a grammar tightening makes
/// one of our templates invalid — the bench surfaces the mismatch
/// before the first measurement, not in a noisy timing diff.
fn assert_clean_parse(src: &str) {
    let parse = cypher_syntax::parse(src);
    let errors = parse.errors();
    assert!(
        errors.is_empty(),
        "synthetic_10k produced {} syntax error(s); first: {:?}",
        errors.len(),
        errors.first()
    );
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let src = synthetic_10k();
    assert_clean_parse(&src);

    // 1. Criterion — feeds the existing 10% time-regression gate.
    let mut c = Criterion::default()
        .sample_size(10) // heavy bench — 10 samples keeps runtime sane
        .configure_from_args();
    bench_parse_10k(&mut c, &src);
    bench_hir_lower_10k(&mut c, &src);
    bench_diagnose_10k(&mut c, &src);
    c.final_summary();

    // 2. Budget gate — reads checked-in TOML and fails non-zero on
    //    regression.  Gate runs after criterion so the HTML reports are
    //    still written even if we abort.
    let budgets = load_budgets();
    println!();
    println!("=== bench_large_file budget gate (cy-y6a.1) ===");

    let parse_p95 = p95_parse(&src);
    report("parse_10k", parse_p95, budgets.parse_ms);

    let hir_p95 = p95_hir_lower(&src);
    report("hir_lower_10k", hir_p95, budgets.hir_lower_ms);

    let diag_p95 = p95_diagnose(&src);
    report("diagnose_10k", diag_p95, budgets.diagnose_ms);

    let mut failed = false;
    failed |= parse_p95 > Duration::from_millis(budgets.parse_ms);
    failed |= hir_p95 > Duration::from_millis(budgets.hir_lower_ms);
    failed |= diag_p95 > Duration::from_millis(budgets.diagnose_ms);
    if failed {
        eprintln!(
            "  FAIL: one or more p95s exceeded the checked-in budget in {BUDGET_PATH}.\n\
             Re-baseline intentionally by updating that file + this bench's rustdoc."
        );
        std::process::exit(1);
    }
    println!("  OK");
}

fn report(name: &str, p95: Duration, budget_ms: u64) {
    let ms = p95.as_secs_f64() * 1000.0;
    let tag = if p95 > Duration::from_millis(budget_ms) {
        "FAIL"
    } else {
        "ok"
    };
    println!("  {tag:<4}  {name}: p95 = {ms:.2} ms (budget {budget_ms} ms)");
}
