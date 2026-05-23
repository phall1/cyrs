# Changelog — `cyrs-tck`

All notable changes to `cyrs-tck` are documented here.  The format is based
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
- cy-1x7o (§17.5): GQL grammar-coverage harness.  Every scenario
  carries a `@covers:` Gherkin tag naming the GQL.g4 parser
  productions it exercises; `tests/gql_iso.rs` validates the tags
  against `tck/opengql-grammar/rules.json` and emits
  `tck/gql-iso-39075/coverage.md` — how many of the 574 parser
  productions a passing scenario reaches, plus the uncovered-production
  worklist.  Fails on an unknown or missing `@covers:` tag.
- cy-1x7o (§17.5): `cargo xtask gql-coverage` convenience wrapper that
  regenerates both `baseline.md` and `coverage.md`.
- cy-a6ci / cy-gxda / cy-inah (§17.5): corpus growth batch —
  expressions, query composition, and graph-patterns areas (16 new
  feature files, 85 scenarios).  Grammar coverage 35/574 → 128/574
  (6.1% → 22.3%).
- cy-71t0 (§17.5, ISO/IEC 39075:2024 §14.13.3): `GroupBy1.feature`
  scenarios for `RETURN ... GROUP BY` alongside the new parser support
  in cyrs-syntax.  Adds `groupByClause`, `groupingElementList`,
  `groupingElement` to the covered-productions set.
- cy-z0x8 (§17.5, ISO/IEC 39075:2024 §14.13.6 / §14.13.7):
  `Page2.feature` scenarios for the GQL `OFFSET` synonym + `NULLS
  FIRST / LAST` sort-spec trailer alongside the new parser support in
  cyrs-syntax.  Adds `nullOrdering` to the covered-productions set
  and extends coverage for `offsetClause` / `offsetSynonym` to the
  explicit `OFFSET k` spelling.

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
