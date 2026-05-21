# cyrs — a Rust front-end for Cypher / GQL

[![crates.io](https://img.shields.io/crates/v/cyrs-lang.svg)](https://crates.io/crates/cyrs-lang)
[![docs.rs](https://img.shields.io/docsrs/cyrs-lang)](https://docs.rs/cyrs-lang)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](./rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Lexer, recovering parser, lossless CST, typed AST, HIR with name
resolution, schema-aware semantic analysis, diagnostics engine,
formatter, incremental analysis database, language server, agent-facing
JSON API, CLI. openCypher v9 front-end (93.2 % TCK acceptance) and
GQL ISO/IEC 39075:2024 parser-acceptance bootstrap (**18 / 18 = 100 %**
on the hand-authored §-cited corpus; see [coverage](#coverage)).

> Rust-compiler-grade. No execution. No domain coupling.

![cyrs-lsp + nvim demo](./demo/demo.gif)

---

## What this is

A compiler front-end for a query language — not a database. Downstream
consumers execute the typed Plan IR against their own storage.

```
  Cypher / GQL text
        │
        ▼
  cyrs-syntax      lexer, recovering parser, lossless CST
        │
        ▼
  cyrs-ast         typed AST wrappers over the CST
        │
        ▼
  cyrs-hir         lowered HIR, name resolution, scope graph
        │
        ▼
  cyrs-sema        type system + semantic analysis (schema-aware)
        │
        ▼
  cyrs-plan        logical read / write Plan IR
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

### Embedders: choosing your integration depth

cyrs has six plausible consumption layers (CST, AST, HIR, sema, Plan,
agent JSON). Which one *you* should consume depends on what you're
building (graph database vs. IDE vs. rewriter vs. parser-bench).
[`docs/integration-depth.md`](./docs/integration-depth.md) is the
decision table + per-layer reference that answers that question
before you `cargo add` anything.

---

## Quickstart

```sh
cargo install cyrs-cli
cypher parse demo/samples/good.cyp
cypher fmt   demo/samples/needs_fmt.cyp
cypher check demo/samples/unknown_var.cyp
```

`cyrs-cli` ships the `cypher` binary with `parse`, `check`, `fmt`,
`plan`, `explain`, and schema-file operations (`schema load`,
`schema check`, `schema diff`; see
[spec 0002](docs/specs/0002-schema-file-format.md)). The Rust API is
available as the [`cyrs-lang`](https://crates.io/crates/cyrs-lang) meta-crate.

---

## Features

- **Lossless CST** — every byte preserved, round-trip guaranteed.
- **Recovering parser** — editor-grade: one bad token does not cascade.
- **Typed AST** — codegen'd from `cypher.ungrammar`; zero hand-written
  accessors.
- **Scope graph + name resolution** — HIR layer handles `WITH`, `UNWIND`,
  aggregation scopes, and pattern bindings.
- **Schema-aware semantic analysis** — schema is a trait
  (`cyrs-schema::SchemaProvider`); no hard-coded assumptions.
- **Clippy-equivalent lints** — a starter pack of 6 style / bug-shape
  lints (`W6011`–`W6016`); opt-in via `cypher check --lints` and the
  LSP `lints` option. See the [lint table](#lints) below.
- **Stable diagnostic codes** — `E0001…`, `W6000…`, `N8000…`. See spec
  §10. Codes are SemVer — once assigned, meaning never changes.
- **Idempotent formatter** — `fmt(fmt(x)) == fmt(x)`, round-trips through
  the parser.
- **Salsa-backed incremental DB** — `cyrs-db` re-computes only the
  affected queries on every edit.
- **LSP server + JSON agent API** — share a single
  `cyrs-lang-services` engine layer; zero logic duplication.

---

## Coverage

cyrs is a **Cypher front-end** with first-class GQL ISO/IEC 39075:2024
parser support. The numbers below are rolling measurements written by
the TCK harness (spec §17.5), not aspirations.

| Surface | Corpus | Result | Source |
| ------- | ------ | ------ | ------ |
| openCypher v9 | upstream openCypher TCK `2024.3` (220 feature files, 3 897 expanded scenarios) | **3 632 / 3 897 accepted (93.2 %)** | [`crates/cyrs-tck/tck/full-baseline.md`](./crates/cyrs-tck/tck/full-baseline.md) |
| GQL ISO/IEC 39075:2024 | hand-authored §-cited bootstrap (7 feature files, 18 expanded scenarios) | **18 / 18 accepted (100 %)** | [`crates/cyrs-tck/tck/gql-iso-39075/baseline.md`](./crates/cyrs-tck/tck/gql-iso-39075/baseline.md) |
| GQL ISO/IEC 39075:2024 (upstream samples) | OpenGQL `opengql/grammar` samples (14 files) | **14 / 14 accepted (100 %)** — full coverage after cy-51we / cy-rgqg / cy-9kzx / cy-p1u5 | [`crates/cyrs-tck/tck/opengql-samples/baseline.md`](./crates/cyrs-tck/tck/opengql-samples/baseline.md) |

**Read both numbers carefully — they mean different things.** Both
report **parser acceptance** ("the parser emits zero syntax errors
for the `When executing query:` step"), not end-to-end conformance.
The front-end does no execution (spec §1.3 N1), so neither number
asserts runtime semantics. Concretely:

- **openCypher 93.2 %.** Measured against the full 3 897-scenario
  upstream TCK. `Expected::Error` scenarios are still untriaged
  (`Expected::Ignored`); see the baseline file's preamble.
- **GQL 100 %.** Measured against a *hand-authored bootstrap* of
  18 scenarios that pin the GQL-distinct surface (one feature file
  per area, each scenario citing its ISO/IEC 39075:2024 §). ISO does
  not publish a public conformance test corpus for GQL, so the
  bootstrap is the corpus. Going beyond 18 scenarios is corpus
  growth, not parser work.

### What the GQL bootstrap covers (all green)

Each row maps a GQL-distinct construct to the ISO § it derives from
and the bead that landed it. See the per-feature files under
[`crates/cyrs-tck/tck/gql-iso-39075/features/`](./crates/cyrs-tck/tck/gql-iso-39075/features/)
for the literal scenarios.

| Construct | ISO § | Feature file | Bead |
| --------- | ----- | ------------ | ---- |
| `INSERT NODE` / `INSERT EDGE` (vs Cypher `CREATE`) | §13.4 | `clauses/Insert1.feature` | cy-8z3 |
| `OPTIONAL CALL` (empty-multiset on failure) | §14.11.3 | `clauses/Optional1.feature` | cy-tdl |
| `RETURN ALL` / `RETURN ... EXCLUDE <field>` | §14.13 | `clauses/Return1.feature` | cy-auh |
| `FILTER` (post-projection row filter) | §14.10 | `clauses/Filter1.feature` | cy-r50 |
| `IS TYPED <T>` predicate + `::` cast | §6.5.2 / §6.2 | `types/SchemaTypes1.feature` | cy-pnp |
| `ANY SHORTEST` / `ALL SHORTEST` / `SHORTEST k` + `->+` quantifier | §10.4.2 / §10.5 | `paths/PathSelector1.feature` | cy-3mq |
| `REPEATABLE ELEMENTS` / `DIFFERENT EDGES` + `->{m,n}` quantifier | §10.6.3 | `values/Repeatable1.feature` | cy-q2g |

**Out of scope for the bootstrap** (parser acceptance only — these
are corpus-growth follow-ups, not parser bugs): full ISO 39075
surface coverage, schema DDL (`CREATE GRAPH TYPE` etc.),
transaction-control statements, catalog / authorisation statements
(§14.14, §14.15), procedure-result-set introspection, and anything
`Neo4jCurrent`-only (spec 0001 §9.3).

**Dialect routing.** The parser emits the same CST for both dialects;
the [`DialectMode`](./crates/cyrs-db/src/lib.rs) selector lives at the
analysis layer and gates GQL-only / Cypher-only constructs via the
`E4xxx` diagnostic codes (see [`crates/cyrs-sema/src/dialect.rs`](./crates/cyrs-sema/src/dialect.rs)).

---

## Lints

Beyond the error-severity semantic checks, cyrs ships a
**clippy-equivalent lint pack** — warning-severity diagnostics for
queries that parse and analyse cleanly but are stylistically poor or
likely a bug. Each lint carries a `note:` fix hint. Lints live in
[`crates/cyrs-sema/src/lints/`](./crates/cyrs-sema/src/lints).

Lints are **opt-in** (off by default until the pack stabilises):

- CLI — `cypher check --lints` runs the pass and prints lints alongside
  the analysis diagnostics. Lints never change the exit code.
- LSP — set `initializationOptions.lints` to `true`; lints surface as
  `Information`-severity diagnostics.
- Manifest — each lint maps to a rule name in `cypher-project.toml`'s
  lint registry (`cyrs-project`).

| Code | Lint | Fires when | Rule name |
| ----- | ---- | ---------- | --------- |
| `W6011` | unused pattern variable | a `MATCH` binder is never referenced downstream | `unused-pattern-var` |
| `W6012` | redundant `MATCH` | a `MATCH` exactly duplicates an earlier one | `redundant-match` |
| `W6013` | unrestricted pattern | a node/relationship pattern has no label / type (schema-aware) | `unrestricted-pattern` |
| `W6014` | implicit cartesian product | two `MATCH` clauses share no variable or join predicate | `cartesian-product` |
| `W6015` | wide `RETURN *` | `RETURN *` in a statement binding more than N variables | `wildcard-return` |
| `W6016` | `OPTIONAL MATCH` + `WHERE` on its binding | a trailing `WHERE` constrains the optional binding (defeats `OPTIONAL`) | `optional-match-where` |

`W6012` (redundant `MATCH`) and `W6014` (cartesian product) are
deliberately conservative — they fire only on unambiguous cases and
prefer to miss the harder ones over warning wrongly.

---

## Status

**0.1.0 on crates.io.** All workspace crates (`cyrs-syntax`, `cyrs-ast`,
`cyrs-hir`, `cyrs-sema`, `cyrs-schema`, `cyrs-plan`, `cyrs-fmt`,
`cyrs-diag`, `cyrs-db`, `cyrs-lang-services`, `cyrs-lsp`, `cyrs-agent`,
`cyrs-cli`, `cyrs-tck`, `cyrs-lang`, `cyrs-wasm`, `cyrs-ffi`, `cyrs-py`,
`cyrs-project`) are published. 0.1.0 is an initial release — the API
surface is stable enough to depend on but expect minor breakage on the
path to 1.0.

The spec is accepted and locked. Start there:
[`docs/specs/0001-cypher-frontend.md`](./docs/specs/0001-cypher-frontend.md).
Twenty-three numbered sections from scope through testing. Before
adding features, touching architecture, or filing issues, open the
spec and reference section numbers.

---

## Try it

A no-plugin Neovim walkthrough that spins up the language server,
publishes diagnostics, and runs format-on-save against real queries:

```sh
cargo build --release -p cyrs-lsp
nvim -u demo/nvim/init.lua demo/samples/unclosed_paren.cyp
```

See [`demo/README.md`](./demo/README.md) for the full tour (samples,
format-on-save, CLI comparison) and [`demo/demo.gif`](./demo/demo.gif)
for the recording.

For VS Code / VSCodium, the language client lives at
[`editors/vscode/`](./editors/vscode) — see its
[README](./editors/vscode/README.md) for dev-install instructions
(marketplace publishing is a manual maintainer step).

---

## Crates

| Crate            | Purpose                                                      |
| ---------------- | ------------------------------------------------------------ |
| `cyrs-syntax`  | Lexer, recovering parser, lossless CST, `SyntaxKind`         |
| `cyrs-ast`     | Typed AST wrappers over the CST                              |
| `cyrs-hir`     | Lowered HIR, name resolution, scope graph, desugaring        |
| `cyrs-sema`    | Semantic analysis + type system                              |
| `cyrs-schema`  | `SchemaProvider` trait + supporting types                    |
| `cyrs-diag`    | Diagnostic type, stable code registry, rendering backends    |
| `cyrs-plan`    | Logical read / write plan IR                                 |
| `cyrs-fmt`     | CST-driven formatter                                         |
| `cyrs-db`      | Salsa-based incremental analysis database                    |
| `cyrs-lang-services` | Shared completion / hover / rewrite engines            |
| `cyrs-lsp`     | Language server binary                                       |
| `cyrs-agent`   | JSON-over-stdio agent API binary                             |
| `cyrs-cli`     | `cypher {parse,check,fmt,explain,plan}`                      |
| `cyrs-tck`     | openCypher TCK harness                                       |
| `cyrs-testkit` | Shared test fixtures, compiletest runner (dev only)          |
| `cyrs-lang`    | Meta-crate re-exporting the library surface                  |

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
`cyrs-syntax` is authoritative; the tree-sitter grammar is a
hand-maintained artefact kept in lock-step by the
`cargo xtask tree-sitter-parity` gate.

**Parity claim:** the grammar parses the same TCK v1 surface as the Rust
parser — every `outcome = "ok"` scenario in
`crates/cyrs-tck/tck/v1.toml` parses without `(ERROR)` nodes, every
`outcome = "error"` scenario produces at least one. Regressions fail CI.

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

## Testing

Spec §17 grades testing to the rust-compiler standard:

```
cargo test --workspace                              # unit + integration + snapshots
cargo insta review                                  # snapshot review
cargo llvm-cov --workspace --html                   # coverage
cargo fuzz run fuzz_parser -- -max_total_time=300   # fuzz (nightly only)
cargo mutants -- -p cyrs-sema                     # mutation testing
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
