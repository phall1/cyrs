//! Agent-side implementations of the `complete`, `hover`, and `rewrite`
//! ops (spec §15.2, beads cy-da0 / cy-o59 / cy-taz).
//!
//! The LSP crate ships equivalent engines for `textDocument/*`
//! requests; v1 duplicates the logic here rather than factoring out
//! a shared engine so that the agent binary stays small (no
//! lsp-server / lsp-types dependency).  The deduplication is tracked
//! as a follow-up bead that would lift the three functions into a
//! new neutral crate (likely `cypher-sema` or a fresh
//! `cypher-lang-services`).
//!
//! # Conservative name-match model
//!
//! Every engine in here uses the same position-to-token plumbing the
//! LSP uses: a UTF-8 byte offset + `SyntaxToken::token_at_offset` to
//! find the lexeme, then name-match against the `Statement`'s
//! bindings / `Diagnostic.fixes`.  Shadowing and scope barriers are
//! not honoured, matching the LSP v1 cut.

use std::collections::BTreeSet;

use cypher_db::{Database, FileId};
use cypher_diag::{Applicability, Diagnostic};
use cypher_syntax::{SyntaxKind, SyntaxToken, TextRange, TextSize};
use rowan::TokenAtOffset;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// complete — spec §15.2 (cy-da0)
// ---------------------------------------------------------------------------

/// Curated keyword set the agent emits outside structural contexts.
/// Ordered by frequency of use; clients display as given when no
/// other ranking signal is available.
const KEYWORD_COMPLETIONS: &[&str] = &[
    "MATCH", "OPTIONAL", "WHERE", "WITH", "RETURN", "CREATE", "MERGE", "UNWIND", "CALL", "YIELD",
    "SET", "REMOVE", "DELETE", "DETACH", "AND", "OR", "NOT", "IN", "IS", "NULL", "TRUE", "FALSE",
    "AS", "ORDER", "BY", "ASC", "DESC", "LIMIT", "SKIP", "DISTINCT", "UNION", "ALL",
];

/// Compute the completion items for `(text, offset)`.  Shape matches
/// the LSP's `CompletionItem` but keyed for the agent wire format.
pub(crate) fn complete(db: &Database, file_id: FileId, offset: u32) -> Vec<Value> {
    let source = db.source_of(file_id).unwrap_or_default();
    let trigger = trigger_char_before(&source, offset);
    match trigger {
        Some(':') => label_completions(db),
        Some('$') => parameter_completions(&source),
        _ => keyword_completions(),
    }
}

fn trigger_char_before(source: &str, offset: u32) -> Option<char> {
    let upto = offset as usize;
    let prefix = source.get(..upto)?;
    prefix.chars().rev().find(|c| !c.is_whitespace())
}

fn label_completions(db: &Database) -> Vec<Value> {
    let Some(schema) = db.schema() else {
        return Vec::new();
    };
    let mut items: Vec<Value> = schema
        .labels()
        .into_iter()
        .map(|name| json!({ "label": name.to_string(), "kind": "label" }))
        .collect();
    items.extend(
        schema
            .relationship_types()
            .into_iter()
            .map(|name| json!({ "label": name.to_string(), "kind": "relationship_type" })),
    );
    items.sort_by(|a, b| a["label"].as_str().cmp(&b["label"].as_str()));
    items
}

fn parameter_completions(source: &str) -> Vec<Value> {
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
        return vec![json!({ "label": "param", "kind": "parameter", "detail": "placeholder" })];
    }
    names
        .into_iter()
        .map(|name| json!({ "label": name, "kind": "parameter" }))
        .collect()
}

fn keyword_completions() -> Vec<Value> {
    KEYWORD_COMPLETIONS
        .iter()
        .map(|kw| json!({ "label": *kw, "kind": "keyword" }))
        .collect()
}

// ---------------------------------------------------------------------------
// hover — spec §15.2 (cy-o59)
// ---------------------------------------------------------------------------

/// Hover payload: `markdown` + the byte range the hover applies to
/// (`[start, end]`).  An empty markdown string means "no content" —
/// the agent wraps this in the existing `AgentResponse::Hover`
/// shape, so callers see consistent wire output regardless of
/// whether the engine found anything.
pub(crate) struct HoverPayload {
    pub markdown: String,
    pub range: [u32; 2],
}

