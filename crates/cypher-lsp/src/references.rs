//! v1 `textDocument/references` engine (spec §14.2, bead cy-vhe).
//!
//! Resolves the IDENT at the cursor to its text and returns every IDENT
//! token in the file's CST whose text equals it.  Matches the
//! name-based conservative model of the v1 hover (cy-pn7) and
//! definition (cy-ag6) engines — shadowing and scope barriers are not
//! honoured.  Position-aware HIR resolution is a follow-up bead.
//!
//! # Reuse
//!
//! [`collect_sites`] is deliberately `pub(crate)` and decoupled from the
//! cursor-plumbing so the rename engine (bead cy-rjh, blocked-by this
//! bead) can call it directly with a `(file_id, name)` it has already
//! resolved from `prepareRename`.  Do **not** fold it into the handler.
//!
//! # `includeDeclaration`
//!
//! When the client asks for references **without** the declaration we
//! drop the binding's `defined_at` range from the result.  Mechanics
//! match `definition::compute`: we look the token's text up against
//! the statement's bindings and, when we find one, exclude the exact
//! range it points at.  A token without a binding has no declaration
//! to exclude — everything stays.

use cypher_db::{Database, FileId};
use cypher_syntax::{LineIndex, SyntaxKind, SyntaxToken, TextRange, TextSize};
use lsp_types::{Location, Position, Range, Uri};
use rowan::TokenAtOffset;

/// Collect every IDENT token in `file_id`'s CST whose text equals
/// `name`, returning their byte-range spans.
///
/// Exposed to sibling modules (in particular the future rename engine,
/// bead cy-rjh) so they can skip the cursor-to-name plumbing and drive
/// the walk directly.  Walks the whole CST rather than a single
/// Statement: v1 is name-match and the v1 parser produces one root per
/// file, so "whole file" and "enclosing statement" coincide for realistic
/// open-editor inputs.
pub(crate) fn collect_sites(db: &Database, file_id: FileId, name: &str) -> Vec<TextRange> {
    let Ok(parse) = db.parse_cst(file_id) else {
        return Vec::new();
    };
    let root = parse.parse().syntax();
    let mut out = Vec::new();
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.as_token()
            && token.kind() == SyntaxKind::IDENT
            && token.text() == name
        {
            out.push(token.text_range());
        }
    }
    out
}

/// Compute a references response for the given cursor.
///
/// Returns `None` when the cursor is on a non-identifier token or the
/// file is not tracked.  When the cursor is on an IDENT that matches no
/// binding (e.g. a free-floating label in v1) we still return every
/// matching IDENT occurrence; `include_declaration` only gates the
/// binding's `defined_at` range.
pub(crate) fn compute(
    db: &Database,
    file_id: FileId,
    uri: &Uri,
    position: Position,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;

    let parse = db.parse_cst(file_id).ok()?;
    let token = pick_token(parse.parse().syntax().token_at_offset(offset))?;
    if token.kind() != SyntaxKind::IDENT {
        return None;
    }

    let name = token.text().to_owned();
    let mut ranges = collect_sites(db, file_id, &name);

    if !include_declaration {
        let stmt = cypher_hir::lower::lower_statement(&source);
        if let Some(binding) = stmt.bindings.values().find(|b| b.name.as_str() == name) {
            let defined_at = binding.defined_at;
            ranges.retain(|r| *r != defined_at);
        }
    }

    Some(
        ranges
            .into_iter()
            .map(|range| Location {
                uri: uri.clone(),
                range: text_range_to_lsp(&line_index, range),
            })
            .collect(),
    )
}

fn position_to_offset(line_index: &LineIndex, pos: Position) -> Option<TextSize> {
    let utf8 = line_index.from_utf16(cypher_syntax::WideLineCol {
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
            // Mirror `definition::pick_token`: at the end of an
            // identifier we want the identifier, not the following
            // whitespace.  Left-first unless it's trivia.
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
