# cyrs-cli

[![crates.io](https://img.shields.io/crates/v/cyrs-cli.svg)](https://crates.io/crates/cyrs-cli)
[![docs.rs](https://img.shields.io/docsrs/cyrs-cli)](https://docs.rs/cyrs-cli)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Command-line tool for the [cyrs](https://github.com/phall1/cyrs) Cypher /
GQL frontend. Installs a `cypher` binary with subcommands for `parse`,
`check`, `fmt`, `plan`, and `explain`. See spec 0001 §16.

```sh
cargo install cyrs-cli
cypher parse demo/samples/good.cyp
```

For the full story — architecture, dependency graph, and testing bar — see
the [repo-root README](https://github.com/phall1/cyrs#readme).

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
