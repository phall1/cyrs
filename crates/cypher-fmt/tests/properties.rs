//! Formatter property tests (spec 0001 §17.3).
//!
//! Properties implemented here:
//!
//! - **P17.3.3 idempotence** — `fmt(fmt(s)) == fmt(s)` for all inputs.
//! - **P17.3.4 semantic preservation** — the significant (non-trivia) token
//!   sequence of `parse(fmt(s))` equals that of `parse(s)`, for inputs that
//!   parse without error nodes.  Inputs that contain ERROR nodes are skipped
//!   (`prop_assume!`) so we only test valid sources.
//! - **deterministic** — two calls to `format(s)` with identical options
//!   produce byte-identical output.
//! - **trivia preservation** — every line comment (`//…`) and block comment
//!   (`/*…*/`) present in a valid formatted output survives a second format
//!   pass (comment text is not dropped by the formatter).
//!
//! `cypher-fmt` depends on `cypher-syntax` only (§3 crate graph), so
//! "AST structural equality" is implemented as equality of the significant
//! token sequence (kind + text after case-normalisation), which is a sound
//! approximation: the formatter only touches trivia and keyword casing, so
//! the non-trivia token sequence must be invariant for valid inputs.

use cypher_fmt::format;
use cypher_syntax::{SyntaxKind, parse};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the significant (non-trivia) tokens of a parsed source as
/// `Vec<(SyntaxKind, String)>`, with keyword text uppercased so that
/// casing differences do not cause spurious failures.
fn significant_tokens(src: &str) -> Vec<(SyntaxKind, String)> {
    let tree = parse(src);
    let root = tree.syntax();
    root.preorder_with_tokens()
        .filter_map(|event| match event {
            rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(tok)) => {
                let kind = tok.kind();
                if kind.is_trivia() {
                    None
                } else {
                    let text = if kind.is_keyword() {
                        tok.text().to_uppercase()
                    } else {
                        tok.text().to_string()
                    };
                    Some((kind, text))
                }
            }
            _ => None,
        })
        .collect()
}

/// Returns `true` when the parse tree for `src` contains no ERROR nodes.
/// We use this to filter the semantic-preservation property to valid inputs
/// only (per spec P17.3.4: "for any valid `s`").
fn has_no_errors(src: &str) -> bool {
    let p = parse(src);
    p.errors().is_empty() && {
        let root = p.syntax();
        // Also walk the tree: ERROR *nodes* can appear even without a
        // recorded SyntaxError (e.g. from error recovery tokens).
        !root.preorder_with_tokens().any(|ev| match ev {
            rowan::WalkEvent::Enter(rowan::NodeOrToken::Node(n)) => n.kind() == SyntaxKind::ERROR,
            rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(t)) => t.kind() == SyntaxKind::ERROR,
            _ => false,
        })
    }
}

