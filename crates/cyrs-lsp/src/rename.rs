//! v1 `textDocument/rename` + `textDocument/prepareRename` engines
//! (spec §14.2, bead cy-rjh).
//!
//! Renames reuse [`crate::references::collect_sites`] so the
//! occurrence-walking logic has exactly one implementation.  The only
//! additional responsibilities here are:
//!
//! * gate the request against reserved token kinds in
//!   [`prepare_rename`]; and
//! * produce a [`lsp_types::WorkspaceEdit`] with one [`lsp_types::TextEdit`]
//!   per occurrence in [`compute`].
//!
//! # Conservative scoping
//!
//! Like the rest of the v1 LSP engines, rename matches by name.  A
//! file with two independently-scoped bindings called `n` will rename
//! both.  Scope-aware rename lands with the position-to-HirId
//! follow-up (cy-pn7 follow-up's block also unlocks this).
//!
//! # Validation
//!
//! `prepare_rename` rejects renames on reserved tokens (keywords,
//! punctuation, literals) by returning `None`.  The handler converts
//! `None` into a JSON-RPC error per the LSP spec so clients do not
//! offer the rename action at all.

use cyrs_db::{Database, FileId};
use cyrs_syntax::{LineIndex, SyntaxKind, SyntaxToken, TextRange, TextSize};
use lsp_types::{
    OneOf, OptionalVersionedTextDocumentIdentifier, Position, PrepareRenameResponse, Range,
    TextDocumentEdit, TextEdit, Uri, WorkspaceEdit,
};
use rowan::TokenAtOffset;

use crate::references;

/// `prepareRename` result: `Some(range)` when the token at the cursor is
/// a renameable identifier; `None` otherwise.  The caller converts
/// `None` into a JSON-RPC error so the client removes the rename UI.
pub(crate) fn prepare_rename(
    db: &Database,
    file_id: FileId,
    position: Position,
) -> Option<PrepareRenameResponse> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;

    let parse = db.parse_cst(file_id).ok()?;
    let token = pick_token(parse.parse().syntax().token_at_offset(offset))?;
    if !is_renameable(&token) {
        return None;
    }

    Some(PrepareRenameResponse::Range(text_range_to_lsp(
        &line_index,
        token.text_range(),
    )))
}

/// `rename` result: a [`WorkspaceEdit`] that replaces every occurrence
/// of the identifier under the cursor with `new_name`.  Returns `None`
/// when the cursor is on a non-renameable token OR when `new_name` is
/// not a valid identifier (empty, leading digit, contains punctuation).
pub(crate) fn compute(
    db: &Database,
    file_id: FileId,
    uri: &Uri,
    position: Position,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    if !is_valid_identifier(new_name) {
        return None;
    }
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let offset = position_to_offset(&line_index, position)?;

    let parse = db.parse_cst(file_id).ok()?;
    let token = pick_token(parse.parse().syntax().token_at_offset(offset))?;
    if !is_renameable(&token) {
        return None;
    }

    let old_name = token.text();
    // `collect_sites` lives on the references engine and is the single
    // source of truth for "every IDENT occurrence matching `name`".
    let sites = references::collect_sites(db, file_id, old_name);
    if sites.is_empty() {
        return None;
    }

    let edits: Vec<OneOf<TextEdit, _>> = sites
        .into_iter()
        .map(|range| {
            OneOf::Left(TextEdit {
                range: text_range_to_lsp(&line_index, range),
                new_text: new_name.to_owned(),
            })
        })
        .collect();

    Some(WorkspaceEdit {
        document_changes: Some(lsp_types::DocumentChanges::Edits(vec![TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: uri.clone(),
                // `None` = "version unknown"; the server doesn't track
                // client-side document versions separately under the
                // full-sync model.  Clients accept this.
                version: None,
            },
            edits,
        }])),
        ..WorkspaceEdit::default()
    })
}

/// A token is renameable iff it is a plain identifier.  Keywords,
/// punctuation, literals, and trivia are not — renaming them would
/// corrupt the program.
fn is_renameable(token: &SyntaxToken) -> bool {
    token.kind() == SyntaxKind::IDENT
}

/// Minimal Cypher identifier validator: ASCII `[A-Za-z_][A-Za-z0-9_]*`.
/// Does not permit backtick-quoted forms on the rename target — we
/// reject them here rather than silently accept an arbitrary new name
/// that the parser might re-reject later.
fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn pick_token(at: TokenAtOffset<SyntaxToken>) -> Option<SyntaxToken> {
    match at {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(left, right) => {
            if left.kind().is_trivia() {
                Some(right)
            } else {
                Some(left)
            }
        }
    }
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
