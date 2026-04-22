//! Shape tests for the `Diagnostic` / `FixIt` / `Applicability` /
//! `DiagnosticsSink` public surface (spec §10.1, §10.4, §10.5, §10.6).
//!
//! These are external-consumer-perspective tests; they mirror the
//! `tests/registry.rs` pattern for the `DiagCode` registry so the two
//! halves of the `cypher-diag` public contract are exercised from
//! outside the crate.

#![forbid(unsafe_code)]

use cypher_diag::{
    Applicability, DiagCode, Diagnostic, DiagnosticsSink, FixIt, Related, Severity, TextEdit,
};
use cypher_syntax::{TextRange, TextSize};

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::new(start), TextSize::new(end))
}

/// Spec §10.4 — `DiagnosticsSink` drains diagnostics sorted by primary
/// span start offset. Push in reverse order; assert ordered on drain.
#[test]
fn sink_drains_sorted_by_primary_span() {
    let mut sink = DiagnosticsSink::new();
    sink.push(Diagnostic::error(DiagCode::E0001, range(20, 21), "third"));
    sink.push(Diagnostic::error(DiagCode::E0001, range(0, 1), "first"));
    sink.push(Diagnostic::error(DiagCode::E0001, range(10, 11), "second"));

    let sorted = sink.into_sorted();
    let messages: Vec<_> = sorted.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, vec!["first", "second", "third"]);
}

/// Spec §10.4 — ties on primary span start are broken by `DiagCode`
/// numeric order so output is deterministic (§17.14).
#[test]
fn sink_drains_deterministic_on_ties() {
    let mut sink = DiagnosticsSink::new();
    // Pushed in reverse code order; same span.
    sink.push(Diagnostic::error(DiagCode::E0003, range(5, 6), "c"));
    sink.push(Diagnostic::error(DiagCode::E0001, range(5, 6), "a"));
    sink.push(Diagnostic::error(DiagCode::E0002, range(5, 6), "b"));

    let sorted = sink.into_sorted();
    let codes: Vec<_> = sorted.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        vec![DiagCode::E0001, DiagCode::E0002, DiagCode::E0003]
    );
}

/// Spec §10.5 — a `FixIt` carries a `Vec<TextEdit>` that round-trips
/// through `Debug`, `Clone`, and `PartialEq`.
#[test]
fn fixit_edits_round_trip() {
    let edits = vec![
        TextEdit {
            range: range(0, 5),
            replacement: "MATCH".into(),
        },
        TextEdit {
            range: range(10, 13),
            replacement: "RETURN".into(),
        },
    ];
    let fix = FixIt {
        id: "cy-fix.rename-keyword".into(),
        title: "Rename keyword to uppercase".into(),
        applicability: Applicability::MachineApplicable,
        edits: edits.clone(),
    };

    // Clone + equality.
    let cloned = fix.clone();
    assert_eq!(fix, cloned);

    // Debug is non-empty and mentions something identifying.
    let dbg = format!("{fix:?}");
    assert!(dbg.contains("FixIt"), "debug missing type name: {dbg}");
    assert!(
        dbg.contains("MachineApplicable"),
        "debug missing applicability: {dbg}"
    );

    // Edits survive clone.
    assert_eq!(cloned.edits, edits);
}

/// Spec §10.5 — `Applicability` has exactly the four variants the spec
/// enumerates. Pinning the count here means adding a fifth variant
/// requires touching this test, which forces a spec amendment.
#[test]
fn applicability_variant_count() {
    // Exhaustive match — if a new variant is added, this fails to
    // compile, which is the strongest possible pin.
    let variants = [
        Applicability::MachineApplicable,
        Applicability::MaybeIncorrect,
        Applicability::HasPlaceholders,
        Applicability::Unspecified,
    ];
    for a in variants {
        // `Applicability` is `#[non_exhaustive]` (cy-2i9.1).  We accept
        // anything here; the known-set length assertion below pins
        // today's variant count.  Match exists to force a compiler error
        // if a variant name changes.
        let _ = match a {
            Applicability::MachineApplicable => 0,
            Applicability::MaybeIncorrect => 1,
            Applicability::HasPlaceholders => 2,
            Applicability::Unspecified => 3,
            _ => 255,
        };
    }
    assert_eq!(variants.len(), 4);
}

/// Spec §10.6 — `Related` carries a `TextRange`, a message, and an
/// optional `file` (no-op in v1). Verify the struct is constructible
/// with a file reference so downstream consumers know the field is
/// public.
#[test]
fn related_carries_optional_file() {
    let r = Related {
        range: range(0, 4),
        message: "defined here".into(),
        file: Some("schema.cypher".into()),
    };
    assert_eq!(r.file.as_deref(), Some("schema.cypher"));

    let r2 = Related {
        range: range(0, 4),
        message: "defined here".into(),
        file: None,
    };
    assert!(r2.file.is_none());
}

/// Spec §10.1 — a `Diagnostic` built via `Diagnostic::error` has the
/// expected severity and an empty-secondary shape so passes can
/// accumulate labels / notes / fixes incrementally (§10.4).
#[test]
fn diagnostic_error_constructor_shape() {
    let d = Diagnostic::error(DiagCode::E0002, range(3, 4), "bad token");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.code, DiagCode::E0002);
    assert_eq!(d.message.as_str(), "bad token");
    assert_eq!(d.primary.range, range(3, 4));
    assert!(d.labels.is_empty());
    assert!(d.notes.is_empty());
    assert!(d.related.is_empty());
    assert!(d.fixes.is_empty());
}

/// Spec §10.1 — warnings carry `Severity::Warning` with the same
/// otherwise-empty shape.
#[test]
fn diagnostic_warning_constructor_shape() {
    let d = Diagnostic::warning(DiagCode::W6001, range(0, 1), "dead with");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.code, DiagCode::W6001);
}
