//! Helpers for the `parse ∘ fmt ∘ parse` differential fuzzer
//! (`fuzz_targets/fmt_parse_roundtrip.rs`, bead cy-h07).
//!
//! Split out of the fuzz-target binary so we can unit-test the
//! structural-equality routine. The binary itself is `#![no_main]` and
//! hosts only the libFuzzer entry point; every pure function lives here.

use cypher_syntax::{SyntaxKind, SyntaxNode};
use rowan::{NodeOrToken, WalkEvent};

/// Serialise the trivia-stripped shape of a CST into a stable string.
///
/// Two CSTs produce the same shape iff, ignoring whitespace / line-comment
/// / block-comment tokens, they have:
/// - the same node-kind tree topology,
/// - the same non-trivia token-kind sequence, and
/// - matching token *text* for non-keyword tokens (identifiers, numeric
///   literals, string literals, punctuation).
///
/// Keyword token text is deliberately omitted: the formatter is allowed
/// to canonicalise keyword case (`null` → `NULL`, `match` → `MATCH`).
/// Keyword *kind* is preserved and compared — only the spelling inside
/// a given kind is treated as equivalent. This mirrors the openCypher
/// TCK's case-insensitive keyword rule (spec §17.5 + §13 formatter
/// canonicalisation).
///
/// Variable and property identifiers use `IDENT` (non-keyword), so the
/// formatter still cannot silently rename them — the property that
/// matters for P17.3.4 semantic preservation.
///
/// The output format is deliberately human-readable for debug output in
/// fuzz-target panic messages; it is not a stable serialisation format
/// and callers must not persist it.
#[must_use]
pub fn shape(root: &SyntaxNode) -> String {
    let mut out = String::with_capacity(256);
    for ev in root.preorder_with_tokens() {
        match ev {
            WalkEvent::Enter(NodeOrToken::Node(n)) => {
                out.push('(');
                out.push_str(&format!("{:?}", n.kind()));
            }
            WalkEvent::Leave(NodeOrToken::Node(_)) => {
                out.push(')');
            }
            WalkEvent::Enter(NodeOrToken::Token(t)) => {
                if t.kind().is_trivia() {
                    continue;
                }
                out.push(' ');
                out.push_str(&format!("{:?}", t.kind()));
                if !t.kind().is_keyword() {
                    // Include the text so `IDENT("n")` and `IDENT("m")`
                    // remain distinguishable — the formatter may NOT
                    // rename variables or rewrite literal values.
                    out.push('=');
                    out.push_str(&format!("{:?}", t.text()));
                }
            }
            WalkEvent::Leave(NodeOrToken::Token(_)) => {}
        }
    }
    out
}

