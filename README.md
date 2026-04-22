# cypher — a Rust front-end for Cypher / GQL

[![crates.io](https://img.shields.io/crates/v/cypher.svg)](https://crates.io/crates/cypher)
[![docs.rs](https://img.shields.io/docsrs/cypher)](https://docs.rs/cypher)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](./rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

A standalone compiler front-end for Cypher and GQL — lexer, recovering
parser, lossless CST, typed AST, HIR with name resolution, schema-aware
semantic analysis, logical plan IR, formatter, incremental analysis
database, language server, agent JSON API, and CLI. Zero coupling to
any storage engine or graph database.

**91.9%** of the openCypher TCK passes. **29 ms** to parse a 10k-line
query. **48 k** agent ops/sec. Embeddable from C, Python, JavaScript,
and any LSP client.

![cypher-lsp + nvim demo](./demo/demo.gif)

---

## What this is

A compiler front-end for a query language — not a database. It takes
Cypher or GQL text and hands you a typed Plan IR, stable diagnostics,
and everything an IDE needs. Downstream consumers execute the Plan
against their own storage.

```
  Cypher / GQL text
        │
        ▼
  cypher-syntax      lexer, recovering parser, lossless CST
        │
        ▼
  cypher-ast         typed AST wrappers over the CST
        │
        ▼
  cypher-hir         HIR + name resolution + desugar
        │
        ▼
  cypher-sema        type system + semantic analysis (schema-aware)
        │
        ▼
  cypher-plan        logical read / write plan IR
        │
        ▼
  consumer executes against its own storage
```

Schema, custom functions, and write-clause semantics are plugged in
through trait implementations — not baked into the workspace.

## Who it's for

- **Graph-DB vendors.** Drop-in front-end; use the Plan IR, own
  execution and storage.
- **Indexer / SCIP consumers.** Emit code intelligence for Cypher
  codebases (`cypher-cli index scip`).
- **Tool authors.** Parse Cypher reliably from any language via the
  C ABI, PyO3 wheel, or WASM binding.
- **Editor integrations.** LSP server + tree-sitter grammar, ready
  to ship.

---

## Install

```sh
cargo install --locked cypher-cli
cypher --help
```

## First query

```sh
cat > q.cyp <<'EOF'
MATCH (p:Person)-[:ACTED_IN]->(m:Movie)
RETURN p.name, m.title
EOF

cypher check q.cyp      # diagnostics
cypher fmt   q.cyp      # canonical form
cypher plan  q.cyp      # logical plan IR
```

Or drop a `cypher-project.toml` and a `schema.toml` alongside multiple
`.cyp` files and run `cypher check .` — cross-file label resolution,
SCIP index emission, and the full LSP surface all work against the
same workspace model.

Full walkthrough: [`docs/getting-started.md`](./docs/getting-started.md).

---

## Interop surfaces

| You are… | Use | Doc |
| --- | --- | --- |
| Web frontend / Monaco / CodeMirror | `cypher-wasm` | [crate README](./crates/cypher-wasm/README.md) |
| Go / Java / Swift / Node / C | `cypher-ffi` (stable C ABI) | [crate README](./crates/cypher-ffi/README.md) |
| Python | `cypher-py` (PyO3 + abi3 wheel) | [crate README](./crates/cypher-py/README.md) |
| Editor highlighting (no LSP) | `tree-sitter-cypher` | [tree-sitter README](./tree-sitter-cypher/README.md) |
| Browser-hosted LSP | `cypher-lsp --features web-lsp` | [`docs/interop.md`](./docs/interop.md) |

All adapters are thin — they wrap `cypher-lang-services` (the shared
engine crate) and add zero analysis logic. Stability commitments per
surface live in
[`docs/specs/0004-interop-surfaces.md`](./docs/specs/0004-interop-surfaces.md).

---

## Numbers

| Metric | Value | Source |
| --- | --- | --- |
| openCypher TCK pass rate | **91.9%** (3583 / 3897) | `crates/cypher-tck/tck/full-baseline.md` |
| Parse + lower + diagnose 10k-line query | p95 29.3 / 55.9 / 56.4 ms | `benches/bench_large_file` |
| Check 100-file workspace (cold sweep) | p95 25.2 ms | `benches/bench_workspace_fan` |
| Agent JSON throughput | 48 034 ops/sec (baseline) | `benches/bench_agent_throughput` |
| Incremental edit — 2k/1k scaling | 1.91× (gate ≤ 2.0×) | `benches/bench_incremental_edit` |
| Workspace steady-state RSS | 32 MiB (100 files) | `benches/bench_workspace_fan` |
| Diagnostic codes registered | 120 (SemVer-stable) | `crates/cypher-diag/src/codes.rs` |

A 10% regression on any committed baseline fails CI. Methodology +
per-bench detail: [`docs/performance.md`](./docs/performance.md).

---

## Crates

| Crate | Purpose |
| --- | --- |
| `cypher-syntax` | Lexer, recovering parser, lossless CST, `SyntaxKind` |
| `cypher-ast` | Typed AST wrappers over the CST |
| `cypher-hir` | HIR, name resolution, scope graph, desugaring |
| `cypher-sema` | Semantic analysis + type system |
| `cypher-schema` | `SchemaProvider` trait + `schema.toml` loader |
| `cypher-project` | `cypher-project.toml` manifest + discovery |
| `cypher-diag` | Diagnostic type, stable code registry |
| `cypher-plan` | Logical read / write Plan IR |
| `cypher-fmt` | CST-driven idempotent formatter |
| `cypher-db` | Salsa-based incremental analysis database |
| `cypher-lang-services` | Shared completion / hover / rewrite / workspace-nav engines |
| `cypher-lsp` | Language server binary (+ `web-lsp` feature for wasm32) |
| `cypher-agent` | JSON-over-stdio agent API binary |
| `cypher-cli` | `cypher {check,fmt,plan,explain,schema,index,project}` |
| `cypher-tck` | openCypher TCK harness (v1 + full) |
| `cypher-testkit` | Shared test fixtures, compiletest runner (dev only) |
| `cypher` | Meta-crate re-exporting the library surface |
| `cypher-wasm` | `wasm-bindgen` adapter over the agent op surface |
| `cypher-ffi` | `cbindgen`-generated C ABI, opaque handles |
| `cypher-py` | PyO3 + maturin wheel (abi3-py310) |

Full graph + allowed edges:
[`docs/architecture.md`](./docs/architecture.md) and
[`docs/specs/0001-cypher-frontend.md`](./docs/specs/0001-cypher-frontend.md) §3.

---

## LSP in Neovim (5-minute demo)

No plugins required. Spin up the language server and watch it publish
diagnostics, format-on-save, and drive goto-definition against real
queries:

```sh
cargo build --release -p cypher-lsp
nvim -u demo/nvim/init.lua demo/samples/unclosed_paren.cyp
```

Full tour: [`demo/README.md`](./demo/README.md).

---

## Non-goals

- **No execution engine, runtime, or storage.** Consumers own that
  (spec §1.3 N1, §12.5).
- **No domain concepts.** The workspace is deliberately free of
  application vocabulary — CI greps for it (spec §2.C2).
- **No overlay crate host.** Domain extensions live in consumer
  repositories and plug in via the traits in spec §8 (spec §2.C3).
- **No `Neo4jCurrent` dialect in v1.** No APOC, no `EXISTS { }`
  subqueries, no `CALL { }`, no `LOAD CSV`, no `SHOW`, no `CYPHER`
  prefix directives (spec §9.3).

---

## Testing

Rust-compiler-grade. The bar is encoded in spec §17 and enforced by
`cargo xtask gate` on every commit:

```
cargo test --workspace                              # unit + integration + snapshots
cargo insta review                                  # snapshot review
cargo llvm-cov --workspace --html                   # coverage
cargo fuzz run fuzz_parser -- -max_total_time=300   # fuzz (nightly only)
cargo mutants -- -p cypher-sema                     # mutation testing
cargo bench --workspace                             # criterion benchmarks
cargo test -p cypher-tck --features full-tck        # full openCypher TCK
```

---

## Where to go next

- **Getting started:**
  [`docs/getting-started.md`](./docs/getting-started.md) — 5-minute tour.
- **Architecture:**
  [`docs/architecture.md`](./docs/architecture.md) — crate graph + layers + invariants.
- **Interop:**
  [`docs/interop.md`](./docs/interop.md) — WASM / FFI / Python / tree-sitter / LSP-Web.
- **Diagnostic codes:**
  [`docs/diagnostics.md`](./docs/diagnostics.md) — all 120 codes, indexed.
- **Performance:**
  [`docs/performance.md`](./docs/performance.md) — benches + methodology.
- **Stability:**
  [`docs/stability.md`](./docs/stability.md) — surface-by-surface contract.
- **Specs:** [`docs/README.md`](./docs/README.md) — normative architecture specs (0001–0004).
- **Examples:** [`examples/`](./examples/) — copy-pasteable mini-projects.
- **Contributing:** [`CONTRIBUTING.md`](./CONTRIBUTING.md).

---

## Status

v0.1.0. The spec is accepted, the CI gate is green, and the full
openCypher TCK runs on every change. Known gaps (tracked as open
beads): `CALL <proc> YIELD` standalone form, `shortestPath` pattern
functions, map projection, and incremental-reparse smart-path. None
block general use — they're language-coverage and perf deltas, not
correctness.

Pre-1.0 SemVer: Rust API can evolve through minor bumps;
`#[non_exhaustive]` is applied where applicable and `cargo-semver-checks`
gates PRs. Diagnostic codes, schema file format, and the agent wire
protocol are stable already — full matrix in
[`docs/stability.md`](./docs/stability.md).

---

## Tree-sitter grammar

A parallel [tree-sitter][ts] grammar for Cypher / GQL lives at
[`tree-sitter-cypher/`](./tree-sitter-cypher) for editor integrations
(Neovim, Helix, GitHub highlighter). The Rust parser in `cypher-syntax`
is authoritative; the grammar is a hand-maintained artefact kept in
lockstep by the `cargo xtask tree-sitter-parity` gate.

**Parity claim:** the grammar parses the same TCK v1 surface as the
Rust parser — every `outcome = "ok"` scenario in
`crates/cypher-tck/tck/v1.toml` parses without `(ERROR)` nodes, every
`outcome = "error"` scenario produces at least one. Regressions fail
CI.

### Neovim (nvim-treesitter)

```lua
local parser_config = require("nvim-treesitter.parsers").get_parser_configs()
parser_config.cypher = {
  install_info = {
    url = "https://github.com/phall1/cyrs",
    location = "tree-sitter-cypher",
    files = { "src/parser.c" },
    branch = "main",
    generate_requires_npm = true,
    requires_generate_from_grammar = true,
  },
  filetype = "cypher",
}
```

Then `:TSInstall cypher`.

### Helix (`~/.config/helix/languages.toml`)

```toml
[[language]]
name = "cypher"
scope = "source.cypher"
file-types = ["cyp", "cypher"]
roots = []
comment-token = "//"

[[grammar]]
name = "cypher"
source = { git = "https://github.com/phall1/cyrs", subpath = "tree-sitter-cypher" }
```

Then `hx --grammar fetch && hx --grammar build`.

See [`tree-sitter-cypher/README.md`](./tree-sitter-cypher/README.md) for
the full scope list and developer workflow.

[ts]: https://tree-sitter.github.io/

---

## Development

After cloning, install the pre-commit hook so `cargo xtask gate` runs
automatically on every commit:

```sh
bash scripts/install-hooks.sh
```

The gate runs `cargo fmt --check`, `cargo clippy -D warnings`,
`cargo test`, `cargo deny check`, non-coupling greps (§2 denylist),
the diagnostic-code registry lint, recovery-code budget, and
`cbindgen --check`.

---

## Agent context

[`AGENTS.md`](./AGENTS.md) is the canonical operating manual for
AI agents working on this workspace. Commits cite the spec section
and the corresponding bead ID (`cy-{3char}`). Beads live at `br` and
track ongoing work (see [beads_rust](https://github.com/Dicklesworthstone/beads_rust)).

---

## License

Dual-licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
