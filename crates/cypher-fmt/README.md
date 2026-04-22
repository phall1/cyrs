# cypher-fmt

Formatter for Cypher / GQL source code. CST-driven and idempotent:
`fmt(fmt(x)) == fmt(x)`, and the output round-trips through the parser
byte-for-byte up to trailing whitespace. See spec 0001 §13.

For the full story — architecture, dependency graph, and testing bar — see
the [repo-root README](https://github.com/phall1/cyrs#readme).

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
