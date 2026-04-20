# cyrs benchmarks (spec §17.10)

Criterion micro-benchmarks for the four core pipeline stages.

## Benchmark targets

| Bench file | Measures |
|---|---|
| `benches/bench_parse.rs` | `cypher_syntax::parse` — lexer + recovering CST parser |
| `benches/bench_format.rs` | `cypher_fmt::format` — CST-driven formatter |
| `benches/bench_sema.rs` | `cypher_sema::analyse` — semantic analysis (schema-free) |
| `benches/bench_plan.rs` | `cypher_plan::lower::lower_statement` — HIR → logical plan |

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

## Exclusion from workspace

`benches/` carries its own `[workspace]` table and is listed in the root
`Cargo.toml` `exclude` array, mirroring the `fuzz/` pattern.  This means
`cargo check --workspace` and `cargo xtask gate` do not touch it.
