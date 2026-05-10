//! Integration tests for the `edit` module (cy-zv0, spec §11).
//!
//! Three scenarios mandated by the bead:
//!
//! 1. `edit_preserves_tree_shape` — an edit inside a string literal does
//!    not disturb the surrounding structure.
//! 2. `edit_across_clause_boundary` — an edit that changes clause shape
//!    triggers a correct reparse.
//! 3. `edit_noop` (a.k.a. rename ident `n` → `m`) — shape is preserved;
//!    the identifier token is renamed.
//!
//! The current `incremental_reparse` is a whole-file reparse fallback;
//! these tests also serve as the behavioural contract that the future
//! sub-tree-splice path must satisfy.

use cyrs_syntax::{
    SyntaxKind, SyntaxNode, TextEdit, TextRange, TextSize, incremental_reparse, parse,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect the pre-order sequence of non-trivia `SyntaxKind`s. Two trees
/// with the same sequence have "the same shape" modulo whitespace.
fn shape(node: &SyntaxNode) -> Vec<SyntaxKind> {
    node.preorder_with_tokens()
        .filter_map(|ev| match ev {
            rowan::WalkEvent::Enter(el) => {
                let k = match &el {
                    rowan::NodeOrToken::Node(n) => n.kind(),
                    rowan::NodeOrToken::Token(t) => t.kind(),
                };
                if k.is_trivia() { None } else { Some(k) }
            }
            rowan::WalkEvent::Leave(_) => None,
        })
        .collect()
}

/// Collect every non-trivia token's `(kind, text)` in source order.
fn tokens(node: &SyntaxNode) -> Vec<(SyntaxKind, String)> {
    node.preorder_with_tokens()
        .filter_map(|ev| match ev {
            rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(t)) if !t.kind().is_trivia() => {
                Some((t.kind(), t.text().to_string()))
            }
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Test 1 — edit inside a string literal preserves tree shape
// ---------------------------------------------------------------------------

#[test]
fn edit_preserves_tree_shape() {
    // A MATCH with a WHERE filter that contains a string literal.  We edit
    // a single character *inside* the literal.  The surrounding clauses
    // must keep their same structural layout.
    let src = r#"MATCH (n) WHERE n.name = "alice" RETURN n"#;
    let p = parse(src);
    assert!(p.errors().is_empty(), "fixture must parse clean");
    let before = shape(&p.syntax());

    // Find the "a" of "alice" — byte offset of the opening " plus 1.
    let quote_start = src.find('"').expect("fixture has a string literal");
    let a_offset = u32::try_from(quote_start + 1).expect("fixture fits in u32");
    let range = TextRange::new(TextSize::new(a_offset), TextSize::new(a_offset + 1));
    let edit = TextEdit::replace(range, "A"); // "alice" → "Alice"

    let np = incremental_reparse(&p.syntax(), &edit);
    assert!(np.errors().is_empty(), "edited tree must still parse clean");
    let after = shape(&np.syntax());
    assert_eq!(
        before, after,
        "structural shape must be unchanged by an edit confined to a string literal"
    );

    // And the resulting source must be what we expect.
    assert_eq!(
        np.syntax().to_string(),
        r#"MATCH (n) WHERE n.name = "Alice" RETURN n"#
    );
}

// ---------------------------------------------------------------------------
// Test 2 — edit across a clause boundary triggers correct reparse
// ---------------------------------------------------------------------------

#[test]
fn edit_across_clause_boundary() {
    // Start: single MATCH + RETURN.  The edit swaps `MATCH` for
    // `OPTIONAL MATCH`, which introduces a whole different clause kind.
    // The new tree must contain OPTIONAL_MATCH_CLAUSE and must *not*
    // contain a bare MATCH_CLAUSE.
    let src = "MATCH (n) RETURN n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "fixture must parse clean");
    assert!(
        shape(&p.syntax()).contains(&SyntaxKind::MATCH_CLAUSE),
        "sanity: fixture has MATCH_CLAUSE"
    );

    // Replace the "MATCH" keyword (5 bytes) with "OPTIONAL MATCH".
    let range = TextRange::new(TextSize::new(0), TextSize::new(5));
    let edit = TextEdit::replace(range, "OPTIONAL MATCH");

    let np = incremental_reparse(&p.syntax(), &edit);
    assert!(
        np.errors().is_empty(),
        "edited tree must parse clean; errors = {:?}",
        np.errors()
    );
    let after = shape(&np.syntax());
    assert!(
        after.contains(&SyntaxKind::OPTIONAL_MATCH_CLAUSE),
        "edited tree must contain OPTIONAL_MATCH_CLAUSE; shape = {after:?}"
    );
    assert!(
        !after.contains(&SyntaxKind::MATCH_CLAUSE),
        "edited tree must not still contain a plain MATCH_CLAUSE"
    );
    assert_eq!(np.syntax().to_string(), "OPTIONAL MATCH (n) RETURN n");
}

// ---------------------------------------------------------------------------
// Test 3 — rename ident preserves shape, changes only the identifier token
// ---------------------------------------------------------------------------

#[test]
fn edit_noop_rename_ident() {
    // Replace every `n` identifier with `m`.  For a single-occurrence
    // fixture this is three edits; we exercise only one edit here per the
    // bead, then rely on the caller to chain edits if needed.
    let src = "MATCH (n) RETURN n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "fixture must parse clean");
    let before_shape = shape(&p.syntax());
    let before_tokens = tokens(&p.syntax());

    // Replace the `n` in the pattern (offset 7).
    let range = TextRange::new(TextSize::new(7), TextSize::new(8));
    let edit1 = TextEdit::replace(range, "m");
    let p1 = incremental_reparse(&p.syntax(), &edit1);

    // And the `n` in RETURN (now at offset 17).
    let range2 = TextRange::new(TextSize::new(17), TextSize::new(18));
    let edit2 = TextEdit::replace(range2, "m");
    let p2 = incremental_reparse(&p1.syntax(), &edit2);

    assert!(p2.errors().is_empty(), "renamed tree must parse clean");
    assert_eq!(p2.syntax().to_string(), "MATCH (m) RETURN m");

    let after_shape = shape(&p2.syntax());
    let after_tokens = tokens(&p2.syntax());

    // Shape is identical.
    assert_eq!(
        before_shape, after_shape,
        "ident rename must not change tree shape"
    );

    // Every token's *kind* is the same; texts differ exactly at the
    // positions where IDENT("n") appeared.
    assert_eq!(before_tokens.len(), after_tokens.len());
    for (i, ((bk, bt), (ak, at))) in before_tokens.iter().zip(after_tokens.iter()).enumerate() {
        assert_eq!(bk, ak, "token {i} kind must be unchanged");
        if *bk == SyntaxKind::IDENT && bt == "n" {
            assert_eq!(at, "m", "token {i} renamed n → m");
        } else {
            assert_eq!(bt, at, "token {i} text unchanged");
        }
    }
}
