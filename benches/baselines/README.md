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
