//! `cyrs-diag` — diagnostics for the Cypher front-end (spec 0001 §10).
//!
//! Every pass in the pipeline emits into a shared [`Diagnostic`] shape
//! with stable [`DiagCode`] identifiers. Rendering backends (plain text,
//! JSON, LSP) live here so no downstream crate reinvents them.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cyrs-diag/0.0.1")]

pub mod codes;
pub mod json;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod render;

pub use codes::DiagCode;
pub use json::{to_json, to_json_string, to_ndjson};
#[cfg(feature = "lsp")]
pub use lsp::{to_lsp, to_lsp_all, to_lsp_all_lints, to_lsp_lint};
pub use render::{render_text, render_text_stderr, render_text_string};

use cyrs_syntax::TextRange;
use smol_str::SmolStr;

/// A single diagnostic. Spec §10.1.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so fields can be added (e.g. a
/// `documentation_url`) without forcing a SemVer-major release.
/// Construct via [`Diagnostic::error`] / [`Diagnostic::warning`] and the
/// builder methods; external crates must not use struct literals.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Diagnostic {
    /// Stable numeric identifier (spec §10.2, e.g. `E0001`).
    pub code: DiagCode,
    /// Error / warning / note / help — drives rendering + exit codes.
    pub severity: Severity,
    /// Human-readable message, rustc-style (lower-case initial, no trailing period).
    pub message: SmolStr,
    /// The primary source span the diagnostic points at.
    pub primary: Label,
    /// Additional labels adding context (e.g. the site of a shadowed binding).
    pub labels: Vec<Label>,
    /// Trailing `note: …` lines rendered after the primary message.
    pub notes: Vec<SmolStr>,
    /// Cross-references to other spans (possibly in other files).
    pub related: Vec<Related>,
    /// Suggested edits that fix the diagnostic.  Each fix is atomic.
    pub fixes: Vec<FixIt>,
}

/// Diagnostic severity (spec §10.3).  `Error` failures set exit code 1
/// in `cypher check`; the others are informational.
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new severities can land
/// without forcing a SemVer-major release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

/// A captioned source span attached to a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Label {
    /// Byte range within the source file.
    pub range: TextRange,
    /// Short caption rendered next to the underline (e.g. "here").
    pub caption: SmolStr,
}

/// A cross-reference to another span, possibly in another file.
///
/// `file` is a no-op in v1 (§10.6) but the field is carried so future
/// multi-file workflows don't require a breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Related {
    /// Byte range in the referenced file.
    pub range: TextRange,
    /// Caption rendered beneath the referenced span.
    pub message: SmolStr,
    /// Referenced filename.  `None` means "same file as the primary
    /// label"; v1 is single-file so this is always `None` in practice
    /// (spec §10.6).
    pub file: Option<SmolStr>,
}

/// A suggested edit. Multiple `edits` are applied atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FixIt {
    /// Stable machine-readable identifier (e.g. `cy-fix.uppercase`).
    /// Agents pass this back via `rewrite` to apply the fix.
    pub id: SmolStr,
    /// Short human-readable title for the quick-fix UI.
    pub title: SmolStr,
    /// Safety classification for automated application.
    pub applicability: Applicability,
    /// Edits applied as a single atomic transaction.
    pub edits: Vec<TextEdit>,
}

/// A single replacement within a `FixIt`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextEdit {
    /// Byte range to replace.
    pub range: TextRange,
    /// Replacement text.  Empty string deletes the range.
    pub replacement: SmolStr,
}

/// How safe it is to apply a `FixIt` automatically (mirrors rustc's
/// `Applicability` taxonomy).
///
/// Marked `#[non_exhaustive]` (cy-2i9.1) so new classifications can land
/// without forcing a SemVer-major release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Applicability {
    /// Safe to apply without review.
    MachineApplicable,
    /// Likely correct but may miss edge cases — review recommended.
    MaybeIncorrect,
    /// Contains placeholder tokens the user must fill in before it
    /// compiles.
    HasPlaceholders,
    /// Applicability unknown; treat as `MaybeIncorrect`.
    Unspecified,
}

impl Diagnostic {
    /// Construct an error diagnostic with no labels, notes, or fixes.
    /// Use the builder methods (`with_note`, `with_label`, `with_fix`)
    /// to attach extra context.
    #[must_use]
    pub fn error(code: DiagCode, range: TextRange, message: impl Into<SmolStr>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            primary: Label {
                range,
                caption: SmolStr::default(),
            },
            labels: Vec::new(),
            notes: Vec::new(),
            related: Vec::new(),
            fixes: Vec::new(),
        }
    }

    /// Construct a warning diagnostic with no labels, notes, or fixes.
    /// Mirror of `error` for non-error severities.
    #[must_use]
    pub fn warning(code: DiagCode, range: TextRange, message: impl Into<SmolStr>) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            message: message.into(),
            primary: Label {
                range,
                caption: SmolStr::default(),
            },
            labels: Vec::new(),
            notes: Vec::new(),
            related: Vec::new(),
            fixes: Vec::new(),
        }
    }

    /// Append a trailing `note:` line to the diagnostic.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<SmolStr>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Attach a secondary label (context span) to the diagnostic.
    #[must_use]
    pub fn with_label(mut self, range: TextRange, caption: impl Into<SmolStr>) -> Self {
        self.labels.push(Label {
            range,
            caption: caption.into(),
        });
        self
    }

    /// Attach a quick-fix suggestion.  Multiple fixes may be attached;
    /// the client chooses which one to apply.
    #[must_use]
    pub fn with_fix(mut self, fix: FixIt) -> Self {
        self.fixes.push(fix);
        self
    }
}

/// Accumulator. Spec §10.4 — no pass short-circuits on first error.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsSink {
    items: Vec<Diagnostic>,
}

impl DiagnosticsSink {
    /// Construct an empty sink.  Prefer `Default::default()` when a
    /// literal default is expected at the call site.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a diagnostic to the sink.  Order is insertion order
    /// until `into_sorted` normalises.
    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    /// Consume the sink and return a stable-sorted diagnostic vec —
    /// primary range start first, then code.  Used at render time
    /// so the output is deterministic regardless of pass order.
    #[must_use]
    pub fn into_sorted(mut self) -> Vec<Diagnostic> {
        self.items
            .sort_by_key(|d| (d.primary.range.start(), d.code));
        self.items
    }

    /// `true` iff at least one error-severity diagnostic is present.
    /// Used by `cypher check` to set the exit code.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    /// Move every diagnostic out of the sink.  Leaves the sink empty.
    pub fn drain(&mut self) -> impl Iterator<Item = Diagnostic> + '_ {
        self.items.drain(..)
    }

    /// Absorb an iterator of diagnostics into the sink.
    pub fn extend(&mut self, iter: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(iter);
    }

    /// Number of diagnostics currently held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` when the sink holds no diagnostics.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrs_syntax::{TextRange, TextSize};

    #[test]
    fn sort_order_by_offset_then_code() {
        let mut sink = DiagnosticsSink::new();
        sink.push(Diagnostic::error(
            DiagCode::E0001,
            TextRange::new(TextSize::new(5), TextSize::new(6)),
            "b",
        ));
        sink.push(Diagnostic::error(
            DiagCode::E0001,
            TextRange::new(TextSize::new(0), TextSize::new(1)),
            "a",
        ));
        let sorted = sink.into_sorted();
        assert_eq!(sorted[0].message.as_str(), "a");
        assert_eq!(sorted[1].message.as_str(), "b");
    }
}
