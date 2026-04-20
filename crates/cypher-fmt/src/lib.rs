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

/// Formatter options (spec 0001 §13.3). Stable surface; adding options is
/// non-breaking.
///
/// All defaults reproduce the pre-options formatter behaviour so existing
/// snapshots remain valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    /// Soft line-length limit in columns (default 100, spec §13.3).
    pub width: usize,
    /// Keyword casing (default `Upper`, spec §13.3).
    pub keyword_casing: KeywordCasing,
    /// Trailing-comma policy (default `AsNeeded`, spec §13.3).
    pub trailing_commas: TrailingCommas,
    /// Indentation style (default 2 spaces, spec §13.3).
    pub indent: Indent,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            width: 100,
            keyword_casing: KeywordCasing::Upper,
            trailing_commas: TrailingCommas::AsNeeded,
            indent: Indent::Spaces(2),
        }
    }
}

/// Keyword casing option (spec 0001 §13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordCasing {
    /// Convert all keywords to uppercase (e.g. `MATCH`, `RETURN`).
    Upper,
    /// Convert all keywords to lowercase (e.g. `match`, `return`).
    Lower,
    /// Leave keyword casing exactly as found in the source.
    Preserve,
}

/// Trailing-comma policy (spec 0001 §13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailingCommas {
    /// Always emit a trailing comma after the last item in a list.
    Always,
    /// Emit a trailing comma only when it aids readability (multi-line lists).
    AsNeeded,
    /// Never emit a trailing comma.
    Never,
}

/// Indentation style (spec 0001 §13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    /// Indent with N spaces per level.
    Spaces(usize),
    /// Indent with one hard tab per level.
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

