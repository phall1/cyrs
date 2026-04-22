//! Shared completion engine. Spec §14.2 (LSP) / §15.2 (agent).
//!
//! Three completion contexts keyed off a cheap "what is the non-
//! whitespace character immediately before the cursor?" classifier:
//!
//! * `:` — label / rel-type completion from the loaded
//!   [`cypher_schema::SchemaProvider`].  When no schema is loaded, the
//!   engine returns an empty list (no guessing).
//! * `$` — parameter completion.  Scans the source for `$name` patterns
//!   already in use and returns them, plus a generic `param` placeholder
//!   when the source contains none.
//! * default — the curated keyword set.
//!
//! The public surface is deliberately *neutral*: no `lsp_types`.  The
//! LSP adapter maps [`CompletionItem`] → `lsp_types::CompletionItem`;
//! the agent adapter maps it → `serde_json::Value`.

use cypher_db::{Database, FileId};
use cypher_syntax::TextSize;
use smol_str::SmolStr;

/// Curated keyword set surfaced by the default completion context.
/// Ordered by frequency of use in real queries — adapters display in
/// this order when no other ranking signal is provided.
const KEYWORD_COMPLETIONS: &[&str] = &[
    "MATCH", "OPTIONAL", "WHERE", "WITH", "RETURN", "CREATE", "MERGE", "UNWIND", "CALL", "YIELD",
    "SET", "REMOVE", "DELETE", "DETACH", "AND", "OR", "NOT", "IN", "IS", "NULL", "TRUE", "FALSE",
    "AS", "ORDER", "BY", "ASC", "DESC", "LIMIT", "SKIP", "DISTINCT", "UNION", "ALL",
];

/// Neutral completion item kind.  Adapters translate to their wire
/// format (LSP `CompletionItemKind` / agent JSON string).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionItemKind {
    /// Cypher clause keyword (`MATCH`, `RETURN`, …).
    Keyword,
    /// Graph-node label (`Person`, `Movie`, …).
    Label,
    /// Graph relationship type (`KNOWS`, `ACTED_IN`, …).
    RelationshipType,
    /// Query parameter (`$param`).
    Parameter,
}

/// Neutral completion item.  Adapters translate `(label, kind, detail)`
/// to `lsp_types::CompletionItem` / `serde_json::Value` as appropriate.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// Human-visible label shown in the completion list.
    pub label: SmolStr,
    /// Item category driving icon / sort order.
    pub kind: CompletionItemKind,
    /// Optional right-aligned detail string (LSP `detail` field /
    /// agent `detail` JSON key).  Adapters can still show this or
    /// ignore it.
    pub detail: Option<SmolStr>,
}

/// Compute the completion items for `(db, file_id, offset)`.
///
/// Always returns `Vec` (possibly empty); callers should never treat
/// "no completions" as an error — LSP clients that see `None`/error
/// stop asking.
#[must_use]
pub fn complete(db: &Database, file_id: FileId, offset: TextSize) -> Vec<CompletionItem> {
    let Ok(source) = db.source_of(file_id) else {
        return Vec::new();
    };

    match trigger_char_before(&source, offset) {
        Some(':') => label_completions(db),
        Some('$') => parameter_completions(&source),
        _ => keyword_completions(),
    }
}

/// The non-whitespace character immediately preceding the cursor, if
/// any.  Cheap heuristic the trigger logic keys off.
fn trigger_char_before(source: &str, offset: TextSize) -> Option<char> {
    let upto: usize = u32::from(offset) as usize;
    let prefix = source.get(..upto)?;
    prefix.chars().rev().find(|c| !c.is_whitespace())
}

fn label_completions(db: &Database) -> Vec<CompletionItem> {
    let Some(schema) = db.schema() else {
        // No schema loaded — surface nothing rather than guess.
        return Vec::new();
    };
    let mut items: Vec<CompletionItem> = schema
        .labels()
        .into_iter()
        .map(|name| CompletionItem {
            label: name,
            kind: CompletionItemKind::Label,
            detail: Some(SmolStr::new_static("label")),
        })
        .collect();
    items.extend(
        schema
            .relationship_types()
            .into_iter()
            .map(|name| CompletionItem {
                label: name,
                kind: CompletionItemKind::RelationshipType,
                detail: Some(SmolStr::new_static("relationship type")),
            }),
    );
    // Stable sort so the wire output is deterministic.
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn parameter_completions(source: &str) -> Vec<CompletionItem> {
    use std::collections::BTreeSet;

    // Scan for `$ident` patterns already in the buffer so callers
    // discover parameters previously typed.
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
        // Always surface a generic placeholder so the caller sees that
        // `$` is a parameter trigger even on a fresh file.
        return vec![CompletionItem {
            label: SmolStr::new_static("param"),
            kind: CompletionItemKind::Parameter,
            detail: Some(SmolStr::new_static("placeholder")),
        }];
    }

    names
        .into_iter()
        .map(|name| CompletionItem {
            label: SmolStr::new(&name),
            kind: CompletionItemKind::Parameter,
            detail: None,
        })
        .collect()
}

fn keyword_completions() -> Vec<CompletionItem> {
    KEYWORD_COMPLETIONS
        .iter()
        .map(|kw| CompletionItem {
            label: SmolStr::new_static(kw),
            kind: CompletionItemKind::Keyword,
            detail: None,
        })
        .collect()
}
