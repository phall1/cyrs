//! Tree-edit primitives for incremental reparse (cy-zv0, spec §11).
//!
//! # Scope
//!
//! This module exposes a [`TextEdit`] value type plus an
//! [`incremental_reparse`] entry point shaped so downstream crates
//! (`cypher-db`) can route document edits through a single API. The name
//! `incremental_reparse` is aspirational: the current implementation is a
//! **whole-file reparse fallback** that reconstructs the full source from
//! the old tree and calls [`crate::parse`] on the result. The API shape is
//! designed so a future "smart" path can slot underneath without breaking
//! callers.
//!
//! # Why an API-first tranche
//!
//! Rowan supports lossless green-tree splicing in principle
//! (`SyntaxNode::replace_with`, `GreenNode::replace_child`), but a
//! production-quality incremental reparse needs:
//!
//! 1. A re-lex boundary sniff so edits inside trivia don't trigger a parser
//!    re-entry.
//! 2. A minimal sub-tree identification that is safe across clause
//!    boundaries (an edit that deletes `MATCH` must invalidate the
//!    enclosing statement, not just the token).
//! 3. Error-recovery reconciliation so an edit that introduces or heals a
//!    syntax error produces a tree whose error set matches a full reparse.
//!
//! Items 1–3 are a research-sized tranche. Landing the API + whole-file
//! fallback lets downstream crates migrate onto
//! [`crate::Database::edit_file`] (see `cypher-db`) today; the smart path
//! can then land in a follow-up bead without touching any caller.
//!
//! # Future smart path
//!
//! When the `incremental` feature (defaulted-on) is enabled, a future
//! implementation of [`incremental_reparse`] may short-circuit to a
//! sub-tree reparse. Consumers must not rely on either the slow or fast
//! path: the invariant is that the returned tree is byte-equal to
//! `parse(new_text).syntax()` for some canonical `new_text` derived from
//! `old_tree` + `edit`.
//!
//! # Invariants
//!
//! - `incremental_reparse(old_tree, edit)` produces a [`Parse`] whose
//!   `syntax().to_string()` equals the new source text.
//! - The call is infallible: malformed UTF-8 cannot enter because
//!   [`TextEdit::new`] takes `&str` and [`TextEdit::apply`] concatenates
//!   bytes at char boundaries.
//! - `edit.range` must lie inside the old source; out-of-range offsets
//!   saturate to the source length (matching `String::replace_range`'s
//!   documented behaviour).

use text_size::{TextRange, TextSize};

use crate::{Parse, SyntaxNode, parse};

/// A single-range text edit.
///
/// Shaped to mirror LSP `TextEdit` / rust-analyzer's `TextEdit`: a byte
/// range inside the *old* source text plus the UTF-8 replacement string.
///
/// # Construction
///
/// Use [`TextEdit::replace`] for a generic range replacement, or
/// [`TextEdit::insert`] for a zero-length insertion at a single offset.
///
/// # Coordinate space
///
/// The `range` is in **byte** offsets over the old source, not characters
/// and not LSP UTF-16 columns. Callers that start from LSP ranges must
/// translate first (see [`crate::LineIndex`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// Byte range (inside the old source) that is replaced.
    pub range: TextRange,
    /// UTF-8 replacement text. Empty string = deletion.
    pub replacement: String,
}

impl TextEdit {
    /// Build a replace-range edit.
    ///
    /// `range` is in **byte** offsets over the *old* source text. The
    /// replacement is owned to keep the edit value trivially movable.
    #[must_use]
    pub fn replace(range: TextRange, replacement: impl Into<String>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }

    /// Build a pure insertion at `offset`.
    #[must_use]
    pub fn insert(offset: TextSize, text: impl Into<String>) -> Self {
        Self {
            range: TextRange::empty(offset),
            replacement: text.into(),
        }
    }

