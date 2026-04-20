//! Parser property tests (spec 0001 §17.3).
//!
//! Properties implemented here:
//!
//! - **P17.3.1 Parser totality** — `parse(s)` never panics for any UTF-8
//!   byte sequence, and `parse(s).syntax().to_string() == s` (bounded time,
//!   no panic).
//! - **P17.3.2 CST losslessness** — `parse(s).syntax().to_string() == s`
//!   for all inputs (the spec's exact statement of the invariant).
//!
//! Both properties are tested over:
//! 1. Arbitrary UTF-8 strings (via proptest's `".*"` strategy).
//! 2. Cypher-like ASCII patterns (char class matching common Cypher chars).
//! 3. A curated set of known-valid Cypher fragments.

use cypher_syntax::parse;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Cypher-like ASCII patterns that exercise common Cypher characters.
fn cypher_like() -> impl Strategy<Value = String> {
    "[A-Z a-z0-9():,.\n;/*_$'\"\\[\\]{}|+-]{0,120}".prop_map(|s| s)
}

/// Known-valid (and a few known-invalid) Cypher fragments for extra coverage.
fn cypher_fragments() -> impl Strategy<Value = String> {
    prop_oneof![
        // valid
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
        Just("// find nodes\nMATCH (n) RETURN n".to_string()),
        Just("/* block */\nMATCH (n) RETURN n".to_string()),
        Just("MATCH (n) /* inline */ RETURN n".to_string()),
        // invalid / partial — parser must survive these too
        Just("MATCH".to_string()),
        Just("RETURN".to_string()),
        Just("(".to_string()),
        Just(")".to_string()),
        Just("->".to_string()),
        Just("MATCH (n) RETURN".to_string()),
        Just(String::new()),
        Just("   ".to_string()),
        Just("\n\n".to_string()),
        Just("MATCH (n) WHERE RETURN n".to_string()),
    ]
}

// ---------------------------------------------------------------------------
// P17.3.1 — Parser totality
// ---------------------------------------------------------------------------
//
// `parse(s)` must return (no panic) for every UTF-8 string, and the
// resulting tree text must equal `s` (losslessness is part of the totality
// contract per spec §17.3 P17.3.1).

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1024,
        ..ProptestConfig::default()
    })]

    /// Totality over arbitrary UTF-8 strings.
    #[test]
    fn totality_arbitrary(s in ".*") {
        // Must not panic.
        let p = parse(&s);
        // Losslessness is asserted here as well (spec P17.3.1 wording).
        let text = p.syntax().to_string();
        prop_assert_eq!(text, s);
    }

    /// Totality over Cypher-like ASCII patterns.
    #[test]
    fn totality_cypher_like(s in cypher_like()) {
        let p = parse(&s);
        let text = p.syntax().to_string();
        prop_assert_eq!(text, s);
    }

    /// Totality over curated Cypher fragments (including invalid ones).
    #[test]
    fn totality_fragments(s in cypher_fragments()) {
        let p = parse(&s);
        let text = p.syntax().to_string();
        prop_assert_eq!(text, s);
    }
}

// ---------------------------------------------------------------------------
// P17.3.2 — CST losslessness
// ---------------------------------------------------------------------------
//
// `parse(s).syntax().to_string() == s` for all inputs.
// (Same underlying check as totality; kept as a separate proptest block
// with its own name so failures are labelled correctly.)

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1024,
        ..ProptestConfig::default()
    })]

    /// Losslessness over arbitrary UTF-8 strings (spec P17.3.2).
    #[test]
    fn losslessness_arbitrary(s in ".*") {
        let text = parse(&s).syntax().to_string();
        prop_assert_eq!(text, s,
            "CST losslessness violated: parse(s).syntax().to_string() != s");
    }

    /// Losslessness over Cypher-like ASCII patterns (spec P17.3.2).
    #[test]
    fn losslessness_cypher_like(s in cypher_like()) {
        let text = parse(&s).syntax().to_string();
        prop_assert_eq!(text, s,
            "CST losslessness violated: parse(s).syntax().to_string() != s");
    }

    /// Losslessness over curated Cypher fragments (spec P17.3.2).
    #[test]
    fn losslessness_fragments(s in cypher_fragments()) {
        let text = parse(&s).syntax().to_string();
        prop_assert_eq!(text, s,
            "CST losslessness violated: parse(s).syntax().to_string() != s");
    }
}

// ---------------------------------------------------------------------------
// Regression guards
// ---------------------------------------------------------------------------

/// Empty string: `parse("").syntax().to_string() == ""`
#[test]
fn regression_empty_input() {
    let s = "";
    assert_eq!(parse(s).syntax().to_string(), s);
}

/// Input that is purely whitespace must round-trip losslessly.
#[test]
fn regression_whitespace_only() {
    let s = "   \n\t  ";
    assert_eq!(parse(s).syntax().to_string(), s);
}

/// Partial keyword must not panic.
#[test]
fn regression_partial_keyword() {
    let s = "MATC";
    let _ = parse(s); // must not panic
    assert_eq!(parse(s).syntax().to_string(), s);
}

/// Unclosed string literal must not panic (E0004 recovery).
#[test]
fn regression_unclosed_string() {
    let s = r#"MATCH (n {name: "Alice}) RETURN n"#;
    let _ = parse(s);
    assert_eq!(parse(s).syntax().to_string(), s);
}
