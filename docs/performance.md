# Performance

Measured numbers for the cyrs front-end at v0.1.0. Every figure is the
committed baseline from `benches/baselines/` — not marketing. If a
nightly run diverges by >10 % the job fails.

## At a glance

Baseline host: MacBook, darwin/aarch64, release profile, captured at
bead cy-y6a landing (nightly suite) and bead cy-wcv landing (24 h
soak). Numbers are per-iter means from Criterion
(`target/criterion/<bench>/new/estimates.json`) unless noted.

| Bench                     | What it measures                                                  | Current (main)                              | Budget                                 |
| ------------------------- | ----------------------------------------------------------------- | ------------------------------------------- | -------------------------------------- |
| `bench_large_file`        | parse + HIR lower + diagnose on a 10 k-line synthetic source       | mean 27.5 / 55.9 / 53.6 ms; p95 28.6 / 57.1 / 54.0 ms | p95 35 / 70 / 65 ms                    |
| `bench_incremental_edit`  | 1 k single-char edits on a 1 k-line fixture; 2 k/1 k scaling ratio | mean 7.26 ms/edit; ratio near 2.0×          | ratio < 2.0×                           |
| `bench_workspace_fan`     | 100-file cross-file diagnostics sweep                              | mean 23.7 ms/sweep; warm-up ≤ 10 s; RSS ratio ≈ 1.0 | warm-up ≤ 10 s; RSS ratio ≤ 2.0×       |
| `bench_agent_throughput`  | 10 k sequential JSON round-trips over stdio                        | mean 20.8 µs/op ≈ 48 034 ops/sec            | ≥ 500 ops/sec; p99 ≤ 50 ms             |
| `bench_incremental_24h`   | 4 h / 24 h RSS soak (agent single-FileId + LSP FileId-churn)       | provisional smoke: agent +56.5 KiB/hr, LSP 0.0 KiB/hr | \|slope\| ≤ 100 KiB/hr (gated at ≥ 4 h) |

The 24 h-soak numbers are **provisional** — captured on a 10 min smoke
at cy-wcv landing. The first weekly 4 h run on ubuntu-latest will
replace them.

## How we measure

- Criterion 0.5 (pinned in `benches/Cargo.toml`) for four of the five
  benches.
- Custom harness (`harness = false`) for `bench_incremental_24h`:
  long-horizon RSS sampling every 300 s (60 s for sub-1 h smokes); OLS
  line fit over the trailing `min(8 h, post-warmup)` window after a
  0.25 × `CY_SOAK_HOURS` warm-up.
- Each bench carries **internal budget gates** (p95 wall-clock,
  scaling ratio, RSS ceiling, ops/sec floor, p99 ceiling, RSS slope).
  These are hard failures — the bench binary exits non-zero and the
  workflow step fails.
- `nightly-benches.yml` additionally compares committed Criterion
  means against the current run. Any bench whose `mean_ns` exceeds
  `baseline * 1.10` fails the job.

## Per-bench methodology

### `bench_large_file`

10 k-line synthetic source generated procedurally (pure function of a
counter — no `rand`, no time seed; two runs on one commit produce
byte-identical fixtures). Four rotating clause templates: MATCH with
property, MATCH with relationship + WHERE, UNWIND, MATCH/WITH. Three
sub-measurements isolate the pipeline stages: `parse_10k` runs
`cypher_syntax::parse`; `hir_lower_10k` iterates every statement through
`lower_statement` + `desugar_statement`; `diagnose_10k` runs the full
`Database::new()` → `open_file` → `all_diagnostics` round-trip. p95
budgets live in `benches/large_file.budget.toml`.

### `bench_incremental_edit`

Uses the `Database::edit_file(TextEdit)` API added in cy-zv0 — the
exact path `textDocument/didChange` takes after the LSP notification
is translated. Each edit appends one ASCII space at a
deterministically-drawn byte offset (LCG seed fixed in source). The
bench measures median per-edit reanalysis time at 1 k and 2 k lines;
the ratio `p50_2k / p50_1k` is the scaling gate — it detects
super-linear regressions without blocking on the smart-path driver
gap. The smart sub-tree splice is deferred to bead cy-li5; today the
reparse is a whole-file fallback and the observed ratio sits near 2.0.

