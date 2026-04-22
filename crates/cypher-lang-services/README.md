# cypher-lang-services

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
