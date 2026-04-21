//! v1 `textDocument/completion` engine (spec §14.2, bead cy-zod).
//!
//! Implements three completion contexts off a cheap "what's the
//! character immediately before the cursor?" classifier:
//!
//! * `:` — label / rel-type completion.  Returns every label and
//!   relationship type from the loaded `SchemaProvider`.  When no
//!   schema is loaded we return an empty list (no spurious
//!   suggestions).
//! * `$` — parameter completion.  Returns every `$name` already used
//!   in the buffer plus a generic `$param` placeholder.
//! * default — keyword completion.  Returns the curated list of
//!   Cypher clause keywords.
//!
//! Property-key completion (`.` after a bound variable) is deferred to
//! a follow-up bead — it requires position-aware HIR resolution to
//! find the variable's label, which the v1 hover engine also stubs out
//! (cy-pn7 follow-up).
//!
//! `completionItem/resolve` is a no-op for v1: every item is fully
//! populated at completion time, so resolve simply echoes the request.

use cypher_db::{Database, FileId};
use cypher_syntax::{LineIndex, TextSize};
use lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse, Position};

/// Curated keyword set surfaced by the default completion context.
/// Ordered by frequency of use in real queries — clients display in
/// this order when no other ranking signal is provided.
const KEYWORD_COMPLETIONS: &[&str] = &[
    "MATCH", "OPTIONAL", "WHERE", "WITH", "RETURN", "CREATE", "MERGE", "UNWIND", "CALL", "YIELD",
    "SET", "REMOVE", "DELETE", "DETACH", "AND", "OR", "NOT", "IN", "IS", "NULL", "TRUE", "FALSE",
    "AS", "ORDER", "BY", "ASC", "DESC", "LIMIT", "SKIP", "DISTINCT", "UNION", "ALL",
];

/// Compute the completion response for the given cursor.
///
/// Always returns a `CompletionResponse::Array` (possibly empty); we
/// never return `None` because LSP clients treat that as "completion
/// not supported" and stop asking.
pub(crate) fn compute(db: &Database, file_id: FileId, position: Position) -> CompletionResponse {
    let Ok(source) = db.source_of(file_id) else {
        return CompletionResponse::Array(Vec::new());
    };
    let line_index = LineIndex::new(&source);
    let Some(offset) = position_to_offset(&line_index, position) else {
        return CompletionResponse::Array(Vec::new());
    };

    let trigger = trigger_char_before(&source, offset);
    let items = match trigger {
        Some(':') => label_completions(db),
        Some('$') => parameter_completions(&source),
        _ => keyword_completions(),
    };
    CompletionResponse::Array(items)
}

/// `completionItem/resolve` echo for v1.  Every item is fully populated
/// at completion time, so resolve is a no-op.  Declared so the server
/// can advertise `resolveProvider: true` and clients that batch-resolve
/// don't break.
pub(crate) fn resolve(item: CompletionItem) -> CompletionItem {
    item
}

/// The non-whitespace character immediately preceding the cursor, if
/// any.  This is the cheap heuristic the trigger logic keys off.
fn trigger_char_before(source: &str, offset: TextSize) -> Option<char> {
    let upto: usize = u32::from(offset) as usize;
    let prefix = source.get(..upto)?;
    let mut iter = prefix.chars().rev().skip_while(|c| c.is_whitespace());
    iter.next()
}

fn label_completions(db: &Database) -> Vec<CompletionItem> {
    let Some(schema) = db.schema() else {
        // No schema loaded — surface nothing rather than guess.  Spec
        // §14.3 (schema init-options adapter) is the user-visible knob
        // for loading one; a missing schema is intentional state, not
        // an error.
        return Vec::new();
    };
    let mut items: Vec<CompletionItem> = schema
        .labels()
        .into_iter()
        .map(|name| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("label".to_string()),
            ..CompletionItem::default()
        })
        .collect();
    items.extend(
        schema
            .relationship_types()
            .into_iter()
            .map(|name| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::INTERFACE),
                detail: Some("relationship type".to_string()),
                ..CompletionItem::default()
            }),
    );
    // Stable sort so the wire output is deterministic.
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn parameter_completions(source: &str) -> Vec<CompletionItem> {
    use std::collections::BTreeSet;

    // Scan for `$ident` patterns already in the buffer so the user
    // discovers parameters they've previously typed.
    let mut names: BTreeSet<String> = BTreeSet::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if j > i + 1
                && let Ok(s) = std::str::from_utf8(&bytes[i + 1..j])
            {
                names.insert(s.to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }

    if names.is_empty() {
        // Always surface a generic placeholder so the user sees that
        // `$` is a parameter trigger even on a fresh file.
        return vec![CompletionItem {
            label: "param".to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some("query parameter (placeholder)".to_string()),
            ..CompletionItem::default()
        }];
    }

    names
        .into_iter()
        .map(|name| CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(format!("query parameter ${name}")),
            ..CompletionItem::default()
        })
        .collect()
}

fn keyword_completions() -> Vec<CompletionItem> {
    KEYWORD_COMPLETIONS
        .iter()
        .map(|kw| CompletionItem {
            label: (*kw).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..CompletionItem::default()
        })
        .collect()
}

fn position_to_offset(line_index: &LineIndex, pos: Position) -> Option<TextSize> {
    let utf8 = line_index.from_utf16(cypher_syntax::WideLineCol {
        line: pos.line,
        col: pos.character,
    });
    let line_start = line_index.line_range(utf8.line)?.start();
    Some(line_start + TextSize::from(utf8.col))
}