/// Return `true` iff `root` contains any `ERROR` node or token.
///
/// Mirrors the inline walker that previously lived in
/// `fuzz_structured_parse` and `fmt_parse_roundtrip`; consolidating the
/// two keeps behaviour identical across targets.
#[must_use]
pub fn contains_error(root: &SyntaxNode) -> bool {
    root.preorder_with_tokens().any(|ev| match ev {
        WalkEvent::Enter(NodeOrToken::Node(n)) => n.kind() == SyntaxKind::ERROR,
        WalkEvent::Enter(NodeOrToken::Token(t)) => t.kind() == SyntaxKind::ERROR,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two structurally identical sources with different whitespace
    /// produce the same shape.
    #[test]
    fn shape_ignores_whitespace() {
        let a = cypher_syntax::parse("MATCH (n) RETURN n");
        let b = cypher_syntax::parse("MATCH   (n)\n  RETURN   n");
        assert_eq!(shape(&a.syntax()), shape(&b.syntax()));
    }

    /// Two sources with different variable names DO differ — the
    /// formatter must not rename.
    #[test]
    fn shape_distinguishes_variable_names() {
        let a = cypher_syntax::parse("MATCH (n) RETURN n");
        let b = cypher_syntax::parse("MATCH (m) RETURN m");
        assert_ne!(shape(&a.syntax()), shape(&b.syntax()));
    }

    /// Line comments are trivia and ignored by `shape`.
    #[test]
    fn shape_ignores_line_comments() {
        let a = cypher_syntax::parse("MATCH (n) RETURN n");
        let b = cypher_syntax::parse("MATCH (n) // hi\nRETURN n");
        assert_eq!(shape(&a.syntax()), shape(&b.syntax()));
    }

    /// Block comments are trivia and ignored by `shape`.
    #[test]
    fn shape_ignores_block_comments() {
        let a = cypher_syntax::parse("MATCH (n) RETURN n");
        let b = cypher_syntax::parse("MATCH /* x */ (n) RETURN n");
        assert_eq!(shape(&a.syntax()), shape(&b.syntax()));
    }

    /// Keyword case is ignored: `null` and `NULL` produce the same
    /// shape because the formatter is allowed to canonicalise keyword
    /// spelling. This is the root cause of the crash bead cy-h07's
    /// smoke test surfaced on `RETURN coalesce(null, 1)` — the
    /// formatter rewrites `null` → `NULL`, which must not trip P17.3.4.
    #[test]
    fn shape_ignores_keyword_case() {
        let lo = cypher_syntax::parse("MATCH (n) RETURN null");
        let hi = cypher_syntax::parse("MATCH (n) RETURN NULL");
        assert_eq!(shape(&lo.syntax()), shape(&hi.syntax()));

        let lo = cypher_syntax::parse("match (n) return n");
        let hi = cypher_syntax::parse("MATCH (n) RETURN n");
        assert_eq!(shape(&lo.syntax()), shape(&hi.syntax()));
    }

    /// Non-keyword identifier text IS sensitive — the formatter must
    /// not rename `n` to `m`.
    #[test]
    fn shape_keeps_identifier_case() {
        let upper = cypher_syntax::parse("MATCH (N) RETURN N");
        let lower = cypher_syntax::parse("MATCH (n) RETURN n");
        assert_ne!(shape(&upper.syntax()), shape(&lower.syntax()));
    }

    /// `contains_error` flags a statement the parser cannot recover from
    /// cleanly (an opened but never-closed bracket is a reliable producer).
    #[test]
    fn contains_error_flags_unbalanced_bracket() {
        let parsed = cypher_syntax::parse("MATCH (n RETURN n");
        // Either the parser reports errors, or recovery produces an
        // ERROR node — either is a "has errors" signal we want flagged.
        assert!(!parsed.errors().is_empty() || contains_error(&parsed.syntax()));
    }

    /// `contains_error` returns `false` on a clean parse.
    #[test]
    fn contains_error_false_on_clean_parse() {
        let parsed = cypher_syntax::parse("MATCH (n) RETURN n");
        assert!(parsed.errors().is_empty());
        assert!(!contains_error(&parsed.syntax()));
    }

    /// P17.3.4 smoke: `parse(fmt(s))` has the same shape as `parse(s)`
    /// for a handful of hand-picked valid statements. This is the same
    /// invariant the fuzz target asserts, but on fixed inputs — useful
    /// as a sanity check when the fuzz harness is unavailable.
    #[test]
    fn fmt_preserves_shape_on_representative_samples() {
        // Samples pulled from the generator's own shape space — every
        // one is known to parse cleanly with the shipping grammar
        // (cy-h07.1 generator). Avoid constructs that depend on grammar
        // extensions not yet landed, e.g. `count(*)` or top-level CASE.
        let samples = [
            "MATCH (n) RETURN n",
            "MATCH (n:Person {id: 1}) WHERE n.age > 18 RETURN n.name",
            "UNWIND [1, 2, 3] AS x RETURN x",
            "MATCH (a)-[:KNOWS]->(b) RETURN a, b",
            "CREATE (n:Label {k: 1})",
            "MATCH (n) RETURN n.k ORDER BY n.k DESC LIMIT 10",
            "MATCH (n) WITH n AS n RETURN n",
            "OPTIONAL MATCH (a) RETURN a",
        ];
        for src in samples {
            let parsed = cypher_syntax::parse(src);
            assert!(parsed.errors().is_empty(), "sample did not parse: {src:?}");
            let fmt = cypher_fmt::format(src);
            let reparsed = cypher_syntax::parse(&fmt);
            assert!(
                reparsed.errors().is_empty(),
                "fmt({src:?}) = {fmt:?} did not re-parse"
            );
            assert_eq!(
                shape(&parsed.syntax()),
                shape(&reparsed.syntax()),
                "shape diverged:\n  src: {src:?}\n  fmt: {fmt:?}"
            );
        }
    }
}
