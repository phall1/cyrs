//! v1 `textDocument/completion` engine (spec §14.2, beads cy-zod + cy-2pk).
//!
//! Implements four completion contexts off the LSP trigger-character
//! contract plus a cheap "what's the character immediately before the
//! cursor?" fallback for user-invoked (Ctrl-Space) requests:
//!
//! * `:` — label / rel-type completion.  Returns every label and
//!   relationship type from the loaded `SchemaProvider`.  When no
//!   schema is loaded we return an empty list (no spurious
//!   suggestions).
//! * `.` — property-key completion (cy-2pk).  Walks back to find the
//!   preceding identifier, resolves it against the HIR scope at the
//!   cursor, and — if the resolved binding has a known label — returns
//!   the schema's `PropertyDecl`s for that label as `CompletionItem`s.
//!   Missing schema, unbound identifier, or unknown label all return
//!   an empty list.
//! * `$` — parameter completion.  Returns every `$name` already used
//!   in the buffer plus a generic `$param` placeholder.
//! * default — keyword completion.  Returns the curated list of
//!   Cypher clause keywords.
//!
//! `completionItem/resolve` is a no-op for v1: every item is fully
//! populated at completion time, so resolve simply echoes the request.

use cypher_db::{Database, FileId};
use cypher_hir::{Clause, PatternElement, Statement, VarId};
use cypher_schema::{PropertyDecl, PropertyType};
use cypher_syntax::{LineIndex, TextSize};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    CompletionTriggerKind, Position,
};
use smol_str::SmolStr;

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

    // Prefer the LSP-supplied trigger context when present; fall back
    // to the cheap character-before-cursor heuristic for user-invoked
    // (Ctrl-Space) requests, where `context.trigger_kind` is `INVOKED`.
    let trigger = trigger_from_context(params).or_else(|| trigger_char_before(&source, offset));

    let items = match trigger {
        Some(':') => label_completions(db),
        Some('.') => property_completions(db, &source, offset),
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

/// Extract the trigger character the client reported via
/// `CompletionParams.context`. Returns `None` for user-invoked requests
/// (Ctrl-Space) or when the trigger kind is anything other than
/// `TriggerCharacter`.
fn trigger_from_context(params: &CompletionParams) -> Option<char> {
    let ctx = params.context.as_ref()?;
    if ctx.trigger_kind != CompletionTriggerKind::TRIGGER_CHARACTER {
        return None;
    }
    ctx.trigger_character.as_ref()?.chars().next()
}

/// The non-whitespace character immediately preceding the cursor, if
/// any.  Fallback for Ctrl-Space invocations where no trigger character
/// is reported; keeps the heuristic cheap and deterministic.
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

/// Property-key completion after `.` (spec §14.2, bead cy-2pk).
///
/// Steps:
///
/// 1. Walk back from the cursor to the `.` token, then past it to find
///    the identifier whose properties we want to complete.
/// 2. Lower the current source to HIR + run the resolver so we have a
///    `ScopeGraph` and a `Statement` with pattern labels recorded.
/// 3. Map the cursor offset to a `ScopeId` via
///    `ScopeGraph::scope_at_offset`.
/// 4. Resolve the identifier name in that scope to a `VarId`.
/// 5. Look up the binding's label in the statement's pattern elements.
/// 6. Ask the `SchemaProvider` for that label's `PropertyDecl`s and
///    convert each to a `CompletionItem`.
///
/// Any "I don't know" along the chain (no schema, unbound name,
/// unknown label) returns an empty list rather than a guess — mirrors
/// the label-completion contract.
fn property_completions(db: &Database, source: &str, offset: TextSize) -> Vec<CompletionItem> {
    let Some(schema) = db.schema() else {
        return Vec::new();
    };
    let Some(ident) = preceding_identifier(source, offset) else {
        return Vec::new();
    };

    // Lower + resolve the current buffer. The LSP is not on a hot path
    // here (completion fires per-keystroke but the parse is cached by
    // Salsa at the CST layer; lowering a single statement is cheap in
    // human-speed terms and matches the existing hover engine's
    // strategy — spec §14.4 p95 ≤ 25ms is comfortable headroom).
    let stmt = cypher_hir::lower::lower_statement(source);
    let mut sink = cypher_diag::DiagnosticsSink::new();
    let resolved = cypher_sema::resolve::resolve(&stmt, false, &mut sink);

    // Which scope does the cursor sit in? If no scope covers the
    // offset, fall back to the deepest scope we do have — the user
    // might be typing past the end of a valid prefix (e.g. "MATCH
    // (n:Person) WHERE n.<cursor>" where the recovering parser closed
    // the statement early).
    let scope = resolved.scope_graph.scope_at_offset(offset).or_else(|| {
        (0..u32::try_from(resolved.scope_graph.len()).unwrap_or(0))
            .next_back()
            .map(cypher_hir::ScopeId)
    });
    let Some(scope) = scope else {
        return Vec::new();
    };

    // Resolve the identifier to a VarId in that scope.
    let Some(var_id) = resolved.scope_graph.resolve_at(scope, &ident) else {
        return Vec::new();
    };

    // Find the label declared at the binding site. v1 only models
    // labels on node patterns — relationship types could be added
    // here in a follow-up via `PatternElement::Rel.types`.
    let Some(label) = node_label_for(&stmt, var_id) else {
        return Vec::new();
    };

    // Consult the schema.
    let Some(props) = schema.node_properties(&label) else {
        return Vec::new();
    };

    let mut items: Vec<CompletionItem> =
        props.into_iter().map(property_decl_to_completion).collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

/// Starting at `offset`, step backwards over a `.` (optionally with
/// whitespace on either side) and harvest the trailing identifier text.
/// Returns `None` when the preceding character is not a `.` or when no
/// identifier precedes it (e.g. `.` after a literal).
fn preceding_identifier(source: &str, offset: TextSize) -> Option<String> {
    let upto: usize = u32::from(offset) as usize;
    let prefix = source.get(..upto)?;
    // Walk back skipping whitespace between cursor and `.`.
    let bytes = prefix.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'.' {
        return None;
    }
    i -= 1;
    // Skip whitespace between `.` and the identifier.
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    // Collect the identifier: [A-Za-z_][A-Za-z0-9_]*.
    let end = i;
    while i > 0 {
        let b = bytes[i - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            i -= 1;
        } else {
            break;
        }
    }
    if i == end {
        return None;
    }
    // The first char must be a letter or underscore, not a digit.
    if !matches!(bytes[i], b'a'..=b'z' | b'A'..=b'Z' | b'_') {
        return None;
    }
    std::str::from_utf8(&bytes[i..end]).ok().map(String::from)
}

/// Walk the statement's pattern elements and return the first label
/// attached to a node binding of `var_id`. Returns `None` if the
/// variable is bound only as a relationship / path / value variable,
/// or if its node pattern is unlabelled.
fn node_label_for(stmt: &Statement, var_id: VarId) -> Option<SmolStr> {
    for clause in &stmt.clauses {
        let (Clause::Match { pattern, .. }
        | Clause::Create { pattern, .. }
        | Clause::Merge { pattern, .. }) = clause
        else {
            continue;
        };
        for part in &pattern.parts {
            for elem in &part.elements {
                if let PatternElement::Node { bind, labels, .. } = elem
                    && *bind == Some(var_id)
                    && let Some(first) = labels.first()
                {
                    return Some(first.clone());
                }
            }
        }
    }
    None
}

fn property_decl_to_completion(decl: PropertyDecl) -> CompletionItem {
    let PropertyDecl { name, ty, required } = decl;
    let type_label = property_type_label(&ty);
    let detail = if required {
        format!("{type_label} (required)")
    } else {
        type_label.clone()
    };
    CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::FIELD),
        detail: Some(detail),
        ..CompletionItem::default()
    }
}

fn property_type_label(ty: &PropertyType) -> String {
    match ty {
        PropertyType::String => "String".into(),
        PropertyType::Int => "Int".into(),
        PropertyType::Float => "Float".into(),
        PropertyType::Bool => "Bool".into(),
        PropertyType::Date => "Date".into(),
        PropertyType::Datetime => "Datetime".into(),
        PropertyType::List(inner) => format!("List<{}>", property_type_label(inner)),
        PropertyType::Enum(name, _) => format!("Enum({name})"),
        PropertyType::Opaque(name) => format!("Opaque({name})"),
        PropertyType::Any => "Any".into(),
    }
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
