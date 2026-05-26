# cyrs

[![crates.io](https://img.shields.io/crates/v/cyrs-lang.svg)](https://crates.io/crates/cyrs-lang)
[![docs.rs](https://img.shields.io/docsrs/cyrs-lang)](https://docs.rs/cyrs-lang)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](./rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

A compiler front-end for Cypher and GQL, written in Rust. Lex, parse,
type-check, lint, format, and lower queries to a logical plan IR.
cyrs does not execute queries — that is the database's job.

![demo](./demo/demo.gif)

## Install

```sh
cargo install cyrs-cli
```

## Use

```sh
cypher parse demo/samples/good.cyp
cypher check demo/samples/unknown_var.cyp
cypher fmt   demo/samples/needs_fmt.cyp
```

As a library: [`cyrs-lang`](https://crates.io/crates/cyrs-lang).
As a language server: `cyrs-lsp` — see [`demo/`](./demo).

## Coverage

openCypher v9 TCK: 3 632 / 3 897 scenarios accepted (93.2 %).
GQL ISO/IEC 39075:2024 bootstrap: 140 / 140 scenarios accepted
(100 %); 153 of 574 grammar productions reached (26.7 %).
All numbers measure parser acceptance, not runtime conformance.
Breakdown: [`docs/coverage.md`](./docs/coverage.md).

## Docs

- [`docs/overview.md`](./docs/overview.md) — what each layer is, in plain words.
- [`docs/concepts/`](./docs/concepts) — per-layer concept guides.
- [`docs/integration-depth.md`](./docs/integration-depth.md) — decision table for which layer to consume.
- [`docs/specs/`](./docs/specs) — normative architecture commitments.
- [`AGENTS.md`](./AGENTS.md) — context for AI agents working on this repo.

## Status

0.1.0 on crates.io. Pre-1.0; the API is depend-able, with minor breakage
possible on the path to 1.0. Surface-by-surface stability contract:
[`docs/stability.md`](./docs/stability.md).

## License

Dual-licensed under [Apache-2.0](./LICENSE-APACHE) OR [MIT](./LICENSE-MIT).
Contributions are dual-licensed under the same terms unless stated
otherwise.
