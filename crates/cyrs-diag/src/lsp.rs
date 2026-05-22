//! LSP diagnostic converter (spec §10.3).
//!
//! Converts [`Diagnostic`] values into [`lsp_types::Diagnostic`] for the
//! `cyrs-lsp` server. This module is gated behind the `lsp` feature;
//! consumers that don't run an LSP server don't pay the dep cost.
//!
//! UTF-16 position conversion happens here at the LSP boundary, per
//! spec §4.5. Callers provide a [`LineIndex`] that indexes the source
//! text; we convert byte offsets to UTF-16 [`Position`]s.

use core::str::FromStr;

use cyrs_syntax::{LineIndex, TextRange, TextSize};
use lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location,
    NumberOrString, Position, Range, Uri,
};

use crate::{Diagnostic, Related, Severity};

/// Convert one [`Diagnostic`] to the LSP wire type.
///
/// `uri` is the document URI the diagnostic belongs to; `line_index`
/// indexes the source text so byte ranges can become UTF-16 positions.
/// [`Related`] entries whose `file` is `None` are attached to `uri`;
/// entries with a `file` get that URI (best-effort — malformed URIs
/// fall back to `uri`).
#[must_use]
pub fn to_lsp(diag: &Diagnostic, uri: &Uri, line_index: &LineIndex) -> LspDiagnostic {
    LspDiagnostic {
        range: to_range(diag.primary.range, line_index),
        severity: Some(to_severity(diag.severity)),
        code: Some(NumberOrString::String(diag.code.to_string())),
        code_description: None,
        source: Some("cypher".into()),
        message: compose_message(diag),
        related_information: related_vec(&diag.related, uri, line_index),
        tags: None,
        data: None,
    }
}

/// Bulk helper.
#[must_use]
pub fn to_lsp_all(diags: &[Diagnostic], uri: &Uri, line_index: &LineIndex) -> Vec<LspDiagnostic> {
    diags.iter().map(|d| to_lsp(d, uri, line_index)).collect()
}

/// Convert one [`Diagnostic`] to the LSP wire type, downgrading
/// lint-range codes to `Information` severity.
///
/// The clippy-equivalent lint pack (`cyrs-sema`, codes `W6011`–`W6016`)
/// produces `Warning`-severity diagnostics for CLI / batch use, but the
/// LSP surfaces lints as `Information` so they read as advisory hints
/// in the editor rather than competing visually with real warnings
/// (bead cy-4yy). Non-lint diagnostics keep their natural severity.
#[must_use]
pub fn to_lsp_lint(diag: &Diagnostic, uri: &Uri, line_index: &LineIndex) -> LspDiagnostic {
    let mut lsp = to_lsp(diag, uri, line_index);
    if is_lint_code(diag.code) {
        lsp.severity = Some(DiagnosticSeverity::INFORMATION);
    }
    lsp
}

/// Bulk [`to_lsp_lint`] — every lint-range diagnostic is downgraded to
/// `Information`.
#[must_use]
pub fn to_lsp_all_lints(
    diags: &[Diagnostic],
    uri: &Uri,
    line_index: &LineIndex,
) -> Vec<LspDiagnostic> {
    diags
        .iter()
        .map(|d| to_lsp_lint(d, uri, line_index))
        .collect()
}

/// Is `code` one of the `cyrs-sema` lint codes (`W6011`–`W6016`,
/// bead cy-4yy)?
fn is_lint_code(code: crate::DiagCode) -> bool {
    use crate::DiagCode::{W6011, W6012, W6013, W6014, W6015, W6016};
    matches!(code, W6011 | W6012 | W6013 | W6014 | W6015 | W6016)
}

fn to_severity(s: Severity) -> DiagnosticSeverity {
    match s {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Note => DiagnosticSeverity::INFORMATION,
        Severity::Help => DiagnosticSeverity::HINT,
    }
}

fn to_range(range: TextRange, line_index: &LineIndex) -> Range {
    Range::new(
        to_position(range.start(), line_index),
        to_position(range.end(), line_index),
    )
}

fn to_position(offset: TextSize, line_index: &LineIndex) -> Position {
    let lc = line_index.line_col(offset);
    let wide = line_index.to_utf16(lc);
    Position::new(wide.line, wide.col)
}

/// The LSP diagnostic `message` is a single string. If the Diagnostic has
/// secondary labels or notes, inline them as trailing lines — mirrors
/// what rust-analyzer / rustc do for terse LSP UI.
fn compose_message(d: &Diagnostic) -> String {
    let mut out = d.message.to_string();
    for label in &d.labels {
        if !label.caption.is_empty() {
            out.push_str("\n  = ");
            out.push_str(label.caption.as_str());
        }
    }
    for note in &d.notes {
        out.push_str("\n  note: ");
        out.push_str(note.as_str());
    }
    out
}

fn related_vec(
    related: &[Related],
    uri: &Uri,
    line_index: &LineIndex,
) -> Option<Vec<DiagnosticRelatedInformation>> {
    if related.is_empty() {
        return None;
    }
    Some(
        related
            .iter()
            .map(|r| {
                let target_uri = r
                    .file
                    .as_ref()
                    .and_then(|f| Uri::from_str(f.as_str()).ok())
                    .unwrap_or_else(|| uri.clone());
                DiagnosticRelatedInformation {
                    location: Location::new(target_uri, to_range(r.range, line_index)),
                    message: r.message.to_string(),
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagCode, Diagnostic};

    fn uri() -> Uri {
        Uri::from_str("file:///q.cypher").unwrap()
    }

    fn at(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::new(start), TextSize::new(end))
    }

    #[test]
    fn to_lsp_lint_downgrades_lint_codes_to_information() {
        let idx = LineIndex::new("MATCH (n) RETURN n");
        // A lint-range diagnostic is `Warning` severity natively …
        let lint = Diagnostic::warning(DiagCode::W6011, at(7, 8), "unused");
        assert_eq!(lint.severity, Severity::Warning);
        // … but the LSP converter downgrades it to `Information`.
        let lsp = to_lsp_lint(&lint, &uri(), &idx);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn to_lsp_lint_leaves_non_lint_severity_untouched() {
        let idx = LineIndex::new("MATCH (n) RETURN n");
        // A real warning (not in the W6011..=W6016 lint block) keeps
        // `Warning`; an error keeps `Error`.
        let warn = Diagnostic::warning(DiagCode::W6001, at(0, 5), "dead with");
        assert_eq!(
            to_lsp_lint(&warn, &uri(), &idx).severity,
            Some(DiagnosticSeverity::WARNING),
        );
        let err = Diagnostic::error(DiagCode::E0001, at(0, 5), "syntax");
        assert_eq!(
            to_lsp_lint(&err, &uri(), &idx).severity,
            Some(DiagnosticSeverity::ERROR),
        );
    }

    #[test]
    fn to_lsp_all_lints_downgrades_only_the_lint_entries() {
        let idx = LineIndex::new("MATCH (n) RETURN n");
        let diags = [
            Diagnostic::error(DiagCode::E0001, at(0, 5), "syntax"),
            Diagnostic::warning(DiagCode::W6013, at(7, 8), "no label"),
        ];
        let out = to_lsp_all_lints(&diags, &uri(), &idx);
        assert_eq!(out[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(out[1].severity, Some(DiagnosticSeverity::INFORMATION));
    }
}
