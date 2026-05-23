# Changelog — `cyrs-syntax`

All notable changes to `cyrs-syntax` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- cy-71t0 (ISO/IEC 39075:2024 §14.13.3): parse `GROUP BY <expr-list>`
  in `RETURN_CLAUSE`.  New keyword `GROUP_KW` (slot 197), new CST node
  `GROUP_BY` (slot 414), new parser function `group_by()` slotting
  between EXCLUDE and ORDER BY.  Recovery codes `E0099`
  (`EXPECTED_BY_AFTER_GROUP`) and `E0100` (`EXPECTED_GROUPBY_EXPR`)
  each have a UI fixture under `tests/ui/syntax/`.
- cy-dwem (ISO/IEC 39075:2024 §20.1): parse `x IS [NOT] TRUE`,
  `x IS [NOT] FALSE`, `x IS [NOT] UNKNOWN` (`truthValuePredicatePart2`
  / `truthValue`).  New keyword `UNKNOWN_KW` (slot 202; `TRUE_KW` and
  `FALSE_KW` were already reserved), new CST node
  `TRUTH_VALUE_PREDICATE` (slot 421).  Wired into the postfix-IS
  dispatch in `expression.rs` alongside `IS NULL` and `IS TYPED`.  No
  new diagnostic codes are registered — the surrounding dispatch
  recovers via the existing `E0025` (`EXPECTED_NULL_AFTER_IS`) path,
  so the legacy diagnostic surface is preserved for queries that
  simply forgot to spell `NULL`.

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
