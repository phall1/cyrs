# Changelog — `cypher-cli`

All notable changes to `cypher-cli` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- cy-atk (§16): `cypher check` now runs the full analysis pipeline via
  `Database::all_diagnostics` and renders each diagnostic in rustc-style
  with source underline (via `cypher-diag::render_text_stderr`). Exit
  code `1` on any error-severity diagnostic; `0` on a clean query.

### Changed

- cy-atk (§16): switched `cypher-cli` from `LegacyDatabase` to the
  workspace `Database` API — same incremental backend the LSP uses.
  `parse` and `fmt` behaviour is unchanged for well-formed input.

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