pub(crate) fn hover(db: &Database, file_id: FileId, offset: u32) -> HoverPayload {
    let empty = HoverPayload {
        markdown: String::new(),
        range: [offset, offset],
    };
    let Ok(parse) = db.parse_cst(file_id) else {
        return empty;
    };
    let root = parse.parse().syntax();
    // Rowan panics when `token_at_offset` is called with an offset
    // larger than the tree's end.  Clamp defensively so malformed
    // cursor positions (offset past EOF from the agent client)
    // produce an empty-markdown response instead of a crash.
    let clamped = u32::min(offset, u32::from(root.text_range().end()));
    let Some(token) = pick_token(root.token_at_offset(TextSize::from(clamped))) else {
        return empty;
    };

    if let Some(md) = keyword_hover(&token) {
        return HoverPayload {
            markdown: md,
            range: text_range_to_pair(token.text_range()),
        };
    }

    if token.kind() == SyntaxKind::IDENT
        && let Ok(source) = db.source_of(file_id)
    {
        let stmt = cypher_hir::lower::lower_statement(&source);
        if let Some(binding) = stmt
            .bindings
            .values()
            .find(|b| b.name.as_str() == token.text())
        {
            let md = format!(
                "**variable** `{name}` ({kind:?})\n\nDefined at byte range `{start}..{end}`.",
                name = binding.name,
                kind = binding.kind,
                start = u32::from(binding.defined_at.start()),
                end = u32::from(binding.defined_at.end()),
            );
            return HoverPayload {
                markdown: md,
                range: text_range_to_pair(token.text_range()),
            };
        }
    }

    empty
}

/// Keyword blurbs mirrored from the LSP hover engine.  Kept in sync
/// by shape only; if the LSP copy grows a variant, add it here too.
fn keyword_hover(token: &SyntaxToken) -> Option<String> {
    let blurb = match token.kind() {
        SyntaxKind::MATCH_KW => {
            "**MATCH** — pattern-match clause; binds variables for later clauses."
        }
        SyntaxKind::OPTIONAL_KW => {
            "**OPTIONAL** — `MATCH` modifier; unmatched bindings become NULL."
        }
        SyntaxKind::WHERE_KW => "**WHERE** — boolean filter on the current binding context.",
        SyntaxKind::WITH_KW => "**WITH** — projection + scope barrier.",
        SyntaxKind::RETURN_KW => "**RETURN** — terminal projection; defines the output signature.",
        SyntaxKind::CREATE_KW => "**CREATE** — pattern-creation clause.",
        SyntaxKind::MERGE_KW => "**MERGE** — match-or-create with `ON CREATE` / `ON MATCH` hooks.",
        SyntaxKind::UNWIND_KW => "**UNWIND … AS v** — iterate a list, binding each element.",
        SyntaxKind::CALL_KW => {
            "**CALL** — invoke a procedure; combine with `YIELD` to bind results."
        }
        SyntaxKind::SET_KW => "**SET** — write properties / labels on bound entities.",
        SyntaxKind::REMOVE_KW => "**REMOVE** — drop properties / labels from bound entities.",
        SyntaxKind::DELETE_KW => "**DELETE** — remove a node, relationship, or path.",
        SyntaxKind::DETACH_KW => {
            "**DETACH** — `DELETE` modifier; also removes incident relationships."
        }
        SyntaxKind::AND_KW => "**AND** — boolean conjunction.",
        SyntaxKind::OR_KW => "**OR** — boolean disjunction.",
        SyntaxKind::NOT_KW => "**NOT** — boolean negation.",
        SyntaxKind::AS_KW => "**AS** — projection alias.",
        SyntaxKind::ORDER_KW => "**ORDER BY** — sort the row stream.",
        SyntaxKind::BY_KW => "**BY** — keyword used after `ORDER` / `GROUP`.",
        SyntaxKind::ASC_KW => "**ASC** — ascending sort direction.",
        SyntaxKind::DESC_KW => "**DESC** — descending sort direction.",
        SyntaxKind::LIMIT_KW => "**LIMIT n** — cap the row stream at `n` rows.",
        SyntaxKind::SKIP_KW => "**SKIP n** — drop the first `n` rows.",
        SyntaxKind::DISTINCT_KW => "**DISTINCT** — deduplicate the row stream by projection.",
        SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => "**boolean literal**.",
        SyntaxKind::NULL_KW => "**NULL** — absence-of-value; comparisons with `NULL` yield `NULL`.",
        _ if token.kind().is_keyword() => return Some(format!("**keyword** `{}`", token.text())),
        _ => return None,
    };
    Some(blurb.to_owned())
}

