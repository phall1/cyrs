//! `cypher-tck` — openCypher TCK conformance harness. Spec 0001 §17.5.
//!
//! The TCK is a feature-tagged suite of scenarios. This crate provides
//! the scaffolding to fetch, parse, and execute those scenarios against
//! the front-end.
//!
//! Three corpora feed into the harness:
//!
//! 1. **`tck/v1.toml`** — a hand-written, representative slice of the
//!    full openCypher TCK.  Every scenario is classified per-bead and
//!    is expected to stay green on every PR.  The `cargo test -p
//!    cypher-tck` pre-commit gate runs this slice.
//!
//! 2. **`tck/embedder-m23.toml`** — a curated subset of M23-fundamental
//!    scenarios, gated alongside `v1.toml` for embedders that pin the
//!    openCypher M23 corpus on every PR (bead cy-emb6, embedder-issue
//!    0006).  Scenarios may only be added when they pass; regressions
//!    are parser bugs, never silenced.
//!
//! 3. **`tck/full/`** — the upstream openCypher TCK corpus, vendored
//!    at tag `2024.3` (see `tck/full/VENDORED.md`).  The harness
//!    loads this corpus only under the `full-tck` Cargo feature.
//!    Every freshly-ingested scenario defaults to
//!    [`Expected::Ignored`] until a human triages it — this keeps the
//!    full-corpus pass-rate a baseline metric rather than a CI gate
//!    (spec §17.5, bead cy-p5q).
//!
//! ## Per-scenario outcome
//!
//! The historical "green / red" *feature-gate* label is retired.
//! Conformance is now declared per-scenario via [`Expected`]:
//!
//! - [`Expected::Supported`] — the parser must accept the query.
//! - [`Expected::Error`]     — the parser must reject the query.
//! - [`Expected::Ignored`]   — the scenario is skipped.
//!
//! This matches how the TCK actually describes itself: individual
//! scenarios under the same feature tag can exercise either positive
//! or negative behaviour.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-tck/0.0.1")]

/// Per-scenario expected outcome (spec §17.5, bead cy-p5q).
///
/// Replaces the old per-tag `Expected::Green | Red` enum.  Each
/// scenario in the TCK fixtures carries its own expectation so that
/// negative tests under a "supported" feature tag are representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Expected {
    /// The parser must accept the query with no syntax errors.
    Supported,
    /// The parser must reject the query with at least one syntax error.
    Error,
    /// The scenario is acknowledged but not executed.  Used for:
    ///
    /// - known parser gaps on the v1 slice (paired with a follow-up bead);
    /// - every scenario freshly vendored from the upstream TCK (default
    ///   until a human triages it).
    Ignored,
}

/// The v1 openCypher feature-tag whitelist (spec §17.5).
///
/// The v1 slice of scenarios is filtered down to only those carrying
/// at least one of these tags.  Scenarios outside this tag set are
/// skipped in the v1 pre-commit harness (they may still be covered by
/// the `--features full-tck` run).
///
/// Tags are kept as `&'static str` so new tags can be introduced by a
/// fixture edit without a crate version bump.
#[must_use]
pub fn v1_tags() -> &'static [&'static str] {
    &[
        "@MATCH",
        "@OPTIONAL-MATCH",
        "@WHERE",
        "@RETURN",
        "@WITH",
        "@UNWIND",
        "@CREATE",
        "@MERGE",
        "@SET",
        "@REMOVE",
        "@DELETE",
        "@EXPRESSIONS",
        "@AGGREGATIONS",
        "@STRINGS",
        "@LISTS",
        "@MAPS",
        "@PATTERNS",
        "@NULL",
        // v1 "red" tags — scenarios carrying these must be rejected.
        // Represented alongside the accept-tags; per-scenario
        // `Expected::Error` selects the outcome.
        "@CALL-SUBQUERY",
        "@EXISTS-SUBQUERY",
        "@LOAD-CSV",
    ]
}

/// The embedder M23 tag used by `tck/embedder-m23.toml` (bead cy-emb6).
///
/// The embedder-M23 fixture marks every scenario with `@M23` so the
/// harness can filter the file independently from the v1 slice.  The
/// tag set is otherwise identical: scenarios additionally carry their
/// area tags (`@MATCH`, `@CREATE`, ...) for human readability.
#[must_use]
pub fn embedder_m23_tag() -> &'static str {
    "@M23"
}
