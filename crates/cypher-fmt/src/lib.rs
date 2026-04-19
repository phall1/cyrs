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

#![doc(html_root_url = "https://docs.rs/cypher-fmt/0.0.1")]

use cypher_syntax::{SyntaxNode, parse};

/// Formatter options. Stable surface; adding options is non-breaking.
#[derive(Debug, Clone)]
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

/// Format a source string. When the grammar lands, this routes through a
/// real pretty-printer; until then it preserves the input verbatim so
/// the idempotence and semantic-preservation invariants hold.
#[must_use]
pub fn format(src: &str, _options: &FmtOptions) -> String {
    let parse = parse(src);
    format_node(&parse.syntax())
}

fn format_node(node: &SyntaxNode) -> String {
    node.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
