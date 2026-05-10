//! v1 `textDocument/codeAction` engine (spec §14.2, bead cy-gc8).
//!
//! Surfaces `cypher_diag::FixIt` entries on diagnostics that intersect
//! the requested range as LSP `CodeAction` quick-fixes, each carrying
//! a `WorkspaceEdit` that the client can apply.
//!
//! # Why this bead matters
//!
//! The diagnostics pipeline already produces structured fix-its
//! (`Diagnostic::fixes`) — this bead is the wire between that engine
//! and the editor surface.  It also unblocks the agent `rewrite`
//! engine (cy-taz): once agents can discover fix-it ids via the LSP
//! code-action response, the rewrite op can reuse the same
//! `FixIt` lookup.
//!
//! # Scope
//!
//! v1 does not implement `codeAction/resolve`: every `CodeAction` is
//! fully populated at request time.  The response carries
//! `CodeActionOrCommand::CodeAction` with `kind =
//! CodeActionKind::QUICKFIX`, a `title` sourced from `FixIt.title`,
//! and a `WorkspaceEdit` whose `changes` map points at the current
//! file's URI.

use std::collections::HashMap;

use cypher_db::{Database, FileId};
use cypher_diag::Diagnostic;
use cypher_syntax::{LineIndex, TextRange, TextRangeExt, TextSize};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionResponse, Position, Range, TextEdit,
    Uri, WorkspaceEdit,
};

/// Compute the set of `CodeAction`s for the given range.
///
/// Returns an empty response (not `None`) when no fixes are available
/// so LSP clients display "no actions" instead of "server not
/// responding".  Returns `None` only when the file is unknown.
pub(crate) fn compute(
    db: &Database,
    file_id: FileId,
    uri: &Uri,
    requested: Range,
) -> Option<CodeActionResponse> {
    let source = db.source_of(file_id).ok()?;
    let line_index = LineIndex::new(&source);
    let requested_byte_range = lsp_range_to_bytes(&line_index, requested);

    let diagnostics = db.all_diagnostics(file_id).ok()?;
    let mut out: Vec<CodeActionOrCommand> = Vec::new();
    for diag in diagnostics.diagnostics() {
        if !requested_byte_range.intersects(diag.primary.range) {
            continue;
        }
        out.extend(fix_actions_for(diag, uri, &line_index));
    }
    Some(out)
}

/// Emit one `CodeAction` per fix-it attached to the diagnostic.
#[allow(clippy::mutable_key_type)]
// `lsp_types::Uri` uses interior mutability, but the hash it produces
// is stable for the parsed URI — clippy's warning is a false positive
// for this crate's use.  The LSP spec's `WorkspaceEdit::changes` is
// literally `HashMap<DocumentUri, Vec<TextEdit>>`, so we have no
// alternative key type.
fn fix_actions_for(
    diag: &Diagnostic,
    uri: &Uri,
    line_index: &LineIndex,
) -> Vec<CodeActionOrCommand> {
    diag.fixes
        .iter()
        .map(|fix| {
            let edits: Vec<TextEdit> = fix
                .edits
                .iter()
                .map(|e| TextEdit {
                    range: text_range_to_lsp(line_index, e.range),
                    new_text: e.replacement.to_string(),
                })
                .collect();

            let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
            changes.insert(uri.clone(), edits);

            let action = CodeAction {
                title: format!("{} — {}", diag.code, fix.title),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..WorkspaceEdit::default()
                }),
                command: None,
                is_preferred: None,
                disabled: None,
                data: None,
            };
            CodeActionOrCommand::CodeAction(action)
        })
        .collect()
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

#[cfg(test)]
#[allow(clippy::mutable_key_type)]
// `lsp_types::Uri` uses interior mutability (a RefCell cache of the
// parsed form), which clippy flags as a risky HashMap key even though
// hashing is stable.  Silence only inside tests — the runtime code
// uses the same `changes: HashMap<Uri, _>` shape the LSP spec requires.
mod tests {
    //! Direct unit coverage of the `FixIt` → `CodeAction` conversion.
    //!
    //! No diagnostic in the workspace currently populates
    //! `Diagnostic::fixes` (the shape exists in `cypher-diag` but no
    //! pass emits one yet).  The integration tests therefore only
    //! exercise the "no actions" path.  This unit test synthesises a
    //! `Diagnostic`-with-fix locally so the conversion itself is still
    //! covered.
    use super::*;
    use cypher_diag::{Applicability, DiagCode, Diagnostic, FixIt, TextEdit as DiagEdit};
    use smol_str::SmolStr;
    use std::str::FromStr;

    #[test]
    fn fixit_round_trips_to_code_action() {
        let line_index = LineIndex::new("MATCH (n) RETURN n");
        let uri: Uri = Uri::from_str("file:///tmp/t.cyp").expect("valid test URI");
        // `Diagnostic` is `#[non_exhaustive]` (cy-2i9.1) — build via the
        // `error` constructor + `with_fix` rather than a struct literal.
        let diag = Diagnostic::error(
            DiagCode::E0001,
            TextRange::new(TextSize::from(0), TextSize::from(5)),
            "example",
        )
        .with_fix(FixIt {
            id: SmolStr::new("cy-fix.uppercase"),
            title: SmolStr::new("uppercase keyword"),
            applicability: Applicability::MachineApplicable,
            edits: vec![DiagEdit {
                range: TextRange::new(TextSize::from(0), TextSize::from(5)),
                replacement: SmolStr::new("MATCH"),
            }],
        });

        let actions = fix_actions_for(&diag, &uri, &line_index);
        assert_eq!(actions.len(), 1);
        let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
            panic!("expected CodeAction, not a bare Command");
        };
        assert!(
            action.title.contains("uppercase keyword"),
            "title must carry the FixIt title: {:?}",
            action.title
        );
        assert_eq!(action.kind.as_ref(), Some(&CodeActionKind::QUICKFIX));
        let edit = action.edit.as_ref().expect("edit present");
        let changes = edit.changes.as_ref().expect("changes map present");
        let file_edits = changes.get(&uri).expect("edits for this URI");
        assert_eq!(file_edits.len(), 1);
        assert_eq!(file_edits[0].new_text, "MATCH");
    }
}
