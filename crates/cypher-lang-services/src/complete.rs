//! Shared completion engine. Spec §14.2 (LSP) / §15.2 (agent).
//!
//! Four completion contexts keyed off a cheap "what is the non-
//! whitespace character immediately before the cursor?" classifier:
//!
//! * `:` — label / rel-type completion from the loaded
//!   [`cypher_schema::SchemaProvider`].  When no schema is loaded, the
//!   engine returns an empty list (no guessing).
//! * `.` — property-key completion (cy-2pk).  Walks back to find the
//!   preceding identifier, resolves it against the HIR scope at the
//!   cursor, and — if the resolved binding has a known label — returns
//!   the schema's `PropertyDecl`s for that label as [`CompletionItem`]s.
//!   Missing schema, unbound identifier, or unknown label all return
//!   an empty list.
//! * `$` — parameter completion.  Scans the source for `$name` patterns
//!   already in use and returns them, plus a generic `param` placeholder
//!   when the source contains none.
//! * default — the curated keyword set.
//!
//! The public surface is deliberately *neutral*: no `lsp_types`.  The
//! LSP adapter maps [`CompletionItem`] → `lsp_types::CompletionItem`;
//! the agent adapter maps it → `serde_json::Value`.

use cypher_db::{Database, FileId};
use cypher_hir::{Clause, PatternElement, Statement, VarId};
use cypher_schema::{PropertyDecl, PropertyType};
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
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new completion kinds can
/// land without forcing a SemVer-major release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompletionItemKind {
    /// Cypher clause keyword (`MATCH`, `RETURN`, …).
    Keyword,
    /// Graph-node label (`Person`, `Movie`, …).
    Label,
    /// Graph relationship type (`KNOWS`, `ACTED_IN`, …).
    RelationshipType,
    /// Query parameter (`$param`).
    Parameter,
    /// Property key on a node / relationship (`name`, `age`, …).
    Property,
}

/// Neutral completion item.  Adapters translate `(label, kind, detail)`
/// to `lsp_types::CompletionItem` / `serde_json::Value` as appropriate.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new metadata fields (e.g. a
/// `documentation` body) can land without forcing a SemVer-major
/// release.  External crates access fields by name; construction is
/// internal to `cypher-lang-services`.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
        Some('.') => property_completions(db, &source, offset),
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
///    convert each to a [`CompletionItem`].
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

    // Lower + resolve the current buffer. Completion is per-keystroke
    // but parse is Salsa-cached at the CST layer; lowering a single
    // statement is cheap and mirrors the hover engine's strategy —
    // spec §14.4 p95 ≤ 25ms has comfortable headroom.
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
    // `PropertyDecl` is `#[non_exhaustive]` (cy-2i9.1); destructure with
    // `..` so future fields don't break this site.
    let PropertyDecl {
        name, ty, required, ..
    } = decl;
    let type_label = property_type_label(&ty);
    let detail = if required {
        format!("{type_label} (required)")
    } else {
        type_label.clone()
    };
    CompletionItem {
        label: name,
        kind: CompletionItemKind::Property,
        detail: Some(SmolStr::new(&detail)),
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
