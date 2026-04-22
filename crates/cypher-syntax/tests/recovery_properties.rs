//! Parser recovery property tests (spec 0001 §17.3).
//!
//! Properties implemented here (cy-gkh.1):
//!
//! - **No panic** — `parse(prefix)` never panics (belt-and-braces on top of
//!   the fuzz harness).
//! - **Single root** — the resulting syntax node is always a `SOURCE_FILE`.
//! - **Bounded diagnostics** — `parse(prefix).errors().len()` is bounded
//!   (see `MAX_ERRORS` below) even for truncated inputs.
//! - **Locality** — when there *are* errors, the first reported diagnostic's
//!   byte offset is within the last `LOCALITY_WINDOW` bytes of the prefix
//!   (i.e. errors are reported near the point of truncation, not drifting
//!   back into already-parsed text).
//!
//! Strategy: hard-code a curated ~15 diverse TCK v1 sources (MATCH, RETURN,
//! WITH, UNWIND, CREATE, list/map literals, variable-length paths, etc.).
//! For each, proptest generates a random byte-prefix length and we parse
//! the prefix (truncated to the nearest char boundary). The prefix is
//! exactly the kind of input an LSP or REPL sees while the user is still
//! typing — bounded diagnostics + locality are what make incremental UX
//! survive mid-token edits.

use cypher_syntax::{SyntaxKind, parse};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Tuning knobs
// ---------------------------------------------------------------------------
//
// Both bounds were set empirically by running the proptest suite against
// the curated TCK subset below. If a future grammar change pushes a common
// prefix above these limits, tune here (and document why) rather than in
// the parser — this test is intentionally an observability gate, not a
// correctness contract.

/// Upper bound on `parse(prefix).errors().len()`. Truncated-prefix parses
/// in the curated corpus produce at most a handful of recovery events; we
/// budget a little headroom.
const MAX_ERRORS: usize = 6;

/// Window (in bytes, counted back from the end of the prefix) in which the
/// *first* reported diagnostic is allowed to start. Chosen empirically:
/// recovery typically fires at the first unexpected token, which lands
/// within one keyword of truncation. 48 bytes covers even the longest
/// keyword (`OPTIONAL MATCH`) plus a preceding identifier tail.
const LOCALITY_WINDOW: u32 = 48;

// ---------------------------------------------------------------------------
// Curated TCK v1 source subset
// ---------------------------------------------------------------------------
//
// Hand-picked from `crates/cypher-tck/tck/v1.toml` to cover MATCH, RETURN,
// WITH, UNWIND, CREATE, list/map literals, variable-length paths, and
// clause combinators. Degenerate sources (empty, comment-only, single
// keyword) are excluded per the bead brief.

