//! `textDocument/completion` LSP adapter (spec §14.2).
//!
//! Thin wrapper around [`cypher_lang_services::complete`].  All the
//! real logic (trigger classification, schema lookup, parameter scan,
//! property-key resolution, keyword list) lives in the shared crate;
//! this module's only job is to map LSP [`Position`] → byte offset on
//! the way in and [`cypher_lang_services::CompletionItem`] →
//! [`lsp_types::CompletionItem`] on the way out.
//!
//! `completionItem/resolve` is a no-op for v1: every item is fully
//! populated at completion time, so resolve simply echoes the request.

use cypher_db::{Database, FileId};
use cypher_lang_services::{
    CompletionItem as NeutralItem, CompletionItemKind as NeutralKind, complete as complete_shared,
};
use cypher_syntax::{LineIndex, TextSize};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, Position,
};

/// Compute the completion response for the given cursor.
///
/// Always returns a `CompletionResponse::Array` (possibly empty); we
/// never return `None` because LSP clients treat that as "completion
/// not supported" and stop asking.
pub(crate) fn compute(
    db: &Database,
    file_id: FileId,
    params: &CompletionParams,
) -> CompletionResponse {
    let position = params.text_document_position.position;
    let Ok(source) = db.source_of(file_id) else {
        return CompletionResponse::Array(Vec::new());
    };
    let line_index = LineIndex::new(&source);
    let Some(offset) = position_to_offset(&line_index, position) else {
        return CompletionResponse::Array(Vec::new());
    };

    let items = complete_shared(db, file_id, offset)
        .into_iter()
        .map(to_lsp)
        .collect();
    CompletionResponse::Array(items)
}

/// `completionItem/resolve` echo for v1.  Every item is fully populated
/// at completion time, so resolve is a no-op.  Declared so the server
/// can advertise `resolveProvider: true` and clients that batch-resolve
/// don't break.
pub(crate) fn resolve(item: CompletionItem) -> CompletionItem {
    item
}

/// Translate a neutral [`NeutralItem`] into the LSP wire shape.
fn to_lsp(item: NeutralItem) -> CompletionItem {
    let (kind, fallback_detail) = match item.kind {
        NeutralKind::Keyword => (Some(CompletionItemKind::KEYWORD), None),
        NeutralKind::Label => (Some(CompletionItemKind::CLASS), Some("label")),
        NeutralKind::RelationshipType => (
            Some(CompletionItemKind::INTERFACE),
            Some("relationship type"),
        ),
        NeutralKind::Parameter => (Some(CompletionItemKind::VARIABLE), None),
        NeutralKind::Property => (Some(CompletionItemKind::FIELD), None),
        // `CompletionItemKind` is `#[non_exhaustive]` (cy-2i9.1).
        _ => (None, None),
    };

    // Parameter items without explicit detail get a synthesised
    // "query parameter ${name}" string to match the v1 LSP wire
    // shape; placeholder parameters carry "query parameter
    // (placeholder)" to preserve the original text.
    let detail = match (item.kind, item.detail.as_deref()) {
        (NeutralKind::Parameter, None) => Some(format!("query parameter ${}", item.label)),
        (NeutralKind::Parameter, Some("placeholder")) => {
            Some("query parameter (placeholder)".to_string())
        }
        (_, Some(d)) => Some(d.to_string()),
        (_, None) => fallback_detail.map(String::from),
    };

    CompletionItem {
        label: item.label.to_string(),
        kind,
        detail,
        ..CompletionItem::default()
    }
}

fn position_to_offset(line_index: &LineIndex, pos: Position) -> Option<TextSize> {
    let utf8 = line_index.from_utf16(cypher_syntax::WideLineCol {
        line: pos.line,
        col: pos.character,
    });
    let line_start = line_index.line_range(utf8.line)?.start();
    Some(line_start + TextSize::from(utf8.col))
}
