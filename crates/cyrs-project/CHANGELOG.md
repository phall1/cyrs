# Changelog — `cyrs-project`

All notable changes to `cyrs-project` are documented here. The format is based
on [Keep a Changelog][kac], and this crate adheres to [Semantic
Versioning][semver]. See also the root [CHANGELOG.md](../../CHANGELOG.md)
for workspace-wide notes and coordinated releases (spec 0001 §18).

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

* cy-o8c.1 (spec 0003): initial crate. `ProjectManifest` + `ProjectFile`,
  `load_from_toml_str`, `load_from_toml_path`, `discover`, lint-level
  validation against a v0 placeholder registry, glob-based member
  expansion via `globset` + `walkdir`, round-trip tests.

### Changed

### Deprecated

### Removed

### Fixed

### Security
