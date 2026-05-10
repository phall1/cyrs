# Changelog — `cyrs-lsp`

All notable changes to `cyrs-lsp` are documented here.  The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver].  See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

* cy-m0d (spec 0004 §7): `web-lsp` Cargo feature exposes a
  `DedicatedWorkerGlobalScope` + `postMessage` transport for the
  LSP-Web demo worker.  `cyrs_lsp::transport::Transport` trait
  abstracts the wire plumbing; `StdioTransport` wraps the existing
  `lsp_server::Connection` so the native path is byte-identical.
  `cyrs_lsp::web::start_lsp` is the `#[wasm_bindgen]` entry point the
  demo's worker invokes.

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
