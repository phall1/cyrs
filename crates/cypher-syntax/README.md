# cyrs-syntax

[![crates.io](https://img.shields.io/crates/v/cyrs-syntax.svg)](https://crates.io/crates/cyrs-syntax)
[![docs.rs](https://img.shields.io/docsrs/cyrs-syntax)](https://docs.rs/cyrs-syntax)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Lossless concrete syntax tree (CST) and recovering parser for Cypher / GQL.
Layer 1 of the [cyrs](https://github.com/phall1/cyrs) frontend stack.

Built on [`rowan`](https://crates.io/crates/rowan) and
[`logos`](https://crates.io/crates/logos). The parser recovers from syntax
errors so editor-grade tooling (LSP, formatter) can keep analysing partial
code. See spec 0001 §4.

For the full story — architecture, dependency graph, and testing bar — see
the [repo-root README](https://github.com/phall1/cyrs#readme).

## Span convention

cyrs uses **byte offsets** for every span — `TextRange` is the canonical
type and embedders should treat its endpoints as half-open byte indices
into the original source. Line/column conversion is owned by
[`LineIndex`](https://docs.rs/cyrs-syntax/latest/cyrs_syntax/struct.LineIndex.html)
and only happens at LSP and diagnostic-render boundaries; everything
inside the analysis pipeline (parser, HIR, diagnostics, formatter) stays
in byte-offset land. The [`TextRangeExt`] trait in
[`range_ext`](src/range_ext.rs) provides the canonical sugar
(`as_byte_range`, `as_u32_range`, `intersects`) so consumers don't roll
their own one-line conversions.

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
