//! bench_agent_throughput — 10 k sequential agent ops against the
//! `cypher-agent` JSON-over-stdio protocol; reports ops/sec and
//! p99 latency (spec §17.10, §15; bead cy-y6a).
//!
//! # What this measures
//!
//! A long-running `cypher-agent` subprocess driven over stdio with
//! one request per line, one response per line (the real wire
//! format per spec §15).  The bench measures round-trip latency —
//! JSON encode → write stdin → server handle → read stdout → JSON
//! decode — for [`REQUESTS`] back-to-back requests.
//!
//! This gives us an honest end-to-end ops/sec number.  A bench that
//! called the handler in-process would under-report because it would
//! skip the exact cost the agent caller pays on every op: stdio
//! write + newline + flush + read.
//!
//! # Fixture
//!
//! We rotate the four simplest ops (`parse`, `check`, `format`,
//! `plan`) across the 10 k requests on a realistic multi-clause
//! query.  The `complete` / `hover` ops require byte-offset
//! coordination that the bench shouldn't couple to; the four cheap
//! ops alone already exercise the hot path on both the request
//! parser and the response serialiser.
//!
//! # Build step
//!
//! On first run the bench builds `cypher-agent` in release mode from
//! the parent workspace (benches/ is its own `[workspace]` table per
//! spec §17.10 — see `benches/Cargo.toml` — so we have to reach
//! explicitly at `../Cargo.toml`).  The compiled binary is reused
//! across iterations.  Build output is streamed to stderr.
//!
//! # Gate
//!
//! Two numbers the operator cares about:
//!
//! - **ops/sec** — throughput over the 10 k requests.  We gate at a
//!   floor of [`OPS_PER_SEC_FLOOR`]; a regression that drops below
//!   is a blocking failure.
//! - **p99 latency** — 99th-percentile round-trip.  We gate at
//!   [`P99_CEILING`]; tail latency matters for interactive agents.

#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use criterion::{Criterion, black_box};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Total request count for the gate run.  Spec §17.10 / bead fixes this
/// at 10,000.
const REQUESTS: usize = 10_000;

/// Requests we run + discard before starting the sample window.  Covers
/// one-time costs: stdio buffer warm-up, JIT tokenizer warm-up, Salsa
/// first-query interning.
const WARMUP_REQUESTS: usize = 200;

/// Throughput floor, in ops/sec.  Any regression below this fails the
/// bench.  500 ops/sec ≈ 2ms per round trip — comfortable even on slow
/// CI runners where stdio flush is the dominant cost.
const OPS_PER_SEC_FLOOR: f64 = 500.0;

/// p99 ceiling.  Tail latency matters for interactive agent use.
const P99_CEILING: Duration = Duration::from_millis(50);

/// Representative multi-clause query used across all four ops.
const SAMPLE_QUERY: &str =
    "MATCH (n:Person {name: $name})-[:KNOWS]->(m:Person) WHERE m.age > 30 RETURN n.name";

// ---------------------------------------------------------------------------
// Build + spawn the agent subprocess
// ---------------------------------------------------------------------------

/// Build `cypher-agent` in release mode against the parent workspace
/// manifest and return the path to the compiled binary.  Rebuild is a
/// no-op if the binary is already up to date.
fn build_agent_binary() -> PathBuf {
    // benches/ lives one level below the workspace root.  CARGO_MANIFEST_DIR
    // points at the benches/ crate when cargo invokes the bench, so we
    // step one directory up to reach the workspace root.
    let benches_manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(benches_manifest)
        .parent()
        .expect("benches/ sits under the workspace root")
        .to_path_buf();
    let workspace_manifest = workspace_root.join("Cargo.toml");

    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "-p",
            "cypher-agent",
            "--manifest-path",
        ])
        .arg(&workspace_manifest)
        .status()
        .expect("spawn `cargo build -p cypher-agent`");
    assert!(
        status.success(),
        "cargo build -p cypher-agent failed (exit {status})"
    );

    let bin = workspace_root
        .join("target")
        .join("release")
        .join(if cfg!(windows) {
            "cypher-agent.exe"
        } else {
            "cypher-agent"
        });
    assert!(
        bin.exists(),
        "cypher-agent binary not at expected path after build: {}",
        bin.display(),
    );
    bin
}

/// Long-lived agent subprocess.  Owns the child handle + its stdio so
/// request/response round-trips share the same pipes across all
/// measured iterations.
struct AgentHarness {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    line_buf: String,
}

