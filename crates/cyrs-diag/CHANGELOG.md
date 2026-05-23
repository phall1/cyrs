# Changelog — `cyrs-diag`

All notable changes to `cyrs-diag` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- cy-0ek (spec 0002 §9): register three new diagnostic codes consumed
  by the `cypher schema check` linter — `E3010` (opaque schema-file
  type), `E3011` (self-referential relationship type), and `W6010`
  (unreachable label).
- cy-71t0 (ISO/IEC 39075:2024 §14.13.3): register `E0099`
  (`EXPECTED_BY_AFTER_GROUP`) and `E0100` (`EXPECTED_GROUPBY_EXPR`)
  for the new GQL `GROUP BY` parser in cyrs-syntax.

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