/// Errors that can occur during formatting.
///
/// Currently formatting is always successful (the formatter is
/// partial-input-tolerant per §13.1). This type is reserved for future
/// hard-failure modes (e.g. cyclic trivia, pathological inputs) so the API
/// surface is stable.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// Placeholder — no conditions currently produce this.
    #[doc(hidden)]
    __NonExhaustive,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::__NonExhaustive => write!(f, "internal formatter error"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Format a source string with the given options (spec 0001 §13.3).
///
/// Returns `Ok(formatted)` on success. The formatter is partial-input-tolerant
/// (spec §13.1): it formats what it can and emits error regions verbatim, so
/// this function currently never returns `Err`.
///
/// # Magic-comment toggle (spec §13.4)
///
/// A `// cypher-fmt: off` comment suspends formatting until `// cypher-fmt:
/// on`. The suspended region is emitted verbatim.
pub fn format_with(src: &str, opts: &FormatOptions) -> Result<String, FormatError> {
    let parse = parse(src);
    let root = parse.syntax();
    let mut printer = Printer::new(opts.clone());
    printer.print_node(&root);
    Ok(printer.finish())
}

/// Format a source string with default options (spec 0001 §13.3).
///
/// Equivalent to `format_with(src, &FormatOptions::default()).unwrap()`.
#[must_use]
pub fn format(src: &str) -> String {
    format_with(src, &FormatOptions::default()).expect("formatter is infallible")
}

// ---------------------------------------------------------------------------
// Backward-compat alias for internal callers that still pass explicit opts.
// ---------------------------------------------------------------------------

/// Convenience alias used by the snapshot test helpers below.
#[allow(dead_code)]
fn fmt_with(src: &str, opts: &FormatOptions) -> String {
    format_with(src, opts).expect("formatter is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;

    fn fmt(src: &str) -> String {
        format(src)
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
        let a = format("");
        let b = format(&a);
        assert_eq!(a, b);
    }

    #[test]
    fn idempotent_on_simple() {
        let src = "MATCH (n) RETURN n";
        let a = format(src);
        let b = format(&a);
        assert_eq!(a, b);
    }

    #[test]
    fn idempotent_on_complex() {
        let src = "MATCH (n:Person)-[r:KNOWS]->(m) WHERE n.age > 21 RETURN n.name, m.name ORDER BY n.name";
        let a = format(src);
        let b = format(&a);
        assert_eq!(a, b);
    }

    #[test]
    fn idempotent_on_comments() {
        let src = "// comment\nMATCH (n) RETURN n";
        let a = format(src);
        let b = format(&a);
        assert_eq!(a, b);
    }

    // ------------------------------------------------------------------
    // Options: keyword_casing = Lower
    // ------------------------------------------------------------------

    #[test]
    fn snap_keyword_lower_match_return() {
        let opts = FormatOptions {
            keyword_casing: KeywordCasing::Lower,
            ..Default::default()
        };
        assert_snapshot!(format_with("MATCH (n) RETURN n", &opts).unwrap());
    }

    #[test]
    fn snap_keyword_lower_where() {
        let opts = FormatOptions {
            keyword_casing: KeywordCasing::Lower,
            ..Default::default()
        };
        assert_snapshot!(format_with("MATCH (n) WHERE n.age > 21 RETURN n", &opts).unwrap());
    }

    // ------------------------------------------------------------------
    // Options: keyword_casing = Preserve
    // ------------------------------------------------------------------

    #[test]
    fn snap_keyword_preserve_mixed() {
        let opts = FormatOptions {
            keyword_casing: KeywordCasing::Preserve,
            ..Default::default()
        };
        assert_snapshot!(format_with("Match (n) Return n", &opts).unwrap());
    }

    // ------------------------------------------------------------------
    // Options: indent = Tabs
    // ------------------------------------------------------------------

    #[test]
    fn snap_indent_tabs_match_return() {
        let opts = FormatOptions {
            indent: Indent::Tabs,
            ..Default::default()
        };
        // Indentation depth is not yet implemented (depth=0), so tab vs spaces
        // produces identical top-level output — this test just asserts no panic
        // and that the keyword casing default (Upper) is honoured.
        let out = format_with("match (n) return n", &opts).unwrap();
        assert!(out.contains("MATCH"), "expected MATCH in {out:?}");
    }

    // ------------------------------------------------------------------
    // Options: indent = Spaces(4)
    // ------------------------------------------------------------------

    #[test]
    fn snap_indent_4_spaces_match_return() {
        let opts = FormatOptions {
            indent: Indent::Spaces(4),
            ..Default::default()
        };
        let out = format_with("match (n) return n", &opts).unwrap();
        assert!(out.contains("MATCH"), "expected MATCH in {out:?}");
    }

    // ------------------------------------------------------------------
    // Options: trailing_commas = Never
    // ------------------------------------------------------------------

    #[test]
    fn snap_trailing_commas_never() {
        let opts = FormatOptions {
            trailing_commas: TrailingCommas::Never,
            ..Default::default()
        };
        assert_snapshot!(format_with("MATCH (n) RETURN n.name, n.age, n.id", &opts).unwrap());
    }

    // ------------------------------------------------------------------
    // Options: trailing_commas = Always
    // ------------------------------------------------------------------

    #[test]
    fn snap_trailing_commas_always() {
        let opts = FormatOptions {
            trailing_commas: TrailingCommas::Always,
            ..Default::default()
        };
        assert_snapshot!(format_with("MATCH (n) RETURN n.name, n.age, n.id", &opts).unwrap());
    }

    // ------------------------------------------------------------------
    // Magic-comment per-region toggle (spec §13.4)
    // ------------------------------------------------------------------

    #[test]
    fn snap_fmt_off_respects_options() {
        // Even with lowercase option, the off-region is emitted verbatim.
        let opts = FormatOptions {
            keyword_casing: KeywordCasing::Lower,
            ..Default::default()
        };
        assert_snapshot!(
            format_with(
                "// cypher-fmt: off\nMATCH (n) RETURN n\n// cypher-fmt: on\nmatch (m) return m",
                &opts,
            )
            .unwrap()
        );
    }

    // ------------------------------------------------------------------
    // format_with API: verify Result is Ok
    // ------------------------------------------------------------------

    #[test]
    fn format_with_returns_ok() {
        let result = format_with("MATCH (n) RETURN n", &FormatOptions::default());
        assert!(result.is_ok());
    }

    // ------------------------------------------------------------------
    // Property test: idempotence
    // ------------------------------------------------------------------

    use proptest::prelude::*;

    proptest! {
        /// Property P17.3.3 — formatter idempotence across arbitrary inputs.
        #[test]
        fn idempotence(s in ".*") {
            let a = format(&s);
            let b = format(&a);
            prop_assert_eq!(a, b);
        }

        /// Idempotence on ASCII Cypher-like patterns.
        #[test]
        fn idempotence_cypher_like(s in "[A-Z a-z0-9():,.\n;/*]{0,80}") {
            let a = format(&s);
            let b = format(&a);
            prop_assert_eq!(a, b);
        }
    }
}
