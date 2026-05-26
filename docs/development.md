# Development

Local-development workflow for contributors. Testing standards are
graded to the rust-compiler bar (spec 0001 §17); the commands below
match that surface.

## Pre-commit hook

After cloning, install the gate hook so `cargo xtask gate` runs on
every commit:

```sh
bash scripts/install-hooks.sh
```

The gate runs:

- `cargo fmt --check`
- `cargo clippy -D warnings`
- `cargo test`
- `cargo deny check`

against the workspace.

## Test commands

```sh
cargo test --workspace                              # unit + integration + snapshots
cargo insta review                                  # snapshot review
cargo llvm-cov --workspace --html                   # coverage
cargo fuzz run fuzz_parser -- -max_total_time=300   # fuzz (nightly only)
cargo mutants -- -p cyrs-sema                       # mutation testing
cargo bench --workspace                             # criterion benchmarks
```

## TCK regeneration

The TCK harness writes the rolling acceptance numbers cited in
[`coverage.md`](./coverage.md). Spec 0001 §17.5 documents the
regeneration workflow; the baseline files under
[`crates/cyrs-tck/tck/`](../crates/cyrs-tck/tck/) carry the per-corpus
preambles.

## Release

The release workflow lives in [`release-playbook.md`](./release-playbook.md).
PRs are gated by `cargo-semver-checks`; see
[`stability.md`](./stability.md) for the surface-by-surface stability
contract.

## Fuzzing

Run-book and corpus management:
[`fuzz-runbook.md`](./fuzz-runbook.md).
