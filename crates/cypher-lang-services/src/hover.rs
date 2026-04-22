//! Shared hover engine. Spec §14.2 (LSP) / §15.2 (agent).
//!
//! Two layers of useful content off a byte-offset cursor:
//!
//! 1. **Keyword hover** — if the cursor lands on a clause keyword
//!    (`MATCH`, `WHERE`, `RETURN`, …) return a one-line description.
//! 2. **Variable-name hover** — if the cursor sits on an identifier
//!    whose text matches a binding in the current statement, return a
//!    "variable" line naming the binding's kind plus its definition
//!    range.
//!
//! Approximate: shadowed bindings are not disambiguated; scope barriers
//! are not honoured.  Real position-aware HIR resolution is the cy-pn7
//! follow-up.
//!
//! # Offset robustness
//!
//! rowan panics when `token_at_offset` is called past EOF; the engine
//! clamps defensively so callers (LSP clients, agent CLI) that produce
//! an out-of-range offset get an empty [`Hover`] rather than a crash.

use cypher_db::{Database, FileId};
use cypher_hir::Binding;
use cypher_syntax::{SyntaxKind, SyntaxToken, TextRange, TextSize};
use rowan::TokenAtOffset;

/// Neutral hover payload.  `markdown` empty and `range` collapsed on
/// the cursor means "no content" — adapters can fold that into
/// `Option<lsp_types::Hover>` / an agent `deferred: false` response.
#[derive(Debug, Clone)]
pub struct Hover {
    /// Markdown blurb shown in the tooltip.  Empty string means "no
    /// content" (the cursor landed on trivia / an unknown token).
    pub markdown: String,
    /// Byte-range the hover applies to.  Adapters translate to
    /// `lsp_types::Range` via a `LineIndex`.
    pub range: TextRange,
}

impl Hover {
    /// Empty hover collapsed at `offset` — "nothing to show".
    fn empty(offset: TextSize) -> Self {
        Self {
            markdown: String::new(),
            range: TextRange::empty(offset),
        }
    }
}

/// Compute hover content for the given cursor `offset`.  Always
/// returns [`Hover`] — a caller that wants "no content" maps
/// `markdown.is_empty()` → `None`.
#[must_use]
pub fn hover(db: &Database, file_id: FileId, offset: TextSize) -> Hover {
    let Ok(parse) = db.parse_cst(file_id) else {
        return Hover::empty(offset);
    };
    let root = parse.parse().syntax();
    // Rowan panics when `token_at_offset` is called with an offset
    // larger than the tree's end.  Clamp defensively so malformed
    // cursor positions (offset past EOF) produce an empty hover
    // response instead of a crash.
    let clamped = u32::min(u32::from(offset), u32::from(root.text_range().end()));
    let Some(token) = pick_token(root.token_at_offset(TextSize::from(clamped))) else {
        return Hover::empty(offset);
    };

    if let Some(content) = keyword_hover(&token) {
        return Hover {
            markdown: content,
            range: token.text_range(),
        };
    }

    if token.kind() == SyntaxKind::IDENT
        && let Some(content) = binding_hover(db, file_id, token.text())
    {
        return Hover {
            markdown: content,
            range: token.text_range(),
        };
    }

    Hover::empty(offset)
}

/// Choose the most informative token at the cursor.  When the cursor
/// sits exactly between two tokens (`TokenAtOffset::Between`) we prefer
/// the non-trivia side so hovering at the end of an identifier still
/// describes that identifier rather than following whitespace.
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

