//! Shared rewrite engine. Spec §15.2 (agent), also consumable by LSP
//! callers that want textual fixup (distinct from `textDocument/
//! codeAction`, which the LSP produces directly from
//! `Diagnostic::fixes` → `WorkspaceEdit`).
//!
//! Pulls every [`cypher_diag::Diagnostic::fixes`] whose `id` matches
//! one of the requested `fix_ids`, applies the edits in reverse byte-
//! offset order (so earlier edits don't shift later ranges), and
//! returns the rewritten text plus a record of applied edits and any
//! unknown fix ids the caller asked for.
//!
//! Non-applicable fixes tagged [`cypher_diag::Applicability::
//! HasPlaceholders`] still count as "matched" — callers asked for them
//! by id — but we refuse to apply their edits (they need human
//! editing first).

use std::collections::BTreeSet;

use cypher_db::{Database, FileId};
use cypher_diag::{Applicability, Diagnostic};
use cypher_syntax::TextRange;

/// One applied edit, as returned by [`rewrite`].  Mirrors the agent's
/// `{fix_id, range, replacement}` wire shape but stays neutral — the
/// agent adapter builds the JSON object; LSP callers could map this to
/// a `TextEdit` if they wanted.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new fields can land without
/// forcing a SemVer-major release.  Construction is internal to
/// `cypher-lang-services`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RewriteEdit {
    /// Fix id the edit came from.  Matches the id the caller asked for.
    pub fix_id: String,
    /// Byte range in the original (pre-rewrite) text the edit replaced.
    pub range: TextRange,
    /// Replacement text substituted into the range.
    pub replacement: String,
}

/// Outcome of applying a set of `fix_ids`.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new fields (e.g. a
/// `bytes_changed` counter) can land without forcing a SemVer-major
/// release.  Construction is internal to `cypher-lang-services`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RewritePayload {
    /// Every edit that was actually applied, in reverse byte-offset
    /// order (the order they were applied to the text).
    pub applied_edits: Vec<RewriteEdit>,
    /// The final source text after all edits were applied.
    pub resulting_text: String,
    /// Fix ids the caller requested that did NOT match any `FixIt` on
    /// any diagnostic — useful for distinguishing "fix applied" from
    /// "typo in fix id".
    pub unknown_fix_ids: Vec<String>,
}

/// Run `db.all_diagnostics`, collect every [`cypher_diag::FixIt`] whose
/// id matches one of the requested `fix_ids`, and apply the edits to
/// `input` in reverse offset order.  Returns the rewritten text plus a
/// record of applied edits and unknown ids.
#[must_use]
pub fn rewrite(db: &Database, file_id: FileId, input: &str, fix_ids: &[String]) -> RewritePayload {
    let Ok(diagnostics) = db.all_diagnostics(file_id) else {
        return RewritePayload {
            applied_edits: Vec::new(),
            resulting_text: input.to_owned(),
            unknown_fix_ids: fix_ids.to_vec(),
        };
    };

    // Collect every (Diagnostic, FixIt) whose id matches one of the
    // requested fix_ids.  Track which ids we satisfied so callers can
    // diff against `unknown_fix_ids`.
    let requested: BTreeSet<&str> = fix_ids.iter().map(String::as_str).collect();
    let mut matched: BTreeSet<&str> = BTreeSet::new();
    let mut edits: Vec<RewriteEdit> = Vec::new();
    for diag in diagnostics.diagnostics() {
        collect_fixes(diag, &requested, &mut matched, &mut edits);
    }

    // Apply edits in reverse byte-range order so earlier edits don't
    // shift later ranges.  This is the standard rustfmt / rustc
    // mechanical-fix strategy.
    edits.sort_by_key(|e| std::cmp::Reverse(u32::from(e.range.start())));
    let mut resulting = input.to_owned();
    let mut applied: Vec<RewriteEdit> = Vec::new();
    for e in edits {
        let start = u32::from(e.range.start()) as usize;
        let end = u32::from(e.range.end()) as usize;
        if end <= resulting.len() && start <= end {
            resulting.replace_range(start..end, &e.replacement);
            applied.push(e);
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

fn collect_fixes<'a>(
    diag: &'a Diagnostic,
    requested: &BTreeSet<&'a str>,
    matched: &mut BTreeSet<&'a str>,
    edits: &mut Vec<RewriteEdit>,
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
            edits.push(RewriteEdit {
                fix_id: id_str.to_owned(),
                range: edit.range,
                replacement: edit.replacement.to_string(),
            });
        }
    }
}
