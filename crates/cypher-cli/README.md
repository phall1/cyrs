# cypher-cli

Command-line tool for the [cyrs](https://github.com/phall1/cyrs) Cypher /
GQL frontend. Installs a `cypher` binary with subcommands for `parse`,
`check`, `fmt`, `plan`, and `explain`. See spec 0001 §16.

```sh
cargo install cypher-cli
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
