# cyrs-db

[![crates.io](https://img.shields.io/crates/v/cyrs-db.svg)](https://crates.io/crates/cyrs-db)
[![docs.rs](https://img.shields.io/docsrs/cyrs-db)](https://docs.rs/cyrs-db)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Salsa-backed incremental analysis database tying the Cypher / GQL frontend
layers together. Editor-grade latency: on edit, only the affected queries
re-run. See spec 0001 §11.

For the full story — architecture, dependency graph, and testing bar — see
the [repo-root README](https://github.com/phall1/cyrs#readme).

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
