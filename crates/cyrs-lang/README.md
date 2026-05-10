# cypher

[![crates.io](https://img.shields.io/crates/v/cyrs-lang.svg)](https://crates.io/crates/cyrs-lang)
[![docs.rs](https://img.shields.io/docsrs/cyrs-lang)](https://docs.rs/cyrs-lang)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

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
