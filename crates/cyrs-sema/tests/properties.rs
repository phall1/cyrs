//! Sema property tests (spec 0001 §17.3).
//!
//! Property implemented here:
//!
//! - **P17.3.5 Diagnostic stability under trivia** — inserting or removing
//!   whitespace and comments in a source does NOT change the set of
//!   diagnostic *codes* emitted. Span positions may shift; codes are stable
//!   (spec §17.3 P17.3.5: "permuting whitespace and comments in a program
//!   does not change the set of non-trivia-sensitive diagnostics produced").
//!
//! Strategy: for a known-valid (or semi-valid) Cypher source, generate a
//! "trivia-injected" variant by substituting each run of whitespace with a
//! different legal trivia string (spaces, tabs, newlines, comments), then
//! assert that the two sets of diagnostic codes are identical.

use cyrs_diag::{DiagCode, DiagnosticsSink};
use cyrs_hir::desugar::desugar_statement;
use cyrs_sema::{SemaOptions, analyse};
use proptest::prelude::*;

/// Lower `src` → HIR best-effort. `cyrs_hir::lower::lower_statement` is
/// fallible since cy-cfi; this property feeds it generated input that may
/// not parse cleanly, so `lower_parse` (infallible) is required.
fn lower_statement(src: &str) -> cyrs_hir::Statement {
    cyrs_hir::lower::lower_parse(&cyrs_syntax::parse(src)).expect("lower_parse is infallible")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run the full sema pipeline on `src` (no schema, default options) and
/// return the sorted, deduplicated set of diagnostic codes.
fn diag_codes(src: &str) -> Vec<DiagCode> {
    let stmt = lower_statement(src);
    let stmt = desugar_statement(stmt);
    let mut sink = DiagnosticsSink::new();
    let opts = SemaOptions::default();
    analyse(&stmt, None, &opts, &mut sink);
    let mut codes: Vec<DiagCode> = sink.into_sorted().into_iter().map(|d| d.code).collect();
    codes.sort();
    codes.dedup();
    codes
}

/// Replace every run of ASCII whitespace in `src` with a trivia string
/// chosen by `picker(i, n_options)` → index.  The significant (non-trivia)
/// tokens are left untouched, so the semantic content is identical.
///
/// Trivia options deliberately span single space, multiple spaces, tabs,
/// newlines, line comments, and block comments so that the property
/// exercises all forms of legal Cypher trivia.
fn inject_trivia(src: &str, mut picker: impl FnMut(usize, usize) -> usize) -> String {
    // Trivia sequences to substitute for any existing whitespace boundary.
    // Each option is syntactically invisible to the semantic analysis passes.
    const TRIVIA: &[&str] = &[
        " ",
        "  ",
        "   ",
        "\t",
        "\n",
        "\n\n",
        " \n ",
        "\t\t",
        " // line comment\n",
        "\n// another comment\n",
        " /* block */ ",
        "\n/* block\n   multiline */\n",
        " /* c1 */ /* c2 */ ",
    ];

    let mut out = String::with_capacity(src.len() + 32);
    let mut in_ws = false;
    let mut buf = String::new();
    let mut call_idx = 0usize;

    for ch in src.chars() {
        if ch.is_ascii_whitespace() {
            in_ws = true;
        } else {
            if in_ws {
                // Substitute the collected whitespace run with a chosen trivia.
                let choice = picker(call_idx, TRIVIA.len());
                call_idx += 1;
                out.push_str(TRIVIA[choice]);
                buf.clear();
                in_ws = false;
            }
            out.push(ch);
        }
    }
    // Trailing whitespace — substitute or drop.
    if in_ws {
        let choice = picker(call_idx, TRIVIA.len());
        out.push_str(TRIVIA[choice]);
    }

    out
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Known-valid and semi-valid Cypher sources for testing diag stability.
fn cypher_sources() -> impl Strategy<Value = String> {
    prop_oneof![
        // Valid — should produce zero diagnostics.
        Just("MATCH (n) RETURN n".to_string()),
        Just("MATCH (n:Person) RETURN n".to_string()),
        Just("MATCH (n)-[r:KNOWS]->(m) RETURN n".to_string()),
        Just("MATCH (n) WHERE n.age > 21 RETURN n".to_string()),
        Just("MATCH (n) RETURN n.name, n.age".to_string()),
        Just("MATCH (n) RETURN DISTINCT n".to_string()),
        Just("MATCH (n) RETURN n ORDER BY n.name ASC".to_string()),
        Just("MATCH (n) RETURN n SKIP 10 LIMIT 5".to_string()),
        Just("UNWIND [1,2,3] AS x RETURN x".to_string()),
        Just("MATCH (n) WITH n RETURN n".to_string()),
        // Semi-valid — will produce E1001 for the unresolved ref.
        Just("MATCH (n) RETURN m".to_string()),
        Just("MATCH (n) WHERE n.age > x RETURN n".to_string()),
    ]
}

// ---------------------------------------------------------------------------
// P17.3.5 — Diagnostic stability under trivia
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    /// For each source, inject random trivia and assert that the set of
    /// diagnostic codes is unchanged (spec P17.3.5).
    #[test]
    fn diag_stability_under_trivia(
        src in cypher_sources(),
        seed in 0u64..u64::MAX,
    ) {
        // Simple LCG for reproducible pseudo-random choices.
        let mut state = seed;
        let mut picker = |_i: usize, n: usize| -> usize {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) as usize) % n.max(1)
        };

        let original_codes = diag_codes(&src);
        let injected_src = inject_trivia(&src, &mut picker);
        let injected_codes = diag_codes(&injected_src);

        prop_assert_eq!(
            &original_codes,
            &injected_codes,
            "diag stability violated:\n  original src:  {:?}\n  injected src:  {:?}\n  original codes: {:?}\n  injected codes: {:?}",
            src,
            injected_src,
            original_codes,
            injected_codes,
        );
    }
}

