# Changelog — `cypher-py`

All notable changes to `cypher-py` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- cy-mpb (spec 0004 §6): initial landing.  Thin PyO3 adapter that
  exposes the agent v1 op surface on a single `CypherDatabase`
  Python class: `parse`, `check`, `complete`, `hover`, `format`,
  `rewrite`, `schema_set`, `schema_clear`, plus a module-level
  `PROTO_VERSION` constant (wire proto pin per spec 0004 §9.3).
  Diagnostics surface as a frozen `#[pyclass]` with `.code`,
  `.severity`, `.message`, `.range` getters.  Packaged as an
  abi3-py310 wheel via maturin; one wheel per `{os, arch}` covers
  CPython 3.10–3.13 (spec 0004 §6.2).  `cypher.pyi` type stub
  shipped alongside the wheel (spec 0004 §6.3).

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
