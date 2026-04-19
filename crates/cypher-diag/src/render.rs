//! Rendering backends for [`crate::Diagnostic`] (spec 0001 §10.3).
//!
//! This module hosts the plain-text renderer, implemented on top of
//! [`codespan-reporting`]. JSON and LSP renderers land in separate
//! beads (cy-0a4, cy-8nc) as their own modules.
//!
//! The renderer takes a [`Diagnostic`] together with the source text it
//! was produced against and emits a terminal-friendly rendering:
//! colour-coded severity, the primary span inlined with the source, any
//! secondary labels, free-form notes, and one `help: <title>` line per
//! registered fix-it — the standard `rustc`-style shape (§10.3).
//!
//! `Related` cross-references (spec §10.1) are appended as trailing
//! `note: related: …` lines; `codespan-reporting` has no dedicated slot
//! for cross-file references in v1 (§10.6 defers multi-file UX).

#![forbid(unsafe_code)]

use codespan_reporting::diagnostic::{
    Diagnostic as CrDiagnostic, Label as CrLabel, LabelStyle, Severity as CrSeverity,
};
use codespan_reporting::files::{Error as FilesError, SimpleFile};
use codespan_reporting::term::termcolor::{ColorChoice, NoColor, StandardStream, WriteColor};
use codespan_reporting::term::{self, Config};

use crate::{Diagnostic, Severity};

/// Render a single diagnostic to `writer` using `codespan-reporting`.
///
/// `source_name` is a human-readable label for the source (e.g.
/// `"<cli>"`, `"query.cypher"`, or a file path). `source` is the full
/// source text the diagnostic refers to — its byte offsets must align
/// with the diagnostic's [`cypher_syntax::TextRange`] values.
///
/// # Errors
/// Forwards any error produced by `writer` or by
/// `codespan-reporting`'s emitter. A `Vec`-backed writer cannot fail;
/// see [`render_text_string`] for the infallible variant.
pub fn render_text<W: WriteColor>(
    writer: &mut W,
    source_name: &str,
    source: &str,
    diag: &Diagnostic,
) -> Result<(), FilesError> {
    let file = SimpleFile::new(source_name, source);
    let cr_diag = to_codespan(diag);
    term::emit(writer, &Config::default(), &file, &cr_diag)
}

/// Render a diagnostic to a `String`, stripped of ANSI escapes.
///
/// Uses [`NoColor`] as the backing writer, so the result is portable
/// ASCII suitable for snapshot tests, compile-test golden files, and
/// non-terminal consumers. The underlying writer is a `Vec<u8>`, so
/// this call is infallible.
#[must_use]
pub fn render_text_string(source_name: &str, source: &str, diag: &Diagnostic) -> String {
    let mut buf = NoColor::new(Vec::new());
    render_text(&mut buf, source_name, source, diag).expect("writing to Vec cannot fail");
    String::from_utf8(buf.into_inner()).expect("codespan-reporting output is utf8")
}

/// Render a diagnostic to the process stderr with colour auto-detected.
///
/// # Errors
/// Forwards any error produced while writing to stderr.
pub fn render_text_stderr(
    source_name: &str,
    source: &str,
    diag: &Diagnostic,
) -> Result<(), FilesError> {
    let mut out = StandardStream::stderr(ColorChoice::Auto);
    render_text(&mut out, source_name, source, diag)
}

/// Lower a [`Diagnostic`] into the shape `codespan-reporting` consumes.
///
/// Only the primary label and any secondary labels become
/// `codespan-reporting` labels. Notes, fix-it summaries, and related
/// cross-references are flattened into the terminal `notes` list in a
/// stable order: user notes first, then `help: <title>` per fix-it
/// (§10.3 / §10.5), then `note: related: <message>` per related entry.
fn to_codespan(d: &Diagnostic) -> CrDiagnostic<()> {
    let severity = match d.severity {
        Severity::Error => CrSeverity::Error,
        Severity::Warning => CrSeverity::Warning,
        Severity::Note => CrSeverity::Note,
        Severity::Help => CrSeverity::Help,
    };

    let primary = CrLabel::new(LabelStyle::Primary, (), to_range(d.primary.range))
        .with_message(d.primary.caption.as_str().to_owned());

    let mut labels = Vec::with_capacity(1 + d.labels.len());
    labels.push(primary);
    for l in &d.labels {
        labels.push(
            CrLabel::new(LabelStyle::Secondary, (), to_range(l.range))
                .with_message(l.caption.as_str().to_owned()),
        );
    }

    let mut notes: Vec<String> = d.notes.iter().map(|n| n.as_str().to_owned()).collect();
    for f in &d.fixes {
        notes.push(format!("help: {}", f.title));
    }
    for r in &d.related {
        notes.push(format!("note: related: {}", r.message));
    }

    CrDiagnostic::new(severity)
        .with_code(d.code.to_string())
        .with_message(d.message.as_str().to_owned())
        .with_labels(labels)
        .with_notes(notes)
}

fn to_range(r: cypher_syntax::TextRange) -> std::ops::Range<usize> {
    usize::from(r.start())..usize::from(r.end())
}

#[cfg(test)]
mod tests {
    use super::render_text_string;
    use crate::{DiagCode, Diagnostic};
    use cypher_syntax::{TextRange, TextSize};

    #[test]
    fn plain_rendering_includes_code_and_message() {
        let d = Diagnostic::error(
            DiagCode::E0002,
            TextRange::new(TextSize::new(6), TextSize::new(7)),
            "unexpected token",
        );
        let out = render_text_string("q.cypher", "MATCH (n)", &d);
        assert!(out.contains("error[E0002]"), "{out}");
        assert!(out.contains("unexpected token"), "{out}");
    }
}
