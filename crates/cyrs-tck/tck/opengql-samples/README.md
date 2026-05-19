# OpenGQL Grammar Samples — Conformance Baseline

This directory holds the upstream-vendored sample queries published by the
OpenGQL grammar project alongside their ANTLR4 grammar for ISO/IEC
39075:2024. See `VENDORED.md` for the pinned commit and refresh procedure.

## Scope

14 short `.gql` queries — one per file — exercising:

- Catalog DDL: `CREATE GRAPH`, `CREATE SCHEMA`, `CREATE …GRAPH TYPE`
  (both double-colon and lexical forms; nested graph types).
- Data DML: `INSERT` (node + edge, including temporal `DATE` literals).
- Mixed: `MATCH … INSERT`.
- Read: `MATCH` with `EXISTS { … }` predicates (three syntactic forms).
- Session: `SESSION SET GRAPH`, `SESSION SET PROPERTY GRAPH`,
  `SESSION SET <param> AS <value>`, `SESSION SET TIME ZONE`.

## Harness

`crates/cyrs-tck/tests/opengql_samples.rs` parses each file in
`DialectMode::GqlAligned` and writes an aggregate
`baseline.md` next to this README. The test **never fails** — it is a
rolling measurement, parallel to the hand-authored `gql-iso-39075`
bootstrap. Run it with:

```sh
cargo test -p cyrs-tck --features opengql-samples --test opengql_samples
```

## Relationship to other corpora

| Corpus                                | Source            | Role                            |
| ------------------------------------- | ----------------- | ------------------------------- |
| `tck/v1.toml` + `tck/full/`           | openCypher TCK v1 | openCypher conformance gate     |
| `tck/gql-iso-39075/features/`         | hand-authored     | GQL-distinct bootstrap baseline |
| `tck/opengql-samples/` *(this dir)*   | OpenGQL upstream  | Official sample smoke baseline  |

The three are independent: a regression in any one is attributable to
its source.