const SOURCES: &[&str] = &[
    // MATCH family
    "MATCH (n) RETURN n",
    "MATCH (n:Person) RETURN n",
    "MATCH (n {name: 'Alice'}) RETURN n",
    "MATCH (a)-[:KNOWS]->(b) RETURN a, b",
    "OPTIONAL MATCH (n:Person) RETURN n",
    // WHERE / predicates
    "MATCH (n:Person) WHERE n.name = 'Alice' RETURN n",
    "MATCH (n) WHERE n.age > 21 AND n.active = true RETURN n",
    // WITH pipelining
    "MATCH (n:Person) WITH n.name AS name RETURN name",
    "MATCH (n:Person) WITH n ORDER BY n.name LIMIT 5 RETURN n",
    // RETURN shapes
    "MATCH (n:Person) RETURN n ORDER BY n.name SKIP 0 LIMIT 10",
    "RETURN 1 AS a, 2 AS b",
    // Literals
    "RETURN [1, 2, 3]",
    "RETURN {a: 1, b: 'two'}",
    // UNWIND
    "UNWIND [1, 2, 3] AS x RETURN x",
    // Writes
    "CREATE (n:Person {name: 'Alice'})",
    "MATCH (n:Person) SET n.active = true",
    // Variable-length path
    "MATCH (a)-[:KNOWS*1..3]->(b) RETURN a, b",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Truncate `src` to the largest char boundary `<= len`.
fn truncate_to_char_boundary(src: &str, len: usize) -> &str {
    let cap = len.min(src.len());
    let mut boundary = cap;
    while boundary > 0 && !src.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &src[..boundary]
}

// ---------------------------------------------------------------------------
// Proptest
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    /// Parsing any byte-prefix of a curated TCK v1 source:
    /// (1) does not panic, (2) yields a SOURCE_FILE root, (3) reports at
    /// most `MAX_ERRORS` diagnostics, (4) when non-empty, the first
    /// diagnostic's offset is within the last `LOCALITY_WINDOW` bytes of
    /// the prefix.
    #[test]
    fn prefix_parse_invariants(
        src_idx in 0..SOURCES.len(),
        len in 0usize..=1024,
    ) {
        let src = SOURCES[src_idx];
        let prefix = truncate_to_char_boundary(src, len);

        // (1) + (2): parse + single SOURCE_FILE root.
        let parse_result = parse(prefix);
        prop_assert_eq!(
            parse_result.syntax().kind(),
            SyntaxKind::SOURCE_FILE,
            "root kind for prefix {:?} was not SOURCE_FILE",
            prefix,
        );

        // Losslessness is not the subject of this bead, but it's a cheap
        // cross-check and catches any builder-side truncation bug.
        prop_assert_eq!(
            parse_result.syntax().to_string(),
            prefix,
            "lossless round-trip failed for prefix {:?}",
            prefix,
        );

        // (3) Bounded diagnostics.
        let errors = parse_result.errors();
        prop_assert!(
            errors.len() <= MAX_ERRORS,
            "got {} errors (> {}) for prefix {:?}: {:?}",
            errors.len(),
            MAX_ERRORS,
            prefix,
            errors,
        );

        // (4) Locality: first diagnostic near the end-of-prefix cursor.
        if let Some(first) = errors.first() {
            let start: u32 = first.offset.into();
            let prefix_len = u32::try_from(prefix.len())
                .expect("prefix length bounded by strategy to <= 1024");
            let lower = prefix_len.saturating_sub(LOCALITY_WINDOW);
            prop_assert!(
                start + LOCALITY_WINDOW >= prefix_len || start >= lower,
                "first diag at offset {} is more than {} bytes before \
                 end-of-prefix {} for prefix {:?} (full errors: {:?})",
                start,
                LOCALITY_WINDOW,
                prefix_len,
                prefix,
                errors,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Smoke (non-proptest) — a hand-constructed pathological prefix.
// ---------------------------------------------------------------------------

/// `MATCH (n WHERE n.age` — a classic mid-typing prefix where the user
/// has forgotten to close the node pattern before adding a predicate.
/// Parser must recover, stay under the error budget, and keep the first
/// diagnostic near the end of the prefix.
#[test]
fn smoke_partial_match_where() {
    let src = "MATCH (n WHERE n.age";
    let p = parse(src);

    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert_eq!(p.syntax().to_string(), src);

    let errs = p.errors();
    assert!(
        errs.len() <= MAX_ERRORS,
        "smoke: got {} errors (> {}): {:?}",
        errs.len(),
        MAX_ERRORS,
        errs,
    );

    if let Some(first) = errs.first() {
        let start: u32 = first.offset.into();
        let prefix_len = u32::try_from(src.len()).expect("fixture src fits in u32");
        let lower = prefix_len.saturating_sub(LOCALITY_WINDOW);
        assert!(
            start + LOCALITY_WINDOW >= prefix_len || start >= lower,
            "smoke: first diag at offset {start} is more than \
             {LOCALITY_WINDOW} bytes before end-of-prefix {prefix_len} \
             (full errors: {errs:?})",
        );
    }
}

/// Regression: empty prefix must parse to a `SOURCE_FILE` with no errors
/// (not merely "recoverable" — literally clean).
#[test]
fn smoke_empty_prefix() {
    let p = parse("");
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(
        p.errors().is_empty(),
        "empty input should produce zero errors, got {:?}",
        p.errors(),
    );
}

/// Regression: a full source from the corpus must parse clean (outcome=ok
/// in v1.toml). This pins the contract that our curated subset is all
/// green-tag and any future breakage is a real regression, not a new
/// scenario slipping in.
#[test]
fn smoke_full_sources_parse_clean() {
    for src in SOURCES {
        let p = parse(src);
        assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
        assert!(
            p.errors().is_empty(),
            "full source {src:?} unexpectedly produced errors: {:?}",
            p.errors(),
        );
    }
}
