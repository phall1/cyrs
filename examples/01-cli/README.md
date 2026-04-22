# 01-cli — `cypher check` on a single file

Smallest possible thing. One file, one diagnostic, one command.

## Run

From the repo root, build the CLI once:

```sh
cargo build --release -p cypher-cli
```

Then from this directory:

```sh
../../target/release/cypher check example.cyp
```

Or use the wrapper:

```sh
./run.sh
```

## Expected output

Stderr (codespan-rendered, ANSI elided):

```
error[E0011]: expected ')' to close node pattern
  ┌─ example.cyp:3:1
  │
3 │ RETURN n
  │ ^
```

Exit code: `1` (diagnostics present; see spec §16).

The code `E0011` is stable — once assigned, it never changes meaning
(spec §10, diagnostic-code stability).

## Note on distribution

`cypher-cli` is not yet published to crates.io, so `cargo install
cypher-cli` does not work today. Build from source with `cargo build
--release -p cypher-cli` and either add `target/release/` to `$PATH` or
invoke the binary by full path.