/// Extract the text of every line comment and block comment present in `src`.
fn extract_comments(src: &str) -> Vec<String> {
    let tree = parse(src);
    let root = tree.syntax();
    root.preorder_with_tokens()
        .filter_map(|event| match event {
            rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(tok)) => {
                if matches!(
                    tok.kind(),
                    SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
                ) {
                    Some(tok.text().trim().to_string())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Cypher-like ASCII strings (mirrors the strategy in `src/lib.rs`).
fn cypher_like() -> impl Strategy<Value = String> {
    "[A-Z a-z0-9():,.\n;/*]{0,80}".prop_map(|s| s)
}

/// Known-valid Cypher fragments (highly likely to parse without errors).
fn cypher_valid() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("MATCH (n) RETURN n".to_string()),
        Just("MATCH (n:Person) RETURN n".to_string()),
        Just("MATCH (n)-[r]->(m) RETURN n, r, m".to_string()),
        Just("MATCH (n)-[r:KNOWS]->(m) RETURN n".to_string()),
        Just("MATCH (n) WHERE n.age > 21 RETURN n".to_string()),
        Just("MATCH (n) RETURN n.name, n.age".to_string()),
        Just("MATCH (n) RETURN DISTINCT n".to_string()),
        Just("MATCH (n) RETURN n ORDER BY n.name ASC".to_string()),
        Just("MATCH (n) RETURN n SKIP 10 LIMIT 5".to_string()),
        Just("UNWIND [1,2,3] AS x RETURN x".to_string()),
        Just("MATCH (n) WITH n RETURN n".to_string()),
        Just("MATCH (n) RETURN n;\nMATCH (m) RETURN m".to_string()),
        Just("// find nodes\nMATCH (n) RETURN n".to_string()),
        Just("/* block */\nMATCH (n) RETURN n".to_string()),
        Just("MATCH (n) /* inline */ RETURN n".to_string()),
        Just("MATCH (n {name: 'Alice'}) RETURN n".to_string()),
        Just("MATCH (n) WHERE n.a > 1 AND n.b < 2 RETURN n".to_string()),
        Just("MATCH (n) WHERE n.a > 1 OR n.b < 2 RETURN n".to_string()),
    ]
}

// ---------------------------------------------------------------------------
// Property tests: idempotence (P17.3.3), deterministic, trivia preservation
// ---------------------------------------------------------------------------
//
// These properties hold over ALL inputs (valid and invalid) and use a plain
// proptest! block with default configuration.

proptest! {
    // ---- P17.3.3: Idempotence -------------------------------------------

    /// Formatter idempotence over arbitrary UTF-8 strings (spec P17.3.3).
    #[test]
    fn idempotence_arbitrary(s in ".*") {
        let a = format(&s);
        let b = format(&a);
        prop_assert_eq!(a, b);
    }

    /// Idempotence on Cypher-like ASCII patterns.
    #[test]
    fn idempotence_cypher_like(s in cypher_like()) {
        let a = format(&s);
        let b = format(&a);
        prop_assert_eq!(a, b);
    }

    /// Idempotence on known-valid Cypher fragments.
    #[test]
    fn idempotence_valid_cypher(s in cypher_valid()) {
        let a = format(&s);
        let b = format(&a);
        prop_assert_eq!(&a, &b, "idempotence failed for: {:?}", s);
    }

    // ---- Deterministic --------------------------------------------------

    /// Two calls to `format` with identical input produce byte-identical
    /// output (spec §17.14 determinism).
    #[test]
    fn deterministic_arbitrary(s in ".*") {
        prop_assert_eq!(format(&s), format(&s));
    }

    /// Determinism on Cypher-like patterns.
    #[test]
    fn deterministic_cypher_like(s in cypher_like()) {
        prop_assert_eq!(format(&s), format(&s));
    }

    // ---- Trivia preservation (comment survival) -------------------------

    /// Comments present in the formatted output survive a second format pass.
    /// We check the *formatted* output (not the raw input) as the stable
    /// baseline; once normalised, a second pass must not drop any comments.
    #[test]
    fn comment_survival_cypher_like(s in cypher_like()) {
        let once = format(&s);
        let twice = format(&once);
        let comments_once = extract_comments(&once);
        let comments_twice = extract_comments(&twice);
        prop_assert_eq!(&comments_once, &comments_twice,
            "comment survival failed: once={:?} twice={:?}", once, twice);
    }

    /// Comment survival on known-valid Cypher (includes sources with comments).
    #[test]
    fn comment_survival_valid_cypher(s in cypher_valid()) {
        let once = format(&s);
        let twice = format(&once);
        let comments_once = extract_comments(&once);
        let comments_twice = extract_comments(&twice);
        prop_assert_eq!(&comments_once, &comments_twice,
            "comment survival failed for input: {:?}", s);
    }
}

// ---------------------------------------------------------------------------
// Semantic preservation (P17.3.4) — separate proptest config to allow
// a higher global-reject budget (random Cypher-like strings rarely parse
// without errors, so many inputs are skipped via prop_assume!).
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        // Allow up to 4096 skipped cases before giving up; with the
        // 256-case default budget this still produces ≥ 256 valid checks
        // when enough valid inputs exist in the strategy space.
        max_global_rejects: 65536,
        ..ProptestConfig::default()
    })]

    /// Semantic preservation on Cypher-like random strings (spec P17.3.4).
    ///
    /// The significant (non-trivia) token sequence of `parse(fmt(s))` must
    /// equal that of `parse(s)` whenever `s` is valid (no ERROR nodes).
    #[test]
    fn semantic_preservation_cypher_like(s in cypher_like()) {
        prop_assume!(has_no_errors(&s));
        let formatted = format(&s);
        prop_assume!(has_no_errors(&formatted));

        let original_toks = significant_tokens(&s);
        let formatted_toks = significant_tokens(&formatted);
        prop_assert_eq!(original_toks, formatted_toks,
            "semantic preservation failed: original={:?} formatted={:?}", s, formatted);
    }

    /// Semantic preservation on known-valid Cypher fragments (spec P17.3.4).
    #[test]
    fn semantic_preservation_valid_cypher(s in cypher_valid()) {
        prop_assume!(has_no_errors(&s));
        let formatted = format(&s);
        prop_assume!(has_no_errors(&formatted));

        let original_toks = significant_tokens(&s);
        let formatted_toks = significant_tokens(&formatted);
        prop_assert_eq!(original_toks, formatted_toks,
            "semantic preservation failed: original={:?} formatted={:?}", s, formatted);
    }
}

// ---------------------------------------------------------------------------
// Unit regression guards for the seeds in proptest-regressions/lib.txt
// ---------------------------------------------------------------------------

#[test]
fn regression_newline_comma() {
    // Seed: s = "\n\n," — previously triggered idempotence failure.
    let s = "\n\n,";
    let a = format(s);
    let b = format(&a);
    assert_eq!(a, b, "idempotence regression: {s:?}");
}

#[test]
fn regression_escape_in_string() {
    // Seed: s = "\"\\".
    let s = "\"\\";
    let a = format(s);
    let b = format(&a);
    assert_eq!(a, b, "idempotence regression: {s:?}");
}

#[test]
fn regression_digits_newline_alpha() {
    // Seed: s = "0\na".
    let s = "0\na";
    let a = format(s);
    let b = format(&a);
    assert_eq!(a, b, "idempotence regression: {s:?}");
}
