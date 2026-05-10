# Changelog — `cypher-tck`

All notable changes to `cypher-tck` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- cy-p5q (§17.5): vendor the full openCypher TCK at tag `2024.3`
  under `tck/full/`, 220 feature files / 1339 scenarios.
- cy-p5q (§17.5): new Cargo feature `full-tck` that runs the full
  corpus via `tests/full.rs` and emits a per-area parser-acceptance
  baseline to `tck/full-baseline.md`.
- cy-p5q (§17.5): `cargo xtask tck-baseline` convenience wrapper.
- cy-0hj (§17.5): GQL ISO/IEC 39075:2024 conformance bootstrap corpus
  under `tck/gql-iso-39075/` — 7 feature files, 18 hand-authored
  scenarios with inline ISO §-citations covering `INSERT NODE/EDGE`,
  `FILTER`, `RETURN ALL/EXCLUDE`, `OPTIONAL CALL`, `REPEATABLE
  ELEMENTS` / `DIFFERENT EDGES`, `IS TYPED` / `::` casts, and path
  selectors (`ANY SHORTEST`, `ALL SHORTEST`, `SHORTEST k`).
- cy-0hj (§17.5): new Cargo feature `gql-iso` that runs the bootstrap
  corpus via `tests/gql_iso.rs` and emits a per-area parser-acceptance
  baseline to `tck/gql-iso-39075/baseline.md`.  Compliance badge is
  separate from the openCypher TCK.

### Changed

- cy-p5q (§17.5): retire `Expected::Green | Red` (per-tag) in favour
  of `Expected::Supported | Error | Ignored` (per-scenario).
  `v1_gates()` renamed to `v1_tags()` and now returns a flat
  whitelist.  The v1.toml on-disk format is unchanged.

### Deprecated

### Removed

- cy-p5q (§17.5): `FeatureGate` struct removed.  Use the per-scenario
  `Expected` enum instead.

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
