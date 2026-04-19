# cypher — a Rust front-end for Cypher / GQL

A standalone, domain-free front-end platform for the Cypher query language
(GQL-aligned and openCypher v9 compatible): lexer, recovering parser,
lossless CST, typed AST, HIR with name resolution, schema-aware semantic
analysis, a diagnostics engine, a formatter, an incremental analysis
database, a language server, an agent-facing JSON API, and a CLI.

**No execution.** Consumers execute the typed Plan IR against their own
storage.

**No domain coupling.** Graph stores, analytic engines, IDE integrations,
and agent tooling are all equally-weighted downstream consumers. Schema,
custom functions, and write-clause semantics are plugged in via trait
implementations — not baked into this workspace.

## Design

Start with the spec: [`docs/specs/0001-cypher-frontend.md`][spec].
Twenty-three numbered sections from scope through testing. Before adding
features, touching architecture, or filing issues, open the spec and
reference section numbers.

[spec]: docs/specs/0001-cypher-frontend.md

## Crates

| Crate            | Purpose                                                      |
| ---------------- | ------------------------------------------------------------ |
| `cypher-syntax`  | Lexer, recovering parser, lossless CST, `SyntaxKind`         |
| `cypher-ast`     | Typed AST wrappers over the CST                              |
| `cypher-hir`     | Lowered HIR, name resolution, scope graph, desugaring        |
| `cypher-sema`    | Semantic analysis + type system                              |
| `cypher-schema`  | `SchemaProvider` trait + supporting types                    |
| `cypher-diag`    | Diagnostic type, stable code registry, rendering backends    |
| `cypher-plan`    | Logical read/write plan IR                                   |
| `cypher-fmt`     | CST-driven formatter                                         |
| `cypher-db`      | Salsa-based incremental analysis database                    |
| `cypher-lsp`     | Language server binary                                       |
| `cypher-agent`   | JSON-over-stdio agent API binary                             |
| `cypher-cli`     | `cypher {parse,check,fmt,explain,plan}`                      |
| `cypher-tck`     | openCypher TCK harness                                       |
| `cypher-testkit` | Shared test fixtures, compiletest runner (dev only)          |
| `cypher`         | Meta-crate re-exporting the library surface                  |

## Status

Pre-0.1. The spec is accepted and locked; implementation is in progress.
Expect breakage. Do not use in production yet.

## Testing

Spec §17 (testing at rust-compiler-grade):

```
cargo test --workspace                    # unit + integration + snapshots
cargo insta review                         # snapshot review
cargo llvm-cov --workspace --html          # coverage
cargo fuzz run fuzz_parser -- -max_total_time=300   # fuzz (nightly only)
cargo mutants -- -p cypher-sema            # mutation testing
cargo bench --workspace                    # criterion benchmarks
```

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
