# cyrs benchmarks (spec §17.10)

Criterion micro-benchmarks for the four core pipeline stages + the
at-scale perf suite (cy-y6a).

## Benchmark targets

### PR gate (fast)

| Bench file | Measures |
|---|---|
| `benches/bench_parse.rs` | `cyrs_syntax::parse` — lexer + recovering CST parser |
| `benches/bench_format.rs` | `cyrs_fmt::format` — CST-driven formatter |
| `benches/bench_sema.rs` | `cyrs_sema::analyse` — semantic analysis (schema-free) |
| `benches/bench_plan.rs` | `cyrs_plan::lower::lower_statement` — HIR → logical plan |
| `benches/bench_incremental.rs` | Long-horizon RSS-stability workload (agent + LSP churn) |
| `benches/bench_lsp_completion.rs` | End-to-end LSP completion round-trip (p95 ≤ 25 ms) |

### Nightly (at-scale, heavy)

See `.github/workflows/nightly-benches.yml`.

| Bench file | Measures |
|---|---|
| `benches/large_file.rs` | 10 k-line parse + HIR-lower + diagnose, p95 budgets in `large_file.budget.toml` |
| `benches/bench_incremental_edit.rs` | 1 k single-char edits in a 1 k-line fixture; super-linear-regression gate |
| `benches/bench_workspace_fan.rs` | 100-file workspace `cypher check <dir>` sweep — warm-up budget + steady-state RSS ceiling |
| `benches/bench_agent_throughput.rs` | 10 k JSON round trips against `cyrs-agent`; ops/sec floor + p99 ceiling |
| `benches/bench_incremental_24h.rs` | 24-hour RSS soak (weekly 4 h, on-demand 24 h) — slow-leak slope gate |

## Running locally

```sh
# All four benches (from workspace root):
cargo bench --manifest-path benches/Cargo.toml

# Single bench:
cargo bench --manifest-path benches/Cargo.toml --bench bench_parse

# Quick smoke-run (one sample, skips warm-up):
cargo bench --manifest-path benches/Cargo.toml -- --sample-size 10

# Save a named baseline (useful before a refactor):
cargo bench --manifest-path benches/Cargo.toml -- --save-baseline before-refactor

# Compare against a saved baseline:
cargo bench --manifest-path benches/Cargo.toml -- --baseline before-refactor
```

HTML reports are written to `benches/target/criterion/`.

## 10% regression gate (CI)

`.github/workflows/bench.yml` implements spec §17.10:

1. **Push to `main`** — runs all benches and uploads a `bench-baseline-main`
   artifact containing mean nanoseconds per benchmark.
2. **Pull request** — downloads the baseline artifact, runs the same benches,
   and fails if any benchmark's mean time increased by more than 10% relative
   to baseline.

If no baseline artifact exists yet (very first PR before any push to main has
landed), the comparison step is skipped and only the raw results are uploaded.

## Soak methodology — `bench_incremental_24h` (cy-wcv, spec §11.6 / §17.10)

Long-horizon sibling of `bench_incremental`.  Runs open/edit/close cycles
continuously for `CY_SOAK_HOURS` (default **24**) and samples resident-set
size (RSS) every 5 minutes (60 s for runs shorter than 1 h).  Two modes
execute sequentially — agent single-FileId and LSP FileId-churn — to
stress both the Salsa LRU caps (cy-31b) and the `didClose` eviction path
(cy-it7).

**Why 4 h is the weekly target.**  The GitHub Actions ceiling for public
runners is 6 hours per job.  A 4 h run is the longest plateau
observation we can do on the free tier; empirically it is more than
enough to see the agent-mode working set converge (LRU fills within
~15 min on the 50-clause fixture) and, once past the 1 h warm-up window
defined in the bench, to exercise the slope gate on a ~3 h post-warmup
tail.

**Why 24 h is operator-triggered.**  A true 24 h trace requires either a
self-hosted runner or splitting the job; either is operationally
expensive and unnecessary on every merge.  Dispatching manually
(`gh workflow run nightly-benches.yml -f hours=24`) on suspected-leak
PRs or before release gates gives the same signal on demand.

**How to read the slope-per-hour number.**  After a (per-run) warm-up
window equal to `0.25 × CY_SOAK_HOURS`, the bench fits an OLS line to
RSS-vs-hours over the trailing `min(8 h, post-warmup duration)`.  The
slope is printed as `<mode> tail slope: +N.NN KiB/hr`.

- Slopes within ±10 KiB/hr reflect allocator jitter and pass.
- Slopes at +100 KiB/hr or beyond are the gate threshold — interpreted
  as "this process would leak ≈ 2.4 MiB per day of continuous use",
  which is the spec §11.6 steady-state bound read as a rate.
- A **negative** slope at gate magnitude is still a FAIL — that
  indicates oscillation (RSS dropped during the tail window, suggesting
  the plateau is not yet stable) and deserves investigation.

The gate only runs when `CY_SOAK_HOURS >= 4`.  Shorter horizons are
smoke-only and print the slope without enforcing it.

### Baseline (`benches/baselines/incremental_24h.json`)

The committed baseline is **provisional** until the first weekly
`soak_4h` workflow run lands representative numbers for the CI runner.
The initial values were captured on a ~10 min smoke during bead cy-wcv
landing and are there purely to document the shape of the file.  Do not
re-baseline from a smoke run — wait for a real plateau trace.

## Exclusion from workspace

`benches/` carries its own `[workspace]` table and is listed in the root
`Cargo.toml` `exclude` array, mirroring the `fuzz/` pattern.  This means
`cargo check --workspace` and `cargo xtask gate` do not touch it.
