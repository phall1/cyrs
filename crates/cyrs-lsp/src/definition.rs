//! v1 `textDocument/definition` engine (spec §14.2, bead cy-ag6).
//!
//! Replaces the `null` stub.  Resolves an identifier at the cursor to
//! the binding's `defined_at` `TextRange` in the same file and returns
//! a single `Location`.
//!
//! Matches the conservative model of the v1 hover engine (cy-pn7):
//! lookup is by name match against the statement's bindings, not by
//! position-aware HIR resolution.  Promoted to real position-to-HirId
//! resolution by the same follow-up bead that promotes hover.

use cyrs_db::{Database, FileId};
use cyrs_syntax::{LineIndex, SyntaxKind, SyntaxToken, TextRange, TextSize};
use lsp_types::{GotoDefinitionResponse, Location, Position, Range, Uri};
use rowan::TokenAtOffset;

/// Compute a goto-definition response for the given cursor.  Returns
/// `None` when the cursor is on a non-identifier token or the
/// identifier text does not match any binding in the statement.
pub(crate) fn compute(
    db: &Database,
    file_id: FileId,
    uri: &Uri,
    position: Position,
) -> Option<GotoDefinitionResponse> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;

    let parse = db.parse_cst(file_id).ok()?;
    let token = pick_token(parse.parse().syntax().token_at_offset(offset))?;
    if token.kind() != SyntaxKind::IDENT {
        return None;
    }

    // Lower from the already-parsed CST (cy-cfi): reuses `parse` above
    // and stays best-effort for buffers with syntax errors.
    let stmt = cyrs_hir::lower::lower_parse(parse.parse()).expect("lower_parse is infallible");
    let binding = stmt
        .bindings
        .values()
        .find(|b| b.name.as_str() == token.text())?;

    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range: text_range_to_lsp(&line_index, binding.defined_at),
    }))
}

fn position_to_offset(line_index: &LineIndex, pos: Position) -> Option<TextSize> {
    let utf8 = line_index.from_utf16(cyrs_syntax::WideLineCol {
        line: pos.line,
        col: pos.character,
    });
    let line_start = line_index.line_range(utf8.line)?.start();
    Some(line_start + TextSize::from(utf8.col))
}

fn pick_token(at: TokenAtOffset<SyntaxToken>) -> Option<SyntaxToken> {
    match at {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(left, right) => {
            // Prefer non-trivia.  When the cursor sits at the end of an
            // identifier we want to jump from THAT identifier, not
            // from the following whitespace, so prefer the left side.
            // Both-sides-trivia is a no-op for the caller anyway.
            if left.kind().is_trivia() {
                Some(right)
            } else {
                Some(left)
            }
        }
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
