# cypher

Meta-crate for the [cyrs](https://github.com/phall1/cyrs) Cypher / GQL
frontend. Re-exports the library surface — lossless CST, typed AST, HIR,
semantic analysis, schema-aware checks, diagnostics, formatter, and plan
IR — behind feature-gated slices.

```toml
[dependencies]
cypher = "0.0.1"
```

Feature slices (see spec 0001 §17.15):

- `core` — lexer, parser, AST, HIR, sema, diagnostics, plan, DB
- `fmt`, `fmt-only` — formatter surface
- `schema`, `schema-only` — schema / type-system surface
- `lsp-only` — library surface consumed by the LSP binary

For the full story — architecture, dependency graph, and testing bar — see
the [repo-root README](https://github.com/phall1/cyrs#readme).

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
