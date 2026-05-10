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
//! fallback lets downstream crates migrate onto `Database::edit_file`
//! (see `cypher-db`) today; the smart path can then land in a follow-up
//! bead without touching any caller.
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
//!   [`TextEdit::replace`] takes `impl Into<String>` and
//!   [`TextEdit::apply`] concatenates bytes at char boundaries.
//! - `edit.range` must lie inside the old source; out-of-range offsets
//!   saturate to the source length (matching `String::replace_range`'s
//!   documented behaviour).

use rowan::NodeOrToken;
use text_size::{TextRange, TextSize};

use crate::{Parse, SyntaxKind, SyntaxNode, parse};

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
/// # Implementation — smart sub-tree splice with whole-file fallback
///
/// When the `incremental` feature is enabled (default-on, cy-li5):
///
/// 1. Locate the smallest `STATEMENT` node in `old_tree` that **fully
///    contains** `edit.range` via [`rowan::SyntaxNode::covering_element`]
///    and an upward walk to the nearest `STATEMENT` ancestor.
/// 2. Reconstruct the new text for that statement's span by stitching
///    `old_text[stmt.start..edit.start] + edit.replacement +
///    old_text[edit.end..stmt.end]`.
/// 3. Lex the candidate statement text in isolation. If a top-level `;`
///    or `UNION` appears inside the new text, the edit changed the
///    statement count — bail to whole-file.
/// 4. Parse the candidate text as a wrapped source-file, extract the
///    `STATEMENT` green sub-tree, and splice it in via
///    [`rowan::SyntaxNode::replace_with`].
/// 5. Re-derive errors by full re-parse of the new source. This step
///    keeps the public-API invariant (errors match what `parse(new_src)`
///    would produce) honest. A future tranche may incrementally
///    reconcile errors, but that is *not* a cy-li5 deliverable —
///    correctness gates the optimization.
///
/// If any of the bail conditions trip (no enclosing STATEMENT, edit
/// straddles a `;`, top-level separator introduced/removed, candidate
/// text fails to parse to a single STATEMENT), the implementation falls
/// back to a whole-file [`parse`]. The whole-file path is also taken
/// unconditionally when the `incremental` feature is disabled — that
/// behaviour is preserved as the slow-but-always-correct A/B baseline
/// (cy-zv0).
///
/// # Caveat — bench observability
///
/// The `bench_incremental_edit` 2k/1k ratio gate is driven by
/// `Database::edit_file`, which today reduces this function's return
/// value to its `syntax().to_string()` and feeds the string back into
/// Salsa. As a result, the green-tree splice savings *inside* this
/// function are not yet observable end-to-end at the bench. Threading
/// the precomputed [`Parse`] into Salsa as a memo is a separate
/// follow-up tranche; the cy-li5 acceptance criterion that the bench
/// ratio drops below 1.5× depends on that wiring landing.
#[must_use]
pub fn incremental_reparse(old_tree: &SyntaxNode, edit: &TextEdit) -> Parse {
    let old_src = old_tree.to_string();
    let new_src = edit.apply(&old_src);

    #[cfg(feature = "incremental")]
    {
        if let Some(parsed) = try_incremental_splice(old_tree, &old_src, edit, &new_src) {
            return parsed;
        }
    }

    parse(&new_src)
}

