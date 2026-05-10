//! `textDocument/hover` LSP adapter (spec §14.2).
//!
//! Thin wrapper around [`cyrs_lang_services::hover`].  Position ↔
//! byte-offset translation and `TextRange` → `Range` mapping happen
//! here; keyword blurbs and binding resolution live in the shared
//! crate.

use cyrs_db::{Database, FileId};
use cyrs_lang_services::hover as hover_shared;
use cyrs_syntax::{LineIndex, TextRange, TextSize};
use lsp_types::{Hover, HoverContents, MarkedString, Position, Range};

/// Compute hover content for the given cursor position.
///
/// Returns `None` when the cursor is on whitespace, trivia, or a token
/// that does not have a useful explanation in the v1 engine.  Internally
/// the shared engine returns an empty-markdown payload for "no content";
/// this adapter maps that to `None` so the LSP spec contract is
/// preserved.
pub(crate) fn compute(db: &Database, file_id: FileId, position: Position) -> Option<Hover> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;

    let payload = hover_shared(db, file_id, offset);
    if payload.markdown.is_empty() {
        return None;
    }

    Some(Hover {
        contents: HoverContents::Scalar(MarkedString::String(payload.markdown)),
        range: Some(text_range_to_lsp(&line_index, payload.range)),
    })
}

fn position_to_offset(line_index: &LineIndex, pos: Position) -> Option<TextSize> {
    let utf8 = line_index.from_utf16(cyrs_syntax::WideLineCol {
        line: pos.line,
        col: pos.character,
    });
    let line_start = line_index.line_range(utf8.line)?.start();
    Some(line_start + TextSize::from(utf8.col))
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
