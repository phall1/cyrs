//! Plain-text render tests (spec 0001 §10.3).
//!
//! Uses plain string-`contains` assertions rather than `insta` snapshots
//! because `codespan-reporting`'s output is sensitive to terminal width
//! and locale — full-snapshot matching is not portable across CI hosts.
//! We exercise the load-bearing shape (severity tag, code, caret, file
//! label, note text, fix-it `help:` line) and trust the underlying
//! renderer for the rest.

#![forbid(unsafe_code)]

use cyrs_diag::{Applicability, DiagCode, Diagnostic, FixIt, TextEdit, render_text_string};
use cyrs_syntax::{TextRange, TextSize};

fn rng(a: u32, b: u32) -> TextRange {
    TextRange::new(TextSize::new(a), TextSize::new(b))
}

#[test]
fn renders_single_line_error() {
    let src = "MATCH (n) RETURN n";
    let d = Diagnostic::error(DiagCode::E0001, rng(10, 16), "unexpected RETURN");
    let out = render_text_string("q.cypher", src, &d);

    assert!(out.contains("error[E0001]"), "{out}");
    assert!(out.contains("unexpected RETURN"), "{out}");
    assert!(out.contains("q.cypher"), "{out}");
    // Pointer glyph varies across codespan-reporting configurations;
    // accept any of the common ones.
    assert!(
        out.contains('^') || out.contains("-->") || out.contains("\u{2550}"),
        "no pointer glyph in output:\n{out}",
    );
}

#[test]
fn renders_warning_with_note() {
    let src = "RETURN 1";
    let mut d = Diagnostic::warning(DiagCode::W6001, rng(0, 6), "dead WITH");
    d.notes.push("consider removing the projection".into());
    let out = render_text_string("q.cypher", src, &d);
    assert!(out.contains("warning[W6001]"), "{out}");
    assert!(out.contains("dead WITH"), "{out}");
    assert!(out.contains("consider removing the projection"), "{out}");
}

#[test]
fn renders_help_from_fixit() {
    let src = "return 1";
    let mut d = Diagnostic::error(DiagCode::E0001, rng(0, 6), "lowercase keyword");
    d.fixes.push(FixIt {
        id: "cy-fix.uppercase".into(),
        title: "uppercase RETURN".into(),
        applicability: Applicability::MachineApplicable,
        edits: vec![TextEdit {
            range: rng(0, 6),
            replacement: "RETURN".into(),
        }],
    });
    let out = render_text_string("q.cypher", src, &d);
    assert!(out.contains("help: uppercase RETURN"), "{out}");
}

#[test]
fn renders_secondary_label() {
    let src = "MATCH (n) MATCH (n) RETURN n";
    let d = Diagnostic::error(
        DiagCode::E1002,
        rng(17, 18),
        "variable shadows outer binding",
    )
    .with_label(rng(7, 8), "first binding here");
    let out = render_text_string("q.cypher", src, &d);
    assert!(out.contains("error[E1002]"), "{out}");
    assert!(out.contains("first binding here"), "{out}");
}
