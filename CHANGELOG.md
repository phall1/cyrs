# Changelog

All notable changes to the `cyrs` workspace are documented here.
The format is based on [Keep a Changelog][kac]; each published crate
adheres to [Semantic Versioning][semver].  See the
per-crate `CHANGELOG.md` under each `crates/<name>/` for crate-scoped
entries — this file only captures workspace-level events (toolchain
bumps, cross-cutting gates, coordinated releases).  See spec 0001 §18.

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

### Added

- `cy-bh5` (§11.6, §17.10): long-horizon `bench_incremental` with ±10 %
  steady-state RSS gate in both agent-single-FileId and LSP FileId-churn
  modes.
- `cy-gc4` (§15.2): `deferred` + `deferred_reason` fields on the agent
  `complete`/`hover`/`rewrite` responses so callers can distinguish
  "no matches" from "engine deferred to v2".
- `cy-urk` (§18): per-crate `CHANGELOG.md` skeletons and this
  workspace-level changelog.
- `cy-zgz` (§18): release-prep landing — CHANGELOG for
  `cypher-lang-services` (was missing), shields.io badges on every
  publishable crate README, `.github/workflows/release.yml`
  (release-plz, `workflow_dispatch`-only), `.github/workflows/
  sign-release.yml` (cosign keyless + CycloneDX SBOM + SLSA
  provenance, triggered on `release:published`), and
  `docs/release-playbook.md` for the operator cut / sign / publish
  / recovery flows.

### Changed

- `cypher-db` (§11.6, bead cy-bh5): `Database::remove_file` now pools
  freed Salsa input handles and recycles them in `open_file`.  Required
  because Salsa 0.26 cannot delete input structs; without pooling, LSP
  file churn grew RSS unboundedly.
- `scripts/check-noncoupling.sh`: added an `external_allow` regex that
  exempts third-party PascalCase identifiers (e.g. `rowan::WalkEvent`)
  from the §2.C2 compound check.  The gate was silently red on `main`
  before this fix.

### Deprecated

### Removed

### Fixed

### Security

<!--
Maintainer notes:

* Workspace-level entries should be cross-cutting (toolchain, CI gates,
  shared deps, spec-wide feature flags, coordinated release cadence).
* Crate-local work goes into the crate's own `crates/<name>/CHANGELOG.md`.
* Every line must cite a bead id and a spec section.
-->
