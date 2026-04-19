//! `cypher-diag` — diagnostics for the Cypher front-end (spec 0001 §10).
//!
//! Every pass in the pipeline emits into a shared [`Diagnostic`] shape
//! with stable [`DiagCode`] identifiers. Rendering backends (plain text,
//! JSON, LSP) live here so no downstream crate reinvents them.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/cypher-diag/0.0.1")]

pub mod codes;
pub mod render;

pub use codes::DiagCode;

use cypher_syntax::TextRange;
use smol_str::SmolStr;

/// A single diagnostic. Spec §10.1.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    pub message: SmolStr,
    pub primary: Label,
    pub labels: Vec<Label>,
    pub notes: Vec<SmolStr>,
    pub related: Vec<Related>,
    pub fixes: Vec<FixIt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Label {
    pub range: TextRange,
    pub caption: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Related {
    pub range: TextRange,
    pub message: SmolStr,
}

/// A suggested edit. Multiple `edits` are applied atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FixIt {
    pub id: SmolStr,
    pub title: SmolStr,
    pub applicability: Applicability,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextEdit {
    pub range: TextRange,
    pub replacement: SmolStr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Applicability {
    MachineApplicable,
    MaybeIncorrect,
    HasPlaceholders,
    Unspecified,
}

impl Diagnostic {
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

    #[must_use]
    pub fn with_note(mut self, note: impl Into<SmolStr>) -> Self {
        self.notes.push(note.into());
        self
    }

    #[must_use]
    pub fn with_label(mut self, range: TextRange, caption: impl Into<SmolStr>) -> Self {
        self.labels.push(Label {
            range,
            caption: caption.into(),
        });
        self
    }

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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    #[must_use]
    pub fn into_sorted(mut self) -> Vec<Diagnostic> {
        self.items
            .sort_by_key(|d| (d.primary.range.start(), d.code));
        self.items
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn drain(&mut self) -> impl Iterator<Item = Diagnostic> + '_ {
        self.items.drain(..)
    }

    pub fn extend(&mut self, iter: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(iter);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cypher_syntax::{TextRange, TextSize};

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