/// One-line description for each grammar keyword.  Returns `None` for
/// non-keyword tokens.  Phrasing is intentionally terse — full
/// references live in the spec, not in tooltips.
fn keyword_hover(token: &SyntaxToken) -> Option<String> {
    let blurb = match token.kind() {
        SyntaxKind::MATCH_KW => {
            "**MATCH** — pattern-match clause.  Binds variables that subsequent clauses can reference."
        }
        SyntaxKind::OPTIONAL_KW => {
            "**OPTIONAL** — modifier on `MATCH` that allows the pattern to fail; unmatched bindings are `NULL`."
        }
        SyntaxKind::WHERE_KW => {
            "**WHERE** — boolean filter on the current binding context.  Does not introduce a new scope."
        }
        SyntaxKind::WITH_KW => {
            "**WITH** — projection + scope barrier.  Only the named projections are visible to clauses that follow."
        }
        SyntaxKind::RETURN_KW => {
            "**RETURN** — terminal projection; defines the statement's output signature."
        }
        SyntaxKind::CREATE_KW => {
            "**CREATE** — pattern-creation clause.  Binds the created variables into the current scope."
        }
        SyntaxKind::MERGE_KW => {
            "**MERGE** — pattern-match-or-create.  Triggers `ON CREATE` / `ON MATCH` SET clauses appropriately."
        }
        SyntaxKind::UNWIND_KW => {
            "**UNWIND … AS v** — iterate a list expression, binding each element to `v`."
        }
        SyntaxKind::CALL_KW => {
            "**CALL** — invoke a procedure; combine with `YIELD` to bind result columns into scope."
        }
        SyntaxKind::SET_KW => "**SET** — write properties / labels on already-bound entities.",
        SyntaxKind::REMOVE_KW => {
            "**REMOVE** — drop properties / labels from already-bound entities."
        }
        SyntaxKind::DELETE_KW => {
            "**DELETE** — remove a node, relationship, or path.  Use `DETACH DELETE` for nodes with relationships."
        }
        SyntaxKind::DETACH_KW => {
            "**DETACH** — modifier on `DELETE` that also removes incident relationships."
        }
        SyntaxKind::AND_KW => "**AND** — short-circuiting boolean conjunction.",
        SyntaxKind::OR_KW => "**OR** — short-circuiting boolean disjunction.",
        SyntaxKind::NOT_KW => "**NOT** — boolean negation.",
        SyntaxKind::AS_KW => "**AS** — projection alias.",
        SyntaxKind::ORDER_KW => "**ORDER BY** — sort the row stream by one or more expressions.",
        SyntaxKind::BY_KW => {
            "**BY** — keyword used after `ORDER` (ORDER BY) and `GROUP` constructs."
        }
        SyntaxKind::ASC_KW => "**ASC** — ascending sort direction (default).",
        SyntaxKind::DESC_KW => "**DESC** — descending sort direction.",
        SyntaxKind::LIMIT_KW => "**LIMIT n** — cap the row stream at `n` rows.",
        SyntaxKind::SKIP_KW => "**SKIP n** — drop the first `n` rows.",
        SyntaxKind::DISTINCT_KW => {
            "**DISTINCT** — deduplicate the row stream by the projection tuple."
        }
        SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => "**boolean literal**.",
        SyntaxKind::NULL_KW => {
            "**NULL** — the absence-of-value marker.  All comparisons with `NULL` are `NULL`."
        }
        _ if token.kind().is_keyword() => return Some(format!("**keyword** `{}`", token.text())),
        _ => return None,
    };
    Some(blurb.to_owned())
}

/// Look the token text up against the bindings of the statement that
/// contains it.  When we find a match, render `**variable** `n`
/// (Kind)` plus a defined-at line; otherwise return `None`.
fn binding_hover(db: &Database, file_id: FileId, name: &str) -> Option<String> {
    let source = db.source_of(file_id).ok()?;
    let stmt = cypher_hir::lower::lower_statement(&source);
    let binding: &Binding = stmt.bindings.values().find(|b| b.name.as_str() == name)?;
    Some(format!(
        "**variable** `{name}` ({kind:?})\n\nDefined at byte range `{start}..{end}`.",
        kind = binding.kind,
        start = u32::from(binding.defined_at.start()),
        end = u32::from(binding.defined_at.end()),
    ))
}
