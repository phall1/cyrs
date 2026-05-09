# 0006 — TCK `Expected` classification needs an embedder M23 subset

**Severity:** medium
**Discovered:** comparing the legacy parser's TCK gating to cyrs's

## Context

- The embedder's legacy parser gates the **full ~220-feature openCypher
  M23 corpus** on every PR. Every scenario passes, or CI fails.
- cyrs gates only the hand-written `tck/v1.toml` slice on PRs. The full
  vendored corpus (`tck/full/`, 2024.3, 1339 scenarios) is
  measurement-only — currently ~80% pass.

For the embedder to migrate off its legacy parser without losing that
pin, cyrs needs either:

1. **An embedder M23 subset** — a curated TOML file that names the M23
   scenarios the embedder depends on, gated like `v1.toml`, OR
2. **Full-corpus gating** at >= the pass-rate the legacy parser
   currently achieves.

Option 1 is a smaller ask and lets the embedder migrate without
waiting on cyrs to close out features beyond M23.

## Proposed shape

Add `cyrs/crates/cypher-tck/tck/embedder-m23.toml` that names every
scenario the legacy parser currently passes. CI gates this slice in
addition to `v1.toml`. When cyrs's full-corpus pass rate hits parity
with M23, the slice can be retired.

A more elegant approach: tag scenarios with `spec_version = "M23"` in
the existing TOML and let CI gate by tag. Either works.

## Why it matters

Without this, switching the embedder's parser is a regression — we'd
lose 220 vendored scenarios of behavioral pinning. Not acceptable per
the embedder's TCK practice.
