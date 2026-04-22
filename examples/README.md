# examples

Copy-pasteable minimum examples. Each subdirectory is self-contained:
`cd` in, run one command, see something. The full tours live at
[`../demo/`](../demo/) — these are the *smallest* programs that prove
the surface works.

| Directory | What it shows | How to run |
| --- | --- | --- |
| [`01-cli/`](01-cli/) | CLI `check` on a single file with one diagnostic | `../../target/release/cypher check example.cyp` |
| [`02-project/`](02-project/) | Multi-file workspace with shared `schema.toml` | `../../target/release/cypher check .` |
| [`03-agent-json/`](03-agent-json/) | Scripting via the `cypher-agent` JSON protocol | `./run.sh` |
| [`04-library/`](04-library/) | Using the `cypher` meta-crate as a Rust library | `cargo run` |
| [`05-wasm-monaco/`](05-wasm-monaco/) | In-browser check via `cypher-wasm` (minimal, no Monaco) | see dir README |
| [`06-ffi-c/`](06-ffi-c/) | C program linking `libcypher_ffi` | see dir README |

Build the two binaries once from the repo root:

```sh
cargo build --release -p cypher-cli -p cypher-agent
```

That gives you `target/release/cypher` (for `01-cli` and `02-project`)
and `target/release/cypher-agent` (for `03-agent-json`). `04-library`
is a standalone `cargo run`. `05-wasm-monaco` and `06-ffi-c` have
their own build steps — see each directory.

## Isolation contract

None of these examples are members of the root Cargo workspace.
`04-library/Cargo.toml` carries its own `[workspace]` table so
`cargo check --workspace` from the repo root ignores it entirely.

## Related

- [`../demo/`](../demo/) — the full Neovim + Monaco + FFI tour.
- [`../README.md`](../README.md) — the top-level project README.
- [`../docs/specs/`](../docs/specs/) — the authoritative spec.
