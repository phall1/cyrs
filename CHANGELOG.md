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

### Changed

### Deprecated

### Removed

### Fixed

### Security

## [0.1.0] — 2026-05-10

First public release. Nineteen crates published to crates.io
together. The workspace covers the full Cypher / GQL frontend stack:
lexer, recovering parser, lossless CST, typed AST, HIR with scope
graph, schema-aware semantic analysis, diagnostics, formatter, plan
IR, incremental analysis database (Salsa), language server, agent
JSON API, and CLI.

### Coverage at release

- openCypher v9: 93.2 % TCK acceptance on the openCypher v1 corpus.
- GQL ISO/IEC 39075:2024: 11.1 % on a 7-feature seed corpus
  (parser bootstrap; cy-0hj).
- Three downstream surfaces present: WASM (cyrs-wasm), C FFI
  (cyrs-ffi), Python/PyO3 (cyrs-py). All three are scaffolds at
  parity with cyrs-lang-services; non-trivial production use
  warrants further hardening.

### Added

- `cy-bh5` (§11.6, §17.10): long-horizon `bench_incremental` with
  ±10 % steady-state RSS gate in both agent-single-FileId and LSP
  FileId-churn modes.
- `cy-gc4` (§15.2): `deferred` + `deferred_reason` fields on the
  agent `complete` / `hover` / `rewrite` responses so callers can
  distinguish "no matches" from "engine deferred to v2".
- `cy-urk` (§18): per-crate `CHANGELOG.md` skeletons and this
  workspace-level changelog.
- `cy-zgz` (§18): CHANGELOG for `cyrs-lang-services` (previously
  missing), shields.io badges on every publishable crate README,
  `.github/workflows/release.yml` (release-plz, dispatch-only),
  `.github/workflows/sign-release.yml` (cosign keyless + CycloneDX
  SBOM + SLSA attestation), and `docs/release-playbook.md`.
- `cy-0hj` (§17.5, spec 0001 §C2.6): GQL ISO 39075 conformance
  bootstrap — TCK harness, 7-feature seed corpus, 11.1 % accept.
- `cy-9w5` (PR #41): VS Code extension scaffold under `editors/
  vscode/` (not yet on the marketplace; separate release track).
- `cy-bod` (PR #28): in-process LSP demo-round-trip latency budget,
  75 ms p95.
- `cy-2i9` (PR #25): `proto_version` field on every agent op so
  callers can negotiate forward.
- `cy-e3h` (PR #26): downstream canary crate (`tests/canary`) that
  guards `non_exhaustive` semver promises across publish.
- `cy-li6` (PR #23): publish precomputed `Parse` from
  `incremental_reparse` to Salsa, eliminating a re-parse per query.
- `cy-emb1` / `cy-emb2` / `cy-emb3` / `cy-emb4` / `cy-emb6` /
  `cy-emb7` / `cy-emb9`: embedder API ergonomics — single-call
  `parse_to_hir` / `lower_parse`, HIR `Visitor` + `walk_*`,
  typed `DiagCode`, `SchemaProvider` example, curated TCK subset,
  write-side coverage matrix, integration-depth doc.
- `cy-4mg` (PR #10): `CALL <proc> YIELD` parser end-to-end.
- `cy-li5` (PR #7): `incremental_reparse` smart-path sub-tree
  splice.
- `cy-b5b` (PR #8): `shortestPath` / `allShortestPaths` (4 of 5
  layers + tests).
- `cy-01q` (PR #11): map projection (parser + AST layers).
- `cy-h9n` (PR #42): unified crate naming on `cyrs-*` (directory +
  package + lib triple). Three binaries kept their `cypher-*`
  names (`cypher`, `cypher-lsp`, `cypher-agent`) for user-config
  stability.
- `cy-3ij` (PR #39): honest README framing on openCypher / GQL
  coverage.
- `cy-ewo` (PR #40): `TextRangeExt::as_byte_range` deduplication.
- `cy-y1k`: 0.1.0 release prep — workspace version bump, dep-order
  publish playbook, dry-run validation across all 19 publishable
  crates.

### Changed

- `cyrs-db` (§11.6, bead cy-bh5): `Database::remove_file` now pools
  freed Salsa input handles and recycles them in `open_file`.
  Required because Salsa 0.26 cannot delete input structs; without
  pooling, LSP file churn grew RSS unboundedly.
- `scripts/check-noncoupling.sh`: added an `external_allow` regex
  that exempts third-party PascalCase identifiers (e.g.
  `rowan::WalkEvent`) from the §2.C2 compound check.

### Fixed

- `cy-eu2` (PR #37): formatter idempotency around the
  `// cypher-fmt: off` directive.
- `cy-863` (PR #38): plan precheck recurses into `PatternPredicate`'s
  embedded pattern, fixing a fuzz_plan unresolved-variable panic.
- LSP (PR #30): produce RFC 8089 file URIs so Windows tests stop
  crashing the server.
- CI (PR #29, PR #31): unblock fuzz-smoke, cargo-semver-checks, and
  rescue miri by gating `cyrs-diag` rowan-touching test/doctest.

### Known limitations

- GQL ISO 39075 acceptance is 11.1 % on a 7-feature seed (bootstrap;
  full coverage tracked).
- No Neo4j-current dialect; workspace targets openCypher v9 + GQL
  ISO only.
- `cy-208`: a third distinct rowan UB pattern (`cursor::free`
  stacked-borrows) is filed but unresolved upstream. Workspace
  pins `rowan = "0.15"` from crates.io; cy-934 / cy-yrz track the
  upstream rewrite.
- VS Code extension is a scaffold (cy-9w5 landed); not yet
  published to the marketplace.
- Lints: `pedantic` is on; `restriction` is not.

<!--
Maintainer notes:

* Workspace-level entries should be cross-cutting (toolchain, CI gates,
  shared deps, spec-wide feature flags, coordinated release cadence).
* Crate-local work goes into the crate's own `crates/<name>/CHANGELOG.md`.
* Every line must cite a bead id and a spec section.
-->
