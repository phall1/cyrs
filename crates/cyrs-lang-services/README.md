# cyrs-lang-services

[![crates.io](https://img.shields.io/crates/v/cyrs-lang-services.svg)](https://crates.io/crates/cyrs-lang-services)
[![docs.rs](https://img.shields.io/docsrs/cyrs-lang-services)](https://docs.rs/cyrs-lang-services)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Neutral language-service engines for Cypher / GQL: completion, hover, and
rewrite. Pure functions keyed on `(db, file_id, byte_offset)`; consumed by
both the LSP binary and the JSON agent binary with zero duplication. See
spec 0001 §14 / §15.

For the full story — architecture, dependency graph, and testing bar — see
the [repo-root README](https://github.com/phall1/cyrs#readme).

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
