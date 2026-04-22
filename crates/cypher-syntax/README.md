# cypher-syntax

Lossless concrete syntax tree (CST) and recovering parser for Cypher / GQL.
Layer 1 of the [cyrs](https://github.com/phall1/cyrs) frontend stack.

Built on [`rowan`](https://crates.io/crates/rowan) and
[`logos`](https://crates.io/crates/logos). The parser recovers from syntax
errors so editor-grade tooling (LSP, formatter) can keep analysing partial
code. See spec 0001 §4.

For the full story — architecture, dependency graph, and testing bar — see
the [repo-root README](https://github.com/phall1/cyrs#readme).

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
