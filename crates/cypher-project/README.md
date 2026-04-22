# cypher-project

[![crates.io](https://img.shields.io/crates/v/cypher-project.svg)](https://crates.io/crates/cypher-project)
[![docs.rs](https://img.shields.io/docsrs/cypher-project)](https://docs.rs/cypher-project)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

`cypher-project.toml` manifest format and loader for the Cypher / GQL frontend
(spec 0003). Declares a project's members, shared schema, dialect, and
project-local lint levels. v0 ships the loader + validator; cross-file
analysis is a follow-up under the cy-o8c epic.

For the full story — architecture, dependency graph, and testing bar — see
the [repo-root README](https://github.com/phall1/cyrs#readme).

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