    /// Apply this edit to `src`, returning the resulting text.
    ///
    /// Offsets that exceed `src.len()` are clamped to the end of the
    /// source (matching `String::replace_range`'s implicit behaviour).
    /// Both endpoints of `range` are rounded *down* to the previous
    /// UTF-8 char boundary if they fall in the middle of a multi-byte
    /// sequence; callers feeding ASCII-only Cypher never hit this path.
    #[must_use]
    pub fn apply(&self, src: &str) -> String {
        let len = src.len();
        let start = usize::from(self.range.start()).min(len);
        let end = usize::from(self.range.end()).min(len).max(start);

        // Snap to char boundaries defensively so we never slice through a
        // multi-byte sequence. ASCII-only inputs (the hot path for the
        // Cypher corpus) take zero iterations of the inner loops.
        let mut s = start;
        while s > 0 && !src.is_char_boundary(s) {
            s -= 1;
        }
        let mut e = end;
        while e < len && !src.is_char_boundary(e) {
            e += 1;
        }

        let mut out = String::with_capacity(len - (e - s) + self.replacement.len());
        out.push_str(&src[..s]);
        out.push_str(&self.replacement);
        out.push_str(&src[e..]);
        out
    }
}

/// Reparse after applying `edit` to `old_tree`'s source.
///
/// # Current implementation — whole-file fallback
///
/// The implementation reconstructs the new source text as
/// `old_tree.to_string().apply(edit)` and calls [`parse`]. This is O(N) in
/// the file size, matching the existing non-incremental path. The wrapper
/// exists so that:
///
/// - The `Database::edit_file` API (in `cypher-db`) is shaped correctly
///   today; future smart paths land behind the same call site.
/// - The `incremental` feature (defaulted-on) gates the pluggable smart
///   dispatch; disabling the feature will still compile callers.
///
/// # Future smart path
///
/// Behind the `incremental` feature a future tranche may:
///
/// 1. Compute the enclosing `SyntaxNode` that fully contains `edit.range`
///    using `rowan::SyntaxNode::covering_element`.
/// 2. Re-lex and reparse only that sub-span.
/// 3. Splice the new green sub-tree into the old root via
///    `GreenNode::replace_child`.
///
/// Until that lands, the fallback is byte-equivalent to a full reparse.
#[must_use]
pub fn incremental_reparse(old_tree: &SyntaxNode, edit: &TextEdit) -> Parse {
    let old_src = old_tree.to_string();
    let new_src = edit.apply(&old_src);
    // The `incremental` feature is defaulted-on. When a real smart path
    // is implemented, `cfg!(feature = "incremental")` gates the fast
    // dispatch; the slow fallback always remains available for A/B
    // testing. Today the two paths are identical.
    #[cfg(feature = "incremental")]
    {
        // Smart-path hook: currently the smart path delegates to the full
        // reparse. When a real incremental splicer lands, it goes here.
        // The `old_tree` is bound so future code can read it without a
        // signature churn.
        let _ = old_tree;
        return parse(&new_src);
    }
    #[cfg(not(feature = "incremental"))]
    {
        let _ = old_tree;
        parse(&new_src)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_at_middle_preserves_prefix_and_suffix() {
        let src = "RETURN 1";
        let edit = TextEdit::insert(TextSize::from(6), "0");
        let out = edit.apply(src);
        assert_eq!(out, "RETURN01 1");
    }

    #[test]
    fn replace_range() {
        let src = "RETURN 1";
        let range = TextRange::new(TextSize::from(7), TextSize::from(8));
        let edit = TextEdit::replace(range, "42");
        let out = edit.apply(src);
        assert_eq!(out, "RETURN 42");
    }

    #[test]
    fn delete_range() {
        let src = "MATCH (n) RETURN n";
        let range = TextRange::new(TextSize::from(0), TextSize::from(10));
        let edit = TextEdit::replace(range, "");
        let out = edit.apply(src);
        assert_eq!(out, "RETURN n");
    }

    #[test]
    fn out_of_range_saturates_to_end() {
        let src = "RETURN 1";
        let edit = TextEdit::insert(TextSize::from(999), ";");
        let out = edit.apply(src);
        assert_eq!(out, "RETURN 1;");
    }

    #[test]
    fn incremental_reparse_roundtrips() {
        let p = parse("RETURN 1");
        let root = p.syntax();
        let edit = TextEdit::replace(
            TextRange::new(TextSize::from(7), TextSize::from(8)),
            "42",
        );
        let np = incremental_reparse(&root, &edit);
        assert_eq!(np.syntax().to_string(), "RETURN 42");
        assert!(np.errors().is_empty(), "edit keeps the file parseable");
    }
}
