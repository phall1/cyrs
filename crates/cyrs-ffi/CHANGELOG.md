# Changelog — `cyrs-ffi`

All notable changes to `cyrs-ffi` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

The C ABI itself is covered by a stronger stability promise (spec 0004
§9.2): once a symbol ships in a tagged release, its signature and
semantics are frozen for the life of the major version.  New symbols may
be added in minor releases; removal or signature change requires a major
bump and an entry here.

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- cy-dh6 (spec 0004 §5): initial landing.  Thin C-ABI adapter that
  exposes the Cyrs frontend as a `cdylib` + `staticlib` + cbindgen-
  generated header.  Opaque handles (`CypherDatabase`,
  `CypherDiagnosticList`, `CypherParseResult`, `CypherHoverResult`,
  `CypherCompletionList`, `CypherRewriteResult`) plus paired `_free`
  functions; accessors return borrowed pointers.  Every export wraps
  its body in `catch_unwind` and stashes the panic payload into a
  thread-local readable via `cypher_last_error`.  `cypher_proto_version`
  returns the wire pin shared with the agent and wasm surfaces.

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
* C-ABI stability: any symbol removal or signature change is a MAJOR
  bump.  Adding new symbols is a MINOR.  See spec 0004 §9.2.
-->
