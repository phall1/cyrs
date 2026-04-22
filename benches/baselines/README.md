# Nightly bench baselines (spec §17.10, bead cy-y6a)

Reference baselines for the four at-scale benches gated by
`.github/workflows/nightly-benches.yml`:

| Bench name                  | Source file                                      |
|----------------------------|--------------------------------------------------|
| `parse_10k`                | `benches/large_file.rs`                          |
| `hir_lower_10k`            | `benches/large_file.rs`                          |
| `diagnose_10k`             | `benches/large_file.rs`                          |
| `incremental_edit_1k`      | `benches/bench_incremental_edit.rs`              |
| `workspace_fan_sweep_100`  | `benches/bench_workspace_fan.rs`                 |
| `agent_round_trip`         | `benches/bench_agent_throughput.rs`              |

## Format

`nightly.json` is a flat `{ bench_name -> { mean_ns, median_ns } }` object.
Both fields come from criterion's
`target/criterion/<bench_name>/new/estimates.json`:

- `mean_ns` — `estimates.json`.`mean.point_estimate`
- `median_ns` — `estimates.json`.`median.point_estimate`

Nanoseconds per iteration, as criterion records them.

## 10% regression gate

`nightly-benches.yml` runs every bench on a fresh runner and compares each
bench's `mean_ns` to the value committed here. Any bench whose mean
exceeds `baseline * 1.10` fails the workflow.

Re-baselining is an explicit operator action: re-run the nightly workflow
locally (or pull down the criterion artifact), update `nightly.json` from
the new estimates, and commit the change in its own PR with the
motivation in the commit message.

## Initial baselines

Captured on a MacBook (darwin/aarch64) at bead cy-y6a landing. Absolute
numbers will differ on the ubuntu-latest GitHub runner; the gate
tolerance (10 %) is set to absorb that first-PR drift without
catastrophising. The first nightly run on `main` will replace these
with runner-representative values.

## `incremental_24h.json` (cy-wcv, spec §11.6 / §17.10)

Separate file because the 24h soak is not a criterion-timed bench; the
values captured there are memory-vs-wall-clock, not time-per-iter.

- `baseline_rss_mib` / `steady_rss_mib` — RSS at the first and last
  sample of each mode.
- `tail_slope_kib_per_hr` — OLS slope of the trailing
  `min(8 h, post-warmup)` window.  The slope gate fires when the
  absolute value reaches 100 KiB/hr; see `benches/README.md` for the
  full methodology.

The baseline shipped with cy-wcv is **provisional**: it was captured
during bead landing via a ~10 min smoke (`CY_SOAK_HOURS=0.1667`) to
confirm the bench wiring builds and produces a sensible trace.  Real
runner-representative numbers will land after the first weekly
`soak_4h` workflow run.