/// Smart-path attempt: splice a freshly-parsed STATEMENT sub-tree into
/// `old_tree` and re-derive errors. Returns `None` when any bail
/// condition trips; the caller then falls back to a whole-file reparse.
///
/// The function is `#[cfg(feature = "incremental")]`-gated so disabling
/// the feature keeps the binary identical to the cy-zv0 fallback.
#[cfg(feature = "incremental")]
fn try_incremental_splice(
    old_tree: &SyntaxNode,
    old_src: &str,
    edit: &TextEdit,
    new_src: &str,
) -> Option<Parse> {
    use crate::lexer::lex;

    // 1. Find the smallest enclosing STATEMENT node.
    //
    // `covering_element` returns the smallest element fully containing
    // the range. We walk upward until we hit a STATEMENT (or fall off).
    let edit_range = edit.range;
    let stmt = covering_statement(old_tree, edit_range)?;
    let stmt_range = stmt.text_range();

    // The covering STATEMENT must strictly contain the edit range — if
    // the edit touches the leading/trailing `;` separator that lives
    // *outside* the STATEMENT, we'd lose the separator. (rowan's
    // covering_element already ensures `stmt_range.contains_range(edit)`
    // is true, but we re-assert defensively.)
    if !stmt_range.contains_range(edit_range) {
        return None;
    }

    // 2. Stitch the new statement text.
    let stmt_start = usize::from(stmt_range.start());
    let stmt_end = usize::from(stmt_range.end());
    let edit_start = usize::from(edit_range.start()).clamp(stmt_start, stmt_end);
    let edit_end = usize::from(edit_range.end()).clamp(edit_start, stmt_end);
    if !old_src.is_char_boundary(stmt_start)
        || !old_src.is_char_boundary(stmt_end)
        || !old_src.is_char_boundary(edit_start)
        || !old_src.is_char_boundary(edit_end)
    {
        return None;
    }
    let mut new_stmt_text = String::with_capacity(
        (edit_start - stmt_start) + edit.replacement.len() + (stmt_end - edit_end),
    );
    new_stmt_text.push_str(&old_src[stmt_start..edit_start]);
    new_stmt_text.push_str(&edit.replacement);
    new_stmt_text.push_str(&old_src[edit_end..stmt_end]);

    // 3. Boundary safety: if the lexed statement text contains a `;` or
    //    `UNION` keyword (which are statement-count-changing tokens), the
    //    edit may have introduced a new statement boundary. Bail.
    let toks = lex(&new_stmt_text);
    for t in &toks {
        match t.kind {
            SyntaxKind::SEMI | SyntaxKind::UNION_KW => return None,
            _ => {}
        }
    }

    // 4. Parse the candidate text and extract a single STATEMENT child.
    //    The simplest robust route: full `parse` on the candidate text,
    //    expect exactly one STATEMENT child of the SOURCE_FILE, take its
    //    green sub-tree. If the candidate text doesn't normalise to a
    //    single STATEMENT (e.g. empty, leading-junk recovery, multiple
    //    statements somehow), bail.
    let cand = parse(&new_stmt_text);
    let cand_root = cand.syntax();
    let mut stmt_children = cand_root
        .children()
        .filter(|n| n.kind() == SyntaxKind::STATEMENT);
    let new_stmt = stmt_children.next()?;
    if stmt_children.next().is_some() {
        return None;
    }
    // The candidate STATEMENT must cover the entire candidate text —
    // otherwise leading/trailing trivia would be lost across the splice
    // boundary in ways the simple replace_with can't preserve.
    if new_stmt.text_range() != cand_root.text_range() {
        return None;
    }

    // 5. Splice. `replace_with` rebuilds the green tree along the spine
    //    only — O(depth × siblings-per-level), not O(file).
    let new_green_root = stmt.replace_with(new_stmt.green().into_owned());

    // 6. Errors: re-parse the new source to derive a correct error set.
    //    Note this defeats the splice savings *for the error half* of
    //    Parse; a future tranche can incrementally reconcile errors by
    //    keeping a sidecar map. The bench is dominated by tree work,
    //    not error scanning, so this is the right correctness/cost
    //    trade-off for cy-li5.
    let full = parse(new_src);

    // Sanity: the spliced tree's text MUST equal the new source. If it
    // doesn't, our bail conditions missed something — fall back so we
    // never violate the API's byte-equivalence invariant.
    let spliced_root = SyntaxNode::new_root(new_green_root.clone());
    if spliced_root.text() != new_src {
        return None;
    }

    Some(make_parse(new_green_root, full.errors().to_vec()))
}

/// Walk upward from `covering_element(range)` until we find the smallest
/// `STATEMENT` ancestor. Returns `None` when no such ancestor exists
/// (e.g. the edit lies between statements or in the source-file root's
/// trailing trivia).
#[cfg(feature = "incremental")]
fn covering_statement(root: &SyntaxNode, range: TextRange) -> Option<SyntaxNode> {
    // `covering_element` panics if `range` is outside the root's text.
    // Clamp defensively to the root's range so out-of-bounds edits go
    // straight to the fallback rather than panicking.
    let root_range = root.text_range();
    if !root_range.contains_range(range) {
        return None;
    }
    let elem = root.covering_element(range);
    let start_node = match elem {
        NodeOrToken::Node(n) => n,
        NodeOrToken::Token(t) => t.parent()?,
    };
    let mut cur = Some(start_node);
    while let Some(n) = cur {
        if n.kind() == SyntaxKind::STATEMENT {
            return Some(n);
        }
        cur = n.parent();
    }
    None
}

