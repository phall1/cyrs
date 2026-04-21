//! v1 hover engine for `textDocument/hover` (spec §14.2, bead cy-pn7).
//!
//! Replaces the `null` stub with two layers of useful content:
//!
//! 1. **Keyword hover** — if the cursor lands on a clause keyword
//!    (`MATCH`, `WHERE`, `RETURN`, …) we return a one-line spec-style
//!    description.  This is the easiest layer to ship and it's the one
//!    that lights up the editor for every Cypher-curious user, since
//!    keywords dominate any real query.
//!
//! 2. Variable-name hover — if the cursor sits on an identifier whose
//!    text matches a binding in the current statement, we return a
//!    "variable" line naming the binding's kind plus its definition
//!    range.  Approximate: shadowed bindings are not disambiguated;
//!    real position-aware HIR resolution is the cy-pn7 follow-up.
//!
//! 3. **Else** — `Hover::None`.  Returning a structured no-content
//!    response keeps the LSP spec contract while staying deterministic.

use cypher_db::{Database, FileId};
use cypher_hir::Binding;
use cypher_syntax::{LineIndex, SyntaxKind, SyntaxToken, TextRange, TextSize};
use lsp_types::{Hover, HoverContents, MarkedString, Position, Range};
use rowan::TokenAtOffset;

/// Compute hover content for the given cursor position.
///
/// Returns `None` when the cursor is on whitespace, trivia, or a token
/// that does not have a useful explanation in the v1 engine.
pub(crate) fn compute(db: &Database, file_id: FileId, position: Position) -> Option<Hover> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;

    let parse = db.parse_cst(file_id).ok()?;
    let root = parse.parse().syntax();
    let token = pick_token(root.token_at_offset(offset))?;

    if let Some(content) = keyword_hover(&token) {
        return Some(make_hover(content, &line_index, token.text_range()));
    }

    if token.kind() == SyntaxKind::IDENT
        && let Some(content) = binding_hover(db, file_id, token.text())
    {
        return Some(make_hover(content, &line_index, token.text_range()));
    }

    None
}

/// Convert an LSP UTF-16 [`Position`] into a byte [`TextSize`] offset
/// against the source.  Returns `None` if the position lies past the end
/// of the file (LSP allows this; we treat it as "no hover").
fn position_to_offset(line_index: &LineIndex, pos: Position) -> Option<TextSize> {
    let utf8 = line_index.from_utf16(cypher_syntax::WideLineCol {
        line: pos.line,
        col: pos.character,
    });
    let line_start = line_index.line_range(utf8.line)?.start();
    Some(line_start + TextSize::from(utf8.col))
}

/// Choose the most informative token at the cursor.  When the cursor
/// sits exactly between two tokens (`TokenAtOffset::Between`) we prefer
/// the one to the left so hovering at the end of an identifier still
/// describes that identifier rather than the following whitespace.
fn pick_token(at: TokenAtOffset<SyntaxToken>) -> Option<SyntaxToken> {
    match at {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(left, right) => {
            // Prefer the non-trivia side so hovering between an
            // identifier and the following whitespace describes the
            // identifier.  Both sides trivia → return the left
            // arbitrarily; a hover-between-two-spaces is empty either
            // way.
            if right.kind().is_trivia() {
                Some(left)
            } else {
                Some(right)
            }
        }
    }
}

