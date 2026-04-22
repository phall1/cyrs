# 04-library — `cypher` as a Rust library

Standalone Cargo package that opens a file in a `Database`, runs the
full analysis pipeline, and prints each diagnostic.

The `[workspace]` table in `Cargo.toml` is deliberate: it stops Cargo
walking up to the repo's root workspace so `cargo check --workspace`
at the root does not pull this example in.

## Run

```sh
cargo run
```

## Expected output

```
1 diagnostic(s):
  [E0011] error (9..9): expected ')' to close node pattern
```

## What it shows

- `Database::new()` + `open_file(path, source, dialect)` — the standard
  entry-point all binaries in the workspace share.
- `Database::all_diagnostics(id)` — full pipeline (parse + sema).
- Iterating `Diagnostic { code, severity, message, primary.range, … }`
  — the stable public shape.

## Dependency

```toml
cypher = { path = "../../crates/cypher" }
# cypher = "0.1"    # swap to this once the meta-crate is published
```

The path dep works today, inside the repo. Once the `cypher` meta-crate
is on crates.io, delete the path line and uncomment the versioned one.