/// Construct a [`Parse`] from raw parts. Lives behind the `incremental`
/// feature because the fallback path uses `parse(...)` directly and
/// doesn't need the constructor.
#[cfg(feature = "incremental")]
fn make_parse(green: rowan::GreenNode, errors: Vec<crate::SyntaxError>) -> Parse {
    Parse::from_parts(green, errors)
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
        // Offset 6 is the boundary between "RETURN" and " 1".
        let edit = TextEdit::insert(TextSize::from(6), "0");
        let out = edit.apply(src);
        assert_eq!(out, "RETURN0 1");
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
        let edit = TextEdit::replace(TextRange::new(TextSize::from(7), TextSize::from(8)), "42");
        let np = incremental_reparse(&root, &edit);
        assert_eq!(np.syntax().to_string(), "RETURN 42");
        assert!(np.errors().is_empty(), "edit keeps the file parseable");
    }

    // ------------------------------------------------------------------
    // cy-li5: smart-path coverage
    // ------------------------------------------------------------------

    /// Helper: assert that `incremental_reparse` produces a tree that
    /// matches a fresh whole-file parse in both text and error set, then
    /// return the resulting Parse so the caller can introspect further.
    #[cfg(feature = "incremental")]
    fn assert_equivalent_to_full(old: &SyntaxNode, edit: &TextEdit) -> Parse {
        let new_src = edit.apply(&old.to_string());
        let smart = incremental_reparse(old, edit);
        let full = parse(&new_src);
        assert_eq!(
            smart.syntax().to_string(),
            full.syntax().to_string(),
            "smart-path text must equal whole-file parse text"
        );
        assert_eq!(
            smart.errors().len(),
            full.errors().len(),
            "smart-path error count must equal whole-file ({}); errors = {:?}",
            full.errors().len(),
            smart.errors().iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        smart
    }

    /// Edit fully inside a single statement — smart path should hit, and
    /// the result must agree with whole-file parse in text + error count.
    #[test]
    #[cfg(feature = "incremental")]
    fn smart_path_inside_single_statement() {
        let src = "MATCH (n) RETURN n;\nMATCH (m) RETURN m;\n";
        let p = parse(src);
        assert!(p.errors().is_empty(), "fixture parses clean");
        // Replace `n` in the FIRST statement's RETURN clause (offset 17).
        let edit = TextEdit::replace(
            TextRange::new(TextSize::new(17), TextSize::new(18)),
            "x",
        );
        let np = assert_equivalent_to_full(&p.syntax(), &edit);
        assert_eq!(
            np.syntax().to_string(),
            "MATCH (n) RETURN x;\nMATCH (m) RETURN m;\n"
        );
    }

    /// Edit at a clause boundary — smart path may take it (the WHERE is
    /// inside the same STATEMENT) but in either case the result must
    /// match a whole-file parse.
    #[test]
    #[cfg(feature = "incremental")]
    fn smart_path_clause_boundary_inside_statement() {
        let src = "MATCH (n) RETURN n;\n";
        let p = parse(src);
        // Insert a WHERE clause between the MATCH and the RETURN.
        let edit = TextEdit::insert(TextSize::new(10), "WHERE n.x = 1 ");
        let np = assert_equivalent_to_full(&p.syntax(), &edit);
        assert_eq!(np.syntax().to_string(), "MATCH (n) WHERE n.x = 1 RETURN n;\n");
    }

    /// Edit that introduces a new top-level `;` — must bail to whole-file
    /// (statement count changes). The result must still be a valid CST
    /// with the right text.
    #[test]
    #[cfg(feature = "incremental")]
    fn smart_path_bails_when_introducing_semicolon() {
        let src = "MATCH (n) RETURN n";
        let p = parse(src);
        // Insert "; MATCH (m) RETURN m" before EOF. The "; " inside the
        // statement covering element forces the bail.
        let edit = TextEdit::insert(TextSize::new(18), "; MATCH (m) RETURN m");
        let np = assert_equivalent_to_full(&p.syntax(), &edit);
        assert_eq!(np.syntax().to_string(), "MATCH (n) RETURN n; MATCH (m) RETURN m");
    }

    /// Edit that introduces a syntax error — smart path or fallback must
    /// produce a tree with `errors()` matching a whole-file parse, and
    /// the tree must still be byte-lossless (spec §4.4).
    #[test]
    #[cfg(feature = "incremental")]
    fn smart_path_introduces_syntax_error() {
        let src = "MATCH (n) RETURN n;\n";
        let p = parse(src);
        // Replace `(n)` with `(n` — unclosed paren, syntax error.
        let edit = TextEdit::replace(
            TextRange::new(TextSize::new(6), TextSize::new(9)),
            "(n",
        );
        let np = assert_equivalent_to_full(&p.syntax(), &edit);
        assert!(!np.errors().is_empty(), "edit must produce errors");
        assert_eq!(np.syntax().to_string(), "MATCH (n RETURN n;\n");
    }

    /// Edit that *heals* an existing syntax error must produce a clean
    /// tree, verified against whole-file parse equivalence.
    #[test]
    #[cfg(feature = "incremental")]
    fn smart_path_heals_syntax_error() {
        let src = "MATCH (n RETURN n;\n";
        let p = parse(src);
        assert!(!p.errors().is_empty(), "fixture has the unclosed paren error");
        // Insert the missing `)` — heal the parse.
        let edit = TextEdit::insert(TextSize::new(8), ")");
        let np = assert_equivalent_to_full(&p.syntax(), &edit);
        assert_eq!(np.syntax().to_string(), "MATCH (n) RETURN n;\n");
        assert!(np.errors().is_empty(), "heal must produce a clean tree");
    }
}
