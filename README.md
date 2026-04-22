# cypher — a Rust front-end for Cypher / GQL

[![crates.io](https://img.shields.io/crates/v/cypher.svg)](https://crates.io/crates/cypher)
[![docs.rs](https://img.shields.io/docsrs/cypher)](https://docs.rs/cypher)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](./rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Lexer, recovering parser, lossless CST, typed AST, HIR with name
resolution, schema-aware semantic analysis, diagnostics engine,
formatter, incremental analysis database, language server, agent-facing
JSON API, CLI. GQL-aligned and openCypher v9 compatible.

> Rust-compiler-grade. No execution. No domain coupling.

![cypher-lsp + nvim demo](./demo/demo.gif)

---

## What this is

A compiler front-end for a query language — not a database. Downstream
consumers execute the typed Plan IR against their own storage.

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
  cypher-hir         lowered HIR, name resolution, scope graph
        │
        ▼
  cypher-sema        type system + semantic analysis (schema-aware)
        │
        ▼
  cypher-plan        logical read / write Plan IR
        │
        ▼
  consumer executes against its own storage
```

Schema, custom functions, and write-clause semantics are plugged in
through trait implementations — not baked into this workspace. Graph
stores, analytic engines, IDE integrations, and agent tooling are
equal-weight downstream consumers.

The authoritative crate graph and allowed-edges list lives in
[`docs/specs/0001-cypher-frontend.md`](./docs/specs/0001-cypher-frontend.md)
§3.

### Specs

- [`0001-cypher-frontend.md`](./docs/specs/0001-cypher-frontend.md) —
  architecture, crate graph, testing bar.
- [`0002-schema-file-format.md`](./docs/specs/0002-schema-file-format.md) —
  `schema.toml` file format + diff.
- [`0003-project-manifest.md`](./docs/specs/0003-project-manifest.md) —
  `cypher-project.toml` workspace manifest.
- [`0004-interop-surfaces.md`](./docs/specs/0004-interop-surfaces.md) —
  WASM, C FFI, PyO3, LSP-Web, tree-sitter parity.

---

## Quickstart

```sh
cargo install cypher-cli
cypher parse demo/samples/good.cyp
cypher fmt   demo/samples/needs_fmt.cyp
cypher check demo/samples/unknown_var.cyp
```

`cypher-cli` ships the `cypher` binary with `parse`, `check`, `fmt`,
`plan`, `explain`, and schema-file requests (`schema load`,
`schema check`, `schema diff`; see
[spec 0002](docs/specs/0002-schema-file-format.md)). The Rust API is
available as the [`cypher`](https://crates.io/crates/cypher) meta-crate.

---

## Features

- **Lossless CST** — every byte preserved, round-trip guaranteed.
- **Recovering parser** — editor-grade: one bad token does not cascade.
- **Typed AST** — codegen'd from `cypher.ungrammar`; zero hand-written
  accessors.
- **Scope graph + name resolution** — HIR layer handles `WITH`, `UNWIND`,
  aggregation scopes, and pattern bindings.
- **Schema-aware semantic analysis** — schema is a trait
  (`cypher-schema::SchemaProvider`); no hard-coded assumptions.
- **Stable diagnostic codes** — `E0001…`, `W6000…`, `N8000…`. See spec
  §10. Codes are SemVer — once assigned, meaning never changes.
- **Idempotent formatter** — `fmt(fmt(x)) == fmt(x)`, round-trips through
  the parser.
- **Salsa-backed incremental DB** — `cypher-db` re-computes only the
  affected queries on every edit.
- **LSP server + JSON agent API** — share a single
  `cypher-lang-services` engine layer; zero logic duplication.

---

## Status

Pre-0.1. The spec is accepted and locked; implementation is in progress.
Expect breakage. Do not use in production yet.

Start with the spec:
[`docs/specs/0001-cypher-frontend.md`](./docs/specs/0001-cypher-frontend.md).
Twenty-three numbered sections from scope through testing. Before adding
features, touching architecture, or filing issues, open the spec and
reference section numbers.

---

## Try it

A no-plugin Neovim walkthrough that spins up the language server,
publishes diagnostics, and runs format-on-save against real queries:

```sh
cargo build --release -p cypher-lsp
nvim -u demo/nvim/init.lua demo/samples/unclosed_paren.cyp
```

See [`demo/README.md`](./demo/README.md) for the full tour (samples,
format-on-save, CLI comparison) and [`demo/demo.gif`](./demo/demo.gif)
for the recording.

---

## Crates

| Crate            | Purpose                                                      |
| ---------------- | ------------------------------------------------------------ |
| `cypher-syntax`  | Lexer, recovering parser, lossless CST, `SyntaxKind`         |
| `cypher-ast`     | Typed AST wrappers over the CST                              |
| `cypher-hir`     | Lowered HIR, name resolution, scope graph, desugaring        |
| `cypher-sema`    | Semantic analysis + type system                              |
| `cypher-schema`  | `SchemaProvider` trait + supporting types                    |
| `cypher-diag`    | Diagnostic type, stable code registry, rendering backends    |
| `cypher-plan`    | Logical read / write plan IR                                 |
| `cypher-fmt`     | CST-driven formatter                                         |
| `cypher-db`      | Salsa-based incremental analysis database                    |
| `cypher-lang-services` | Shared completion / hover / rewrite engines            |
| `cypher-lsp`     | Language server binary                                       |
| `cypher-agent`   | JSON-over-stdio agent API binary                             |
| `cypher-cli`     | `cypher {parse,check,fmt,explain,plan}`                      |
| `cypher-tck`     | openCypher TCK harness                                       |
| `cypher-testkit` | Shared test fixtures, compiletest runner (dev only)          |
| `cypher`         | Meta-crate re-exporting the library surface                  |

---

## Non-goals

- No execution engine, runtime, or storage. Consumers own that
  (spec §1.3 N1, §12.5).
- No domain concepts. The workspace is deliberately free of application
  vocabulary — CI greps for it (spec §2.C2).
- No overlay crate host. Domain extensions live in consumer repositories
  and plug in via the traits in spec §8 (spec §2.C3).
- No `Neo4jCurrent` dialect in v1 — no APOC, no `EXISTS {}` subqueries,
  no `CALL { ... }`, no `LOAD CSV`, no `SHOW`, no `CYPHER` prefixes
  (spec §9.3).

---

## Tree-sitter grammar

A parallel [tree-sitter][ts] grammar for Cypher / GQL lives at
[`tree-sitter-cypher/`](./tree-sitter-cypher) for editor integrations
(Neovim, Helix, GitHub highlighter). The Rust parser in
`cypher-syntax` is authoritative; the tree-sitter grammar is a
hand-maintained artefact kept in lock-step by the
`cargo xtask tree-sitter-parity` gate.

**Parity claim:** the grammar parses the same TCK v1 surface as the Rust
parser — every `outcome = "ok"` scenario in
`crates/cypher-tck/tck/v1.toml` parses without `(ERROR)` nodes, every
`outcome = "error"` scenario produces at least one. Regressions fail CI.

### Neovim (nvim-treesitter)

```lua
local parser_config = require("nvim-treesitter.parsers").get_parser_configs()
parser_config.cypher = {
  install_info = {
    url = "https://github.com/phallsignup/cyrs",
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
source = { git = "https://github.com/phallsignup/cyrs", subpath = "tree-sitter-cypher" }
```

Then `hx --grammar fetch && hx --grammar build`.

See [`tree-sitter-cypher/README.md`](./tree-sitter-cypher/README.md) for
the full scope list and developer workflow.

[ts]: https://tree-sitter.github.io/

---

## Testing

Spec §17 grades testing to the rust-compiler standard:

```
cargo test --workspace                              # unit + integration + snapshots
cargo insta review                                  # snapshot review
cargo llvm-cov --workspace --html                   # coverage
cargo fuzz run fuzz_parser -- -max_total_time=300   # fuzz (nightly only)
cargo mutants -- -p cypher-sema                     # mutation testing
cargo bench --workspace                             # criterion benchmarks
```

---

## Agent context

[`AGENTS.md`](./AGENTS.md) is the canonical context an agent reads before
working on the front-end. Commits cite the spec section and the
corresponding bead ID (`cy-{3char}`). Beads live at `br` and track
ongoing work.
---

## Stability

Cyrs is pre-1.0; see [`docs/stability.md`](./docs/stability.md) for the
surface-by-surface stability contract (diagnostic codes, agent wire
protocol, schema file format, HIR / Plan IR shape, 1.0 cutover plan).
PRs are gated by `cargo-semver-checks`.

---

## Development

After cloning, install the pre-commit hook so `cargo xtask gate`
runs automatically on every commit:

```sh
bash scripts/install-hooks.sh
```

The gate runs `cargo fmt --check`, `cargo clippy -D warnings`,
`cargo test`, and `cargo deny check` against the workspace.

---

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
