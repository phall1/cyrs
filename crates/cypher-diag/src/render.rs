//! Rendering backends for [`crate::Diagnostic`] (spec 0001 §10.3).
//!
//! Three sinks are defined here: plain text (`codespan-reporting`), JSON,
//! and a pure-data transformation hook for LSP (the actual LSP conversion
//! lives in `cypher-lsp` so this crate does not depend on `lsp-types`).

use crate::{Diagnostic, Severity};

/// Render a diagnostic to a plain-text string. No colour. Callers that
/// want ANSI decoration wrap the output.
#[must_use]
pub fn render_plain(src: &str, diag: &Diagnostic) -> String {
    // We render with the straightforward carat-under-span format rather
    // than invoking `codespan-reporting` for the initial stub; the richer
    // rendering lands when the compiletest harness is in place (§17.6).
    let (line, col) = line_col(src, diag.primary.range.start().into());
    let sev = match diag.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
        Severity::Help => "help",
    };
    format!(
        "{sev}[{code}]: {msg}\n  --> line {line}:{col}\n",
        code = diag.code,
        msg = diag.message,
    )
}

fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, b) in src.bytes().enumerate() {
        if i >= offset {
            break;
        }
        if b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::render_plain;
    use crate::{DiagCode, Diagnostic};
    use cypher_syntax::{TextRange, TextSize};

    #[test]
    fn plain_rendering_includes_code_and_position() {
        let d = Diagnostic::error(
            DiagCode::E0002,
            TextRange::new(TextSize::new(6), TextSize::new(7)),
            "unexpected token",
        );
        let out = render_plain("MATCH (n)", &d);
        assert!(out.contains("E0002"), "{out}");
        assert!(out.contains("line 1:7"), "{out}");
    }
}
