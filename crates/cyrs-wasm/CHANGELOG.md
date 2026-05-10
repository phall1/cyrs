# Changelog — `cyrs-wasm`

All notable changes to `cyrs-wasm` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- cy-u6r (spec 0004 §4): initial landing.  Thin `wasm-bindgen` adapter
  that exposes the agent v1 op surface on a single `CypherDatabase` JS
  class: `parse`, `check`, `complete`, `hover`, `format`, `rewrite`,
  `plan`, `explain`, `schemaSet`, `schemaClear`, plus a static
  `protoVersion()` (wire proto pin per spec 0004 §4.3).

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