// ---------------------------------------------------------------------------
// Regression guards
// ---------------------------------------------------------------------------

/// Adding a comment before a valid query must not alter diag codes.
#[test]
fn regression_comment_prefix() {
    let base = "MATCH (n) RETURN n";
    let with_comment = "// find all nodes\nMATCH (n) RETURN n";
    assert_eq!(
        diag_codes(base),
        diag_codes(with_comment),
        "diag codes changed when adding a comment prefix"
    );
}

/// Adding trailing whitespace must not alter diag codes.
#[test]
fn regression_trailing_whitespace() {
    let base = "MATCH (n) RETURN n";
    let with_ws = "MATCH (n) RETURN n   \n";
    assert_eq!(
        diag_codes(base),
        diag_codes(with_ws),
        "diag codes changed when adding trailing whitespace"
    );
}

/// Inline block comment must not alter diag codes for a valid query.
#[test]
fn regression_inline_block_comment_valid() {
    let base = "MATCH (n) RETURN n";
    let with_comment = "MATCH (n) /* hello */ RETURN n";
    assert_eq!(
        diag_codes(base),
        diag_codes(with_comment),
        "diag codes changed when inserting inline block comment (valid input)"
    );
}

/// Inline block comment must not alter diag codes for a query with errors.
#[test]
fn regression_inline_block_comment_with_error() {
    let base = "MATCH (n) RETURN m";
    let with_comment = "MATCH (n) /* hello */ RETURN m";
    assert_eq!(
        diag_codes(base),
        diag_codes(with_comment),
        "diag codes changed when inserting inline block comment (erroneous input)"
    );
}

/// Extra spaces between tokens must not alter diag codes.
#[test]
fn regression_extra_spaces() {
    let base = "MATCH (n) RETURN n";
    let with_spaces = "MATCH   (n)   RETURN   n";
    assert_eq!(
        diag_codes(base),
        diag_codes(with_spaces),
        "diag codes changed when adding extra spaces"
    );
}
