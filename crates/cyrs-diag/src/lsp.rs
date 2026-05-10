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
