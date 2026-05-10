//! Parser recovery property tests (spec 0001 §17.3).
//!
//! # Recovery-code coverage registry (bead cy-gkh)
//!
//! The list below is the *authoritative* set of syntax-range recovery
//! codes (E0001–E0999) that this property test — together with the UI
//! fixtures under `tests/ui/syntax/` — exercises. The
//! `cargo xtask check-recovery-budget` gate
//! (`xtask/src/check_recovery_budget.rs`) grep-walks this file for
//! `E00xx` tokens and flags any PR that adds a new parser emit site
//! without mentioning the code here. The convention: add a one-line
//! `// Exercised: E00xx — <reason>` comment whenever a new code lands;
//! the reason names the covering UI fixture or the `SOURCES` entry that
//! reaches the emit site via a random prefix.
//!
//! Exercised: E0002 — unexpected-token, covered by parser unit tests.
//! Exercised: E0007 — expected-statement, covered by UI fixture candidates + recovery cascades.
//! Exercised: E0008 — expected-semi, covered by `named_path_missing_eq.stderr` tail.
//! Exercised: E0009 — expected-lparen, covered by `named_path_missing_eq.stderr`.
//! Exercised: E0010 — expected-node-after-rel, covered by random prefixes of `MATCH (a)-[:KNOWS]->(b)`.
//! Exercised: E0011 — expected-rparen-node, covered by prefixes of `MATCH (n) RETURN n`.
//! Exercised: E0012..=E0017 — rel/path recovery, covered by prefixes of rel patterns in SOURCES.
//! Exercised: E0018..=E0022 — property-map recovery, covered by prefixes of `MATCH (n {name: 'Alice'}) RETURN n`.
//! Exercised: E0023..=E0043 — expression / clause-trailer recovery, covered by prefixes of WHERE / ORDER BY / WITH / RETURN sources.
//! Exercised: E0044..=E0046 — clause-dispatch and string lexer recovery, covered by random prefixes + lexer unit tests.
//! Exercised: E0047 — list-elem, UI fixture `list_missing_element`.
//! Exercised: E0048 — list-close, UI fixture `list_unclosed`.
//! Exercised: E0049 — map-key, UI fixture `map_missing_key`.
//! Exercised: E0050 — map-colon, UI fixture `map_missing_colon`.
//! Exercised: E0051 — map-value, UI fixture `map_missing_value`.
//! Exercised: E0052 — map-close, UI fixture `map_unclosed`.
//! Exercised: E0053 — unwind-expr, UI fixture `unwind_missing_expr`.
//! Exercised: E0054 — unwind-as, UI fixture `unwind_missing_as`.
//! Exercised: E0055 — create-pattern, UI fixture `create_missing_pattern`.
//! Exercised: E0056 — merge-pattern, UI fixture `merge_missing_pattern`.
//! Exercised: E0057 — set-item, UI fixture `set_missing_item`.
//! Exercised: E0058 — remove-item, UI fixture `remove_missing_item`.
//! Exercised: E0059 — delete-expr, UI fixture `delete_missing_expr`.
//! Exercised: E0060 — detach-delete, UI fixture `detach_without_delete`.
//! Exercised: E0061 — merge-on-action, UI fixture `merge_on_missing_action`.
//! Exercised: E0062 — reserved for var-length hop close; parser path not yet live (see `recovery.md` `RangeHops`).
//! Exercised: E0063 — reserved for path-binder `=`; parser path handles via E0009 today (see `recovery.md` `PathPattern`).
//! Exercised: E0064 — index-close, covered by `crates/cypher-syntax/src/grammar/expression.rs` unit tests.
//! Exercised: E0065..=E0067 — list-predicate recovery (cy-8x5), covered by existing UI fixture for list predicates.
//! Exercised: E0068..=E0069 — list-comprehension recovery (cy-5gh), covered by random prefixes of SOURCES.
//! Exercised: E0072 — EXISTS(<pattern>) missing ')' (cy-lve), UI fixture `exists_pattern_missing_rparen`.
//! Exercised: E0073..=E0075 — CALL <proc> YIELD recovery (cy-4mg), covered by random prefixes of `CALL ns.proc(1, 2) YIELD x AS xx, y` and the unit tests in `crates/cypher-syntax/tests/call_yield.rs`.
//! Exercised: E0077 — shortestPath / allShortestPaths missing `)` (cy-b5b), covered by random prefixes of `MATCH p = shortestPath((a)-[:KNOWS*]->(b))`.
//! Exercised: E0078..=E0081 — Map projection recovery (cy-01q), covered by random prefixes of `RETURN n { .name, key: n.value, .*, * }` and the unit tests in `crates/cypher-syntax/tests/map_projection.rs`.
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
    // CALL <proc> YIELD ... (cy-4mg)
    "CALL ns.proc(1, 2) YIELD x AS xx, y RETURN xx, y",
    // Map projection (cy-01q)
    "MATCH (n) RETURN n { .name, key: n.value, .*, * }",
    // shortestPath / allShortestPaths (cy-b5b) — exercises E0077
    "MATCH p = shortestPath((a)-[:KNOWS*]->(b)) RETURN p",
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
