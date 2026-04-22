# Changelog — `cypher-schema`

All notable changes to `cypher-schema` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

* cy-0ek.1 (spec 0002): `InMemorySchema` + `RelDecl`, builder API, and
  `file::{load_from_toml_str, load_from_toml_path, serialise_to_toml}`
  behind the new `file` feature.  Round-trip integration test at
  `tests/file.rs`.
* cy-0ek (spec 0002 §9): `lint::lint` surfaces schema-file lints with
  stable codes `E3010` (opaque type), `E3011` (self-referential rel
  type), and `W6010` (unreachable label).
* cy-0ek (spec 0002 §9.4): `diff::diff` computes a deterministic,
  serde-serialisable `SchemaDiff` between two schemas with `adds`,
  `removes`, and `breaking` buckets — the CI "schema-compat" gate.

### Changed

### Deprecated

### Removed

### Fixed

### Security

<!--
Maintainer notes:

* Group entries by audience (breaking, new features, fixes) not by commit.
* Reference beads and spec sections: `cy-xxx (§N.N): one-line summary`.
* On release, rename `[Unreleased]` to `[X.Y.Z] — YYYY-MM-DD` and add a
  fresh empty `[Unreleased]` block above it.
* Keep entries terse — one line each is fine.  Detailed notes belong in
  the merge commit.
-->
