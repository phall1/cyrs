//! `cypher-fmt` — CST-driven formatter (spec 0001 §13).
//!
//! # Invariants
//!
//! - **Idempotence.** `fmt(fmt(s)) == fmt(s)` for all valid `s`.
//! - **Semantic preservation.** `parse(fmt(s)).ast()` structurally equals
//!   `parse(s).ast()` for valid `s`.
//! - **Trivia preservation.** Comments survive formatting; blank-line
//!   policy is "at most one consecutive blank line."
//!
//! The formatter walks the CST — not the AST — so it preserves malformed
//! fragments and never asserts.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-fmt/0.0.1")]

mod printer;

pub use printer::Printer;

use cypher_syntax::parse;

/// Formatter options. Stable surface; adding options is non-breaking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmtOptions {
    pub width: usize,
    pub keyword_casing: KeywordCasing,
    pub trailing_commas: TrailingCommas,
    pub indent: Indent,
}

impl Default for FmtOptions {
    fn default() -> Self {
        Self {
            width: 100,
            keyword_casing: KeywordCasing::Upper,
            trailing_commas: TrailingCommas::AsNeeded,
            indent: Indent::Spaces(2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordCasing {
    Upper,
    Lower,
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailingCommas {
    Always,
    AsNeeded,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    Spaces(usize),
    Tabs,
}

impl Indent {
    #[allow(dead_code)] // Used by Printer; reserved for indentation when depth support lands.
    pub(crate) fn as_str(&self, level: usize) -> String {
        match self {
            Indent::Spaces(n) => " ".repeat(*n * level),
            Indent::Tabs => "\t".repeat(level),
        }
    }
}

/// Format a source string with the given options.
///
/// When the CST has errors the formatter still emits output: it formats
/// what it can, and emits error regions verbatim (partial-input tolerance
/// per §13.1).
#[must_use]
pub fn format(src: &str, options: &FmtOptions) -> String {
    let parse = parse(src);
    let root = parse.syntax();
    let mut printer = Printer::new(options.clone());
    printer.print_node(&root);
    printer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;

    fn fmt(src: &str) -> String {
        format(src, &FmtOptions::default())
    }

    // ------------------------------------------------------------------
    // Basic single-statement cases (20+ snapshots)
    // ------------------------------------------------------------------

    #[test]
    fn snap_empty() {
        assert_snapshot!(fmt(""), @"");
    }

    #[test]
    fn snap_match_return() {
        assert_snapshot!(fmt("MATCH (n) RETURN n"));
    }

    #[test]
    fn snap_match_return_uppercase() {
        assert_snapshot!(fmt("match (n) return n"));
    }

    #[test]
    fn snap_match_where_return() {
        assert_snapshot!(fmt("MATCH (n) WHERE n.age > 21 RETURN n"));
    }

    #[test]
    fn snap_match_with_return() {
        assert_snapshot!(fmt("MATCH (n) WITH n RETURN n"));
    }

    #[test]
    fn snap_trailing_semicolon() {
        assert_snapshot!(fmt("MATCH (n) RETURN n;"));
    }

    #[test]
    fn snap_multiple_statements() {
        assert_snapshot!(fmt("MATCH (n) RETURN n; MATCH (m) RETURN m"));
    }

    #[test]
    fn snap_multiple_statements_trailing_semi() {
        assert_snapshot!(fmt("MATCH (n) RETURN n; MATCH (m) RETURN m;"));
    }

    #[test]
    fn snap_return_multiple_items() {
        assert_snapshot!(fmt("MATCH (n) RETURN n.name, n.age, n.id"));
    }

    #[test]
    fn snap_return_alias() {
        assert_snapshot!(fmt("MATCH (n) RETURN n.name AS name, n.age AS age"));
    }

    #[test]
    fn snap_match_relationship() {
        assert_snapshot!(fmt("MATCH (n)-[r]->(m) RETURN n, r, m"));
    }

    #[test]
    fn snap_match_labeled_node() {
        assert_snapshot!(fmt("MATCH (n:Person) RETURN n"));
    }

    #[test]
    fn snap_match_labeled_rel() {
        assert_snapshot!(fmt("MATCH (n)-[r:KNOWS]->(m) RETURN n"));
    }

    #[test]
    fn snap_match_property_map() {
        assert_snapshot!(fmt("MATCH (n {name: 'Alice'}) RETURN n"));
    }

    #[test]
    fn snap_where_and() {
        assert_snapshot!(fmt("MATCH (n) WHERE n.a > 1 AND n.b < 2 RETURN n"));
    }

    #[test]
    fn snap_where_or() {
        assert_snapshot!(fmt("MATCH (n) WHERE n.a > 1 OR n.b < 2 RETURN n"));
    }

    #[test]
    fn snap_return_distinct() {
        assert_snapshot!(fmt("MATCH (n) RETURN DISTINCT n"));
    }

    #[test]
    fn snap_order_by() {
        assert_snapshot!(fmt("MATCH (n) RETURN n ORDER BY n.name ASC"));
    }

    #[test]
    fn snap_skip_limit() {
        assert_snapshot!(fmt("MATCH (n) RETURN n SKIP 10 LIMIT 5"));
    }

    #[test]
    fn snap_unwind() {
        assert_snapshot!(fmt("UNWIND [1,2,3] AS x RETURN x"));
    }

    // ------------------------------------------------------------------
    // Comment preservation
    // ------------------------------------------------------------------

    #[test]
    fn snap_line_comment_before_clause() {
        assert_snapshot!(fmt("// find users\nMATCH (n) RETURN n"));
    }

    #[test]
    fn snap_line_comment_end_of_line() {
        assert_snapshot!(fmt("MATCH (n) // get node\nRETURN n"));
    }

    #[test]
    fn snap_block_comment() {
        assert_snapshot!(fmt("/* find all */ MATCH (n) RETURN n"));
    }

    #[test]
    fn snap_block_comment_multiline() {
        assert_snapshot!(fmt(
            "/* \n  find all nodes\n  in the graph\n*/\nMATCH (n) RETURN n"
        ));
    }

    #[test]
    fn snap_fmt_off_on() {
        assert_snapshot!(fmt(
            "// cypher-fmt: off\nmatch(n)return n\n// cypher-fmt: on\nMATCH (m) RETURN m"
        ));
    }

    // ------------------------------------------------------------------
    // Malformed / partial input tolerance
    // ------------------------------------------------------------------

    #[test]
    fn snap_malformed_incomplete() {
        assert_snapshot!(fmt("MATCH (n) WHERE"));
    }

    #[test]
    fn snap_malformed_garbage() {
        assert_snapshot!(fmt("!@#$%"));
    }

    #[test]
    fn snap_malformed_partial_then_valid() {
        assert_snapshot!(fmt("WHERE MATCH (n) RETURN n"));
    }

    #[test]
    fn snap_just_keyword() {
        assert_snapshot!(fmt("RETURN"));
    }

    // ------------------------------------------------------------------
    // Idempotence unit tests
    // ------------------------------------------------------------------

    #[test]
    fn idempotent_on_empty() {
        let o = FmtOptions::default();
        let a = format("", &o);
        let b = format(&a, &o);
        assert_eq!(a, b);
    }

    #[test]
    fn idempotent_on_simple() {
        let o = FmtOptions::default();
        let src = "MATCH (n) RETURN n";
        let a = format(src, &o);
        let b = format(&a, &o);
        assert_eq!(a, b);
    }

    #[test]
    fn idempotent_on_complex() {
        let o = FmtOptions::default();
        let src = "MATCH (n:Person)-[r:KNOWS]->(m) WHERE n.age > 21 RETURN n.name, m.name ORDER BY n.name";
        let a = format(src, &o);
        let b = format(&a, &o);
        assert_eq!(a, b);
    }

    #[test]
    fn idempotent_on_comments() {
        let o = FmtOptions::default();
        let src = "// comment\nMATCH (n) RETURN n";
        let a = format(src, &o);
        let b = format(&a, &o);
        assert_eq!(a, b);
    }

    // ------------------------------------------------------------------
    // Property test: idempotence
    // ------------------------------------------------------------------

    use proptest::prelude::*;

    proptest! {
        /// Property P17.3.3 — formatter idempotence across arbitrary inputs.
        #[test]
        fn idempotence(s in ".*") {
            let o = FmtOptions::default();
            let a = format(&s, &o);
            let b = format(&a, &o);
            prop_assert_eq!(a, b);
        }

        /// Idempotence on ASCII Cypher-like patterns.
        #[test]
        fn idempotence_cypher_like(s in "[A-Z a-z0-9():,.\n;/*]{0,80}") {
            let o = FmtOptions::default();
            let a = format(&s, &o);
            let b = format(&a, &o);
            prop_assert_eq!(a, b);
        }
    }
}