fn pick_token(at: TokenAtOffset<SyntaxToken>) -> Option<SyntaxToken> {
    match at {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(left, right) => {
            if right.kind().is_trivia() {
                Some(left)
            } else {
                Some(right)
            }
        }
    }
}

fn text_range_to_pair(range: TextRange) -> [u32; 2] {
    [u32::from(range.start()), u32::from(range.end())]
}

// ---------------------------------------------------------------------------
// rewrite — spec §15.2 (cy-taz)
// ---------------------------------------------------------------------------

/// Outcome of applying a set of `fix_ids`.  Mirrors the
/// `AgentResponse::Rewrite` shape.
pub(crate) struct RewritePayload {
    pub applied_edits: Vec<Value>,
    pub resulting_text: String,
    pub unknown_fix_ids: Vec<String>,
}

/// Run `db.all_diagnostics`, collect every `FixIt` whose `id` matches
/// one of the requested `fix_ids`, and apply the edits to the input
/// text in reverse offset order (so earlier edits don't invalidate
/// later ranges).
///
/// `applied_edits` carries the `{ range, replacement, fix_id }`
/// tuples the caller can replay.  `unknown_fix_ids` lists any
/// requested ids that did not match any `FixIt` so callers can
/// distinguish "fix applied" from "typo in fix id".
pub(crate) fn rewrite(
    db: &Database,
    file_id: FileId,
    input: &str,
    fix_ids: &[String],
) -> RewritePayload {
    let Ok(diagnostics) = db.all_diagnostics(file_id) else {
        return RewritePayload {
            applied_edits: Vec::new(),
            resulting_text: input.to_owned(),
            unknown_fix_ids: fix_ids.to_vec(),
        };
    };

    // Collect every (Diagnostic, FixIt) whose id matches one of the
    // requested fix_ids.  Track which ids we satisfied so callers
    // can diff against `unknown_fix_ids`.
    let requested: BTreeSet<&str> = fix_ids.iter().map(String::as_str).collect();
    let mut matched: BTreeSet<&str> = BTreeSet::new();
    let mut edits: Vec<MatchedEdit> = Vec::new();
    for diag in diagnostics.diagnostics() {
        collect_fixes(diag, &requested, &mut matched, &mut edits);
    }

    // Apply edits in reverse byte-range order so earlier edits don't
    // shift later ranges.  This is the standard rustfmt / rustc
    // mechanical-fix strategy.
    edits.sort_by_key(|e| std::cmp::Reverse(u32::from(e.range.start())));
    let mut resulting = input.to_owned();
    let mut applied: Vec<Value> = Vec::new();
    for e in edits {
        let start = u32::from(e.range.start()) as usize;
        let end = u32::from(e.range.end()) as usize;
        if end <= resulting.len() && start <= end {
            resulting.replace_range(start..end, &e.replacement);
            applied.push(json!({
                "fix_id": e.fix_id,
                "range": [u32::from(e.range.start()), u32::from(e.range.end())],
                "replacement": e.replacement,
            }));
        }
    }

    let unknown_fix_ids: Vec<String> = fix_ids
        .iter()
        .filter(|id| !matched.contains(id.as_str()))
        .cloned()
        .collect();

    RewritePayload {
        applied_edits: applied,
        resulting_text: resulting,
        unknown_fix_ids,
    }
}

struct MatchedEdit {
    fix_id: String,
    range: TextRange,
    replacement: String,
}

fn collect_fixes<'a>(
    diag: &'a Diagnostic,
    requested: &BTreeSet<&'a str>,
    matched: &mut BTreeSet<&'a str>,
    edits: &mut Vec<MatchedEdit>,
) {
    for fix in &diag.fixes {
        let id_str = fix.id.as_str();
        if !requested.contains(id_str) {
            continue;
        }
        matched.insert(id_str);
        // Non-applicable fixes still count as "matched" — callers
        // asked for them by id.  We only refuse to apply edits
        // tagged `HasPlaceholders` (they need human editing first).
        if matches!(fix.applicability, Applicability::HasPlaceholders) {
            continue;
        }
        for edit in &fix.edits {
            edits.push(MatchedEdit {
                fix_id: id_str.to_owned(),
                range: edit.range,
                replacement: edit.replacement.to_string(),
            });
        }
    }
}