### `bench_workspace_fan`

Drives the cy-o8c tranche-1 workspace Database. The bench materialises
a `tempfile::TempDir` containing a canonical `cypher-project.toml`, a
shared `schema.toml`, and 100 procedurally-generated 50-line `.cyp`
members (5 000 lines total — procedurally generated so the repo stays
flat per spec §17.10). The cold sweep mints fresh FileIds; subsequent
warm sweeps use `update_file` to match the real
`textDocument/didChange` path. Warm-up budget: one cold sweep ≤ 10 s.
Steady-state RSS gate: after 5 warm sweeps, `steady_rss / warm_rss`
must stay under 2.0 — in practice it sits at 1.00.

### `bench_agent_throughput`

Spawns the release `cypher-agent` binary, drives 10 k sequential
stdio round-trips (one JSON request per line in; one response per line
out). Rotates the four cheap ops — `parse`, `check`, `format`, `plan`
— on a realistic multi-clause query. The numbers reflect the full
cost the agent caller pays on every op: JSON encode, stdin write,
newline + flush, server handle, stdout read, JSON decode. An
in-process bench would under-report. Gates: ops/sec ≥ 500 and p99 ≤
50 ms.

### `bench_incremental_24h`

Two modes run sequentially:

- **Agent single-FileId.** One long-lived `FileId` re-edited forever.
  Exercises the Salsa per-query LRU caps from cy-31b.
- **LSP FileId-churn.** A fresh `FileId` is opened, analysed, and
  `didClose`-evicted every cycle. Exercises the per-FileId
  reclamation path from cy-it7.

`CY_SOAK_HOURS` selects the horizon (default 24, float accepted).
Weekly CI runs at 4 h (GitHub Actions public-runner ceiling is 6 h);
24 h runs are operator-dispatched. The slope gate is only enforced
when `CY_SOAK_HOURS >= 4` — shorter runs are smoke-only.

## Regression policy

- **10 % mean-regression gate** (nightly Criterion). Any bench whose
  `mean_ns` in `benches/baselines/nightly.json` is exceeded by more
  than 10 % fails the workflow.
- **Internal budget gates** fail the bench binary directly: p95
  wall-clock, scaling ratio, RSS ceiling, ops/sec floor, p99 ceiling,
  RSS slope. These are **hard** — no warn-only mode.
- **Re-baselining is explicit.** The operator commits an updated
  `benches/baselines/nightly.json` (or `incremental_24h.json`) in its
  own PR with a rationale in the commit message. See
  `benches/baselines/README.md`.

## Known sub-optimal paths

These are tracked gaps — called out here so the numbers above are
honest, not flattering.

- **Incremental edit falls back to whole-file reparse.** The public
  `Database::edit_file` API landed in cy-zv0, but the underlying
  reparse is not yet sub-tree spliced. Tracked in bead cy-li5. Target
  scaling ratio after cy-li5 lands: < 1.5×.
- **24 h soak baseline is provisional.** Committed numbers in
  `benches/baselines/incremental_24h.json` come from a ~10 min smoke
  at cy-wcv landing, not a real plateau trace. The first weekly
  `soak_4h` workflow run on ubuntu-latest will populate production
  values.

## Reproducing locally

From the workspace root:

```sh
cargo bench --manifest-path benches/Cargo.toml --bench large_file
cargo bench --manifest-path benches/Cargo.toml --bench bench_incremental_edit
cargo bench --manifest-path benches/Cargo.toml --bench bench_workspace_fan
cargo bench --manifest-path benches/Cargo.toml --bench bench_agent_throughput
CY_SOAK_HOURS=0.25 cargo bench --manifest-path benches/Cargo.toml --bench bench_incremental_24h
```

Expected run times (guidance, not commitments): each of the four
Criterion benches 30–60 s; the soak bench runs for the requested
horizon (`CY_SOAK_HOURS=0.25` ≈ 15 min smoke; weekly CI = 4 h;
operator-dispatched = 24 h). HTML reports land in
`benches/target/criterion/`.
