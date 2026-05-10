# Changelog — `cyrs-lang-services`

All notable changes to `cyrs-lang-services` are documented here.  The format
is based on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- `cy-4t0` (§14, §15): initial crate — lifts shared completion, hover, and
  rewrite engines out of `cyrs-lsp` and `cyrs-agent` so both binaries
  consume them as pure functions keyed on `(db, file_id, byte_offset)`.
- `cy-2i9.1` (§18): `#[non_exhaustive]` on `CompletionItem`, `Hover`,
  `RewriteEdit`, `RewritePayload`, and `CompletionItemKind` so future
  fields / variants can land without a SemVer-major bump.
- `cy-gc4` (§15.2): `deferred` + `deferred_reason` plumbed through the
  agent adapter's `complete`/`hover`/`rewrite` responses so callers can
  distinguish "no matches" from "engine deferred to v2".
- `cy-zgz` (§18): release-ready crate metadata (description, categories,
  keywords, docs.rs config) and this changelog.

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
