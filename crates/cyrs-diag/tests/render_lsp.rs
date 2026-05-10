//! LSP conversion tests (spec §10.3, §4.5).
//! Compiled only with `--features lsp`.

#![cfg(feature = "lsp")]
#![forbid(unsafe_code)]

use core::str::FromStr;

use cyrs_diag::{Applicability, DiagCode, Diagnostic, FixIt, TextEdit, to_lsp};
use cyrs_syntax::{LineIndex, TextRange, TextSize};
use lsp_types::{DiagnosticSeverity, NumberOrString, Uri};

fn rng(a: u32, b: u32) -> TextRange {
    TextRange::new(TextSize::new(a), TextSize::new(b))
}

fn uri() -> Uri {
    Uri::from_str("file:///test.cypher").unwrap()
}

#[test]
fn single_line_error_position_utf8_ascii() {
    let src = "MATCH (n) RETURN n";
    let li = LineIndex::new(src);
    let d = Diagnostic::error(DiagCode::E0001, rng(10, 16), "x");
    let lsp = to_lsp(&d, &uri(), &li);

    assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(lsp.code, Some(NumberOrString::String("E0001".into())));
    assert_eq!(lsp.source.as_deref(), Some("cypher"));
    assert_eq!(lsp.range.start.line, 0);
    assert_eq!(lsp.range.start.character, 10);
    assert_eq!(lsp.range.end.character, 16);
}

#[test]
fn utf16_conversion_at_lsp_boundary() {
    // 'é' is 2 bytes UTF-8 but 1 UTF-16 code unit.
    let src = "éh"; // bytes: [0xC3, 0xA9, 'h'] → 3 bytes, 2 chars
    let li = LineIndex::new(src);
    // Byte range 0..3 covers the whole string.
    let d = Diagnostic::error(DiagCode::E0001, rng(0, 3), "x");
    let lsp = to_lsp(&d, &uri(), &li);

    assert_eq!(lsp.range.start.character, 0);
    // End: 3 UTF-8 bytes → 2 UTF-16 code units.
    assert_eq!(lsp.range.end.character, 2, "UTF-16 at LSP boundary");
}

#[test]
fn severity_maps_correctly() {
    let src = "x";
    let li = LineIndex::new(src);
    let mut d = Diagnostic::warning(DiagCode::W6001, rng(0, 1), "w");
    assert_eq!(
        to_lsp(&d, &uri(), &li).severity,
        Some(DiagnosticSeverity::WARNING)
    );

    d.severity = cyrs_diag::Severity::Note;
    assert_eq!(
        to_lsp(&d, &uri(), &li).severity,
        Some(DiagnosticSeverity::INFORMATION)
    );

    d.severity = cyrs_diag::Severity::Help;
    assert_eq!(
        to_lsp(&d, &uri(), &li).severity,
        Some(DiagnosticSeverity::HINT)
    );
}

#[test]
fn notes_inlined_in_message() {
    let src = "x";
    let li = LineIndex::new(src);
    let mut d = Diagnostic::error(DiagCode::E0001, rng(0, 1), "top-level");
    d.notes.push("consider X".into());
    d.notes.push("consider Y".into());
    let lsp = to_lsp(&d, &uri(), &li);
    assert!(lsp.message.contains("top-level"));
    assert!(lsp.message.contains("note: consider X"));
    assert!(lsp.message.contains("note: consider Y"));
}

#[test]
fn fix_it_is_not_in_diagnostic() {
    // Fix-its travel as CodeActions, not in the Diagnostic itself.
    let src = "x";
    let li = LineIndex::new(src);
    let mut d = Diagnostic::error(DiagCode::E0001, rng(0, 1), "x");
    d.fixes.push(FixIt {
        id: "id".into(),
        title: "title".into(),
        applicability: Applicability::MachineApplicable,
        edits: vec![TextEdit {
            range: rng(0, 1),
            replacement: "y".into(),
        }],
    });
    let lsp = to_lsp(&d, &uri(), &li);
    // Nothing from the fix-it should appear in the LSP Diagnostic.
    assert!(!lsp.message.contains("title"));
}