/// One-line description for each grammar keyword.  Returns `None` for
/// non-keyword tokens.  Phrasing is intentionally terse — full
/// references live in the spec, not in tooltips.
fn keyword_hover(token: &SyntaxToken) -> Option<String> {
    let blurb = match token.kind() {
        SyntaxKind::MATCH_KW => {
            "**MATCH** — pattern-match clause.  Binds variables that subsequent clauses can reference."
        }
        SyntaxKind::OPTIONAL_KW => {
            "**OPTIONAL** — modifier on `MATCH` that allows the pattern to fail; unmatched bindings are `NULL`."
        }
        SyntaxKind::WHERE_KW => {
            "**WHERE** — boolean filter on the current binding context.  Does not introduce a new scope."
        }
        SyntaxKind::WITH_KW => {
            "**WITH** — projection + scope barrier.  Only the named projections are visible to clauses that follow."
        }
        SyntaxKind::RETURN_KW => {
            "**RETURN** — terminal projection; defines the statement's output signature."
        }
        SyntaxKind::CREATE_KW => {
            "**CREATE** — pattern-creation clause.  Binds the created variables into the current scope."
        }
        SyntaxKind::MERGE_KW => {
            "**MERGE** — pattern-match-or-create.  Triggers `ON CREATE` / `ON MATCH` SET clauses appropriately."
        }
        SyntaxKind::UNWIND_KW => {
            "**UNWIND … AS v** — iterate a list expression, binding each element to `v`."
        }
        SyntaxKind::CALL_KW => {
            "**CALL** — invoke a procedure; combine with `YIELD` to bind result columns into scope."
        }
        SyntaxKind::SET_KW => "**SET** — write properties / labels on already-bound entities.",
        SyntaxKind::REMOVE_KW => {
            "**REMOVE** — drop properties / labels from already-bound entities."
        }
        SyntaxKind::DELETE_KW => {
            "**DELETE** — remove a node, relationship, or path.  Use `DETACH DELETE` for nodes with relationships."
        }
        SyntaxKind::DETACH_KW => {
            "**DETACH** — modifier on `DELETE` that also removes incident relationships."
        }
        SyntaxKind::AND_KW => "**AND** — short-circuiting boolean conjunction.",
        SyntaxKind::OR_KW => "**OR** — short-circuiting boolean disjunction.",
        SyntaxKind::NOT_KW => "**NOT** — boolean negation.",
        SyntaxKind::AS_KW => "**AS** — projection alias.",
        SyntaxKind::ORDER_KW => "**ORDER BY** — sort the row stream by one or more expressions.",
        SyntaxKind::BY_KW => {
            "**BY** — keyword used after `ORDER` (ORDER BY) and `GROUP` constructs."
        }
        SyntaxKind::ASC_KW => "**ASC** — ascending sort direction (default).",
        SyntaxKind::DESC_KW => "**DESC** — descending sort direction.",
        SyntaxKind::LIMIT_KW => "**LIMIT n** — cap the row stream at `n` rows.",
        SyntaxKind::SKIP_KW => "**SKIP n** — drop the first `n` rows.",
        SyntaxKind::DISTINCT_KW => {
            "**DISTINCT** — deduplicate the row stream by the projection tuple."
        }
        SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => "**boolean literal**.",
        SyntaxKind::NULL_KW => {
            "**NULL** — the absence-of-value marker.  All comparisons with `NULL` are `NULL`."
        }
        _ if token.kind().is_keyword() => return Some(format!("**keyword** `{}`", token.text())),
        _ => return None,
    };
    Some(blurb.to_owned())
}

/// Look the token text up against the bindings of the statement that
/// contains it.  When we find a match, render `**variable** `n`
/// (Kind)` plus a defined-at line; otherwise return `None`.
///
/// The lookup is approximate: shadowing and scope barriers are not
/// honoured, and we re-run AST → HIR lowering once per request rather
/// than caching it.  The follow-up bead promotes this to real
/// position-to-HirId resolution and a Salsa-tracked Statement query.
fn binding_hover(db: &Database, file_id: FileId, name: &str) -> Option<String> {
    let source = db.source_of(file_id).ok()?;
    let stmt = cypher_hir::lower::lower_statement(&source);
    let binding: &Binding = stmt.bindings.values().find(|b| b.name.as_str() == name)?;
    Some(format!(
        "**variable** `{name}` ({kind:?})\n\nDefined at byte range `{start}..{end}`.",
        kind = binding.kind,
        start = u32::from(binding.defined_at.start()),
        end = u32::from(binding.defined_at.end()),
    ))
}

/// Wrap a markdown string in the LSP `Hover` shape, attaching the token
/// range so the client can highlight the exact region the tooltip
/// applies to.
fn make_hover(content: String, line_index: &LineIndex, range: TextRange) -> Hover {
    Hover {
        contents: HoverContents::Scalar(MarkedString::String(content)),
        range: Some(text_range_to_lsp(line_index, range)),
    }
}

fn text_range_to_lsp(line_index: &LineIndex, range: TextRange) -> Range {
    Range {
        start: offset_to_position(line_index, range.start()),
        end: offset_to_position(line_index, range.end()),
    }
}

fn offset_to_position(line_index: &LineIndex, offset: TextSize) -> Position {
    let utf8 = line_index.line_col(offset);
    let utf16 = line_index.to_utf16(utf8);
    Position {
        line: utf16.line,
        character: utf16.col,
    }
}