impl AgentHarness {
    fn spawn(bin: &Path) -> Self {
        let mut child = Command::new(bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn cypher-agent");
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout piped"));
        Self {
            child,
            stdin,
            stdout,
            line_buf: String::with_capacity(4096),
        }
    }

    /// Send one request line + read one response line.  Panics if the
    /// child closes early.
    fn round_trip(&mut self, req_json: &str) -> Value {
        // Write request + newline + flush so the agent's line-buffered
        // reader delivers it immediately.
        self.stdin.write_all(req_json.as_bytes()).expect("write req");
        self.stdin.write_all(b"\n").expect("write newline");
        self.stdin.flush().expect("flush stdin");

        self.line_buf.clear();
        let n = self
            .stdout
            .read_line(&mut self.line_buf)
            .expect("read response");
        assert!(n > 0, "agent closed stdout early");
        serde_json::from_str::<Value>(self.line_buf.trim_end()).expect("valid JSON response")
    }

    /// Ask the agent to shut down cleanly and wait for the child to exit.
    fn shutdown(mut self) {
        let _ = writeln!(self.stdin, r#"{{"op":"shutdown"}}"#);
        let _ = self.stdin.flush();
        // Read the final `{"op":"shutdown"}` response to drain the pipe.
        self.line_buf.clear();
        let _ = self.stdout.read_line(&mut self.line_buf);
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Request rotation
// ---------------------------------------------------------------------------

/// Rotate through four cheap ops so the bench doesn't degenerate into
/// a single-path microbench.  The four ops together cover: parse
/// (cy-nk7), sema (cy-amr), format (cy-fmt), plan (cy-plan).
fn request_for(i: usize) -> String {
    match i % 4 {
        0 => json!({ "op": "parse",  "text": SAMPLE_QUERY }).to_string(),
        1 => json!({ "op": "check",  "text": SAMPLE_QUERY }).to_string(),
        2 => json!({ "op": "format", "text": SAMPLE_QUERY }).to_string(),
        _ => json!({ "op": "plan",   "text": SAMPLE_QUERY }).to_string(),
    }
}

// ---------------------------------------------------------------------------
// Criterion bench — single round-trip latency
// ---------------------------------------------------------------------------

fn bench_single_round_trip(c: &mut Criterion, harness: &mut AgentHarness) {
    // Warmup a handful of requests so the allocator and the agent's
    // per-dialect FileId registry are primed before criterion samples.
    for i in 0..32 {
        let _ = harness.round_trip(&request_for(i));
    }

    let mut round: usize = 0;
    c.bench_function("agent_round_trip", |b| {
        b.iter(|| {
            let req = request_for(round);
            round = round.wrapping_add(1);
            black_box(harness.round_trip(&req));
        });
    });
}

// ---------------------------------------------------------------------------
// Gate — ops/sec + p99 over REQUESTS round trips
// ---------------------------------------------------------------------------

fn run_throughput_gate(bin: &Path) -> (f64, Duration) {
    let mut harness = AgentHarness::spawn(bin);

    // Warmup: amortise first-request fixed costs (Salsa input-interning,
    // stdio buffer fill, etc.).
    for i in 0..WARMUP_REQUESTS {
        let _ = harness.round_trip(&request_for(i));
    }

    let mut samples: Vec<Duration> = Vec::with_capacity(REQUESTS);
    let t0 = Instant::now();
    for i in 0..REQUESTS {
        let req = request_for(WARMUP_REQUESTS + i);
        let rt = Instant::now();
        let _ = harness.round_trip(&req);
        samples.push(rt.elapsed());
    }
    let elapsed = t0.elapsed();

    harness.shutdown();

    let ops_per_sec = (REQUESTS as f64) / elapsed.as_secs_f64().max(f64::MIN_POSITIVE);
    samples.sort_unstable();
    // Classic p99 index — ceil(0.99 * N) - 1.
    let idx = (samples.len() as f64 * 0.99).ceil() as usize - 1;
    let p99 = samples[idx];
    (ops_per_sec, p99)
}

// ---------------------------------------------------------------------------
// main (harness = false)
// ---------------------------------------------------------------------------

fn main() {
    let bin = build_agent_binary();

    // 1. Criterion round-trip latency — feeds the PR 10% regression gate.
    let mut criterion_harness = AgentHarness::spawn(&bin);
    let mut c = Criterion::default().configure_from_args();
    bench_single_round_trip(&mut c, &mut criterion_harness);
    c.final_summary();
    criterion_harness.shutdown();

    // 2. Throughput gate — ops/sec floor + p99 ceiling over 10 k requests.
    println!();
    println!("=== bench_agent_throughput gate (spec §17.10) ===");
    let (ops_per_sec, p99) = run_throughput_gate(&bin);
    println!(
        "  throughput : {ops_per_sec:>9.1} ops/sec  [floor {OPS_PER_SEC_FLOOR:.1}]"
    );
    let p99_ms = p99.as_secs_f64() * 1000.0;
    println!(
        "  p99 latency: {p99_ms:>9.2} ms       [ceiling {} ms]",
        P99_CEILING.as_millis()
    );

    let mut failed = false;
    if ops_per_sec < OPS_PER_SEC_FLOOR {
        eprintln!(
            "  FAIL: throughput {ops_per_sec:.1} ops/sec below floor {OPS_PER_SEC_FLOOR:.1}"
        );
        failed = true;
    }
    if p99 > P99_CEILING {
        eprintln!(
            "  FAIL: p99 {:.2} ms exceeds ceiling {} ms",
            p99_ms,
            P99_CEILING.as_millis()
        );
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
    println!("  OK");
}
