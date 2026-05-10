//! v1 `textDocument/inlayHint` engine (spec §14.2, bead cy-noi).
//!
//! Surfaces type / kind info on every `Binding` in the statement as
//! an inline `Type` hint.  v1 is deliberately approximate: we don't
//! have a position-aware HIR lookup yet (cy-pn7 follow-up), so the
//! hint text uses the `Binding::kind` (Node / Relationship / Path /
//! Value) rather than an inferred `Type` from sema.  The hint is
//! positioned immediately after the `defined_at` range.
//!
//! The hint is filtered to those whose `defined_at` intersects the
//! client-requested range so a partial hint request (typical in
//! large files) does not return hints for the whole document.

use cypher_db::{Database, FileId};
use cypher_syntax::{LineIndex, TextRange, TextRangeExt, TextSize};
use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Range};

/// Compute inlay hints for the requested range.  Returns an empty
/// vec when the file is not open or no bindings intersect the range.
pub(crate) fn compute(db: &Database, file_id: FileId, range: Range) -> Vec<InlayHint> {
    let Ok(source) = db.source_of(file_id) else {
        return Vec::new();
    };
    let line_index = LineIndex::new(&source);
    let filter = lsp_range_to_bytes(&line_index, range);

    // Re-lower rather than reach into a Salsa-cached Statement — the
    // workspace `Database` does not expose the Statement directly;
    // this is the same pattern the hover / executeCommand engines
    // use.  Cost is negligible for a typical open-file workload.
    let stmt = cypher_hir::lower::lower_statement(&source);
    let mut hints: Vec<InlayHint> = Vec::new();
    for binding in stmt.bindings.values() {
        if !filter.intersects(binding.defined_at) {
            continue;
        }
        let pos = offset_to_position(&line_index, binding.defined_at.end());
        hints.push(InlayHint {
            position: pos,
            label: InlayHintLabel::String(format!(": {:?}", binding.kind)),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: Some(true),
            padding_right: None,
            data: None,
        });
    }
    hints
}

fn lsp_range_to_bytes(line_index: &LineIndex, r: Range) -> TextRange {
    TextRange::new(
        position_to_offset(line_index, r.start).unwrap_or_else(|| TextSize::from(0)),
        position_to_offset(line_index, r.end).unwrap_or_else(|| TextSize::from(u32::MAX)),
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

fn offset_to_position(line_index: &LineIndex, offset: TextSize) -> Position {
    let utf8 = line_index.line_col(offset);
    let utf16 = line_index.to_utf16(utf8);
    Position {
        line: utf16.line,
        character: utf16.col,
    }
}
