# cypher — a Rust front-end for Cypher / GQL

Lexer, recovering parser, lossless CST, typed AST, HIR with name
resolution, schema-aware semantic analysis, diagnostics engine,
formatter, incremental analysis database, language server, agent-facing
JSON API, CLI. GQL-aligned and openCypher v9 compatible.

> Rust-compiler-grade. No execution. No domain coupling.

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
format-on-save, CLI comparison).

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
| `cypher-lsp`     | Language server binary                                       |
| `cypher-agent`   | JSON-over-stdio agent API binary                             |
| `cypher-cli`     | `cypher {parse,check,fmt,explain,plan}`                      |
| `cypher-tck`     | openCypher TCK harness                                       |
| `cypher-testkit` | Shared test fixtures, compiletest runner (dev only)          |
| `cypher`         | Meta-crate re-exporting the library surface                  |

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

## Development

After cloning, install the pre-commit hook so `cargo xtask gate`
runs automatically on every commit:

```sh
bash cypher/scripts/install-hooks.sh
```

The gate runs `cargo fmt --check`, `cargo clippy -D warnings`,
`cargo test`, and `cargo deny check` against the workspace.

---

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
