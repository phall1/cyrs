//! JSON renderer shape tests (spec 0001 §10.3, §15).
//!
//! Field names are **stable** — `cyrs-agent` consumers parse them by
//! name. We verify the shape here with `serde_json::Value` equality so
//! key order in the implementation is not contractual, only the
//! field names, types, and primitive encodings.

#![forbid(unsafe_code)]

use cyrs_diag::{
    Applicability, DiagCode, Diagnostic, FixIt, Related, Severity, TextEdit, to_json,
    to_json_string, to_ndjson,
};
use cyrs_syntax::{TextRange, TextSize};
use serde_json::{Value, json};

fn rng(a: u32, b: u32) -> TextRange {
    TextRange::new(TextSize::new(a), TextSize::new(b))
}

#[test]
fn minimal_error_shape() {
    let d = Diagnostic::error(DiagCode::E0001, rng(10, 16), "unexpected token");
    let v = to_json(&d);

    assert_eq!(v["code"], Value::from("E0001"));
    assert_eq!(v["severity"], Value::from("error"));
    assert_eq!(v["message"], Value::from("unexpected token"));
    assert_eq!(v["primary"]["range"], json!([10, 16]));
    assert_eq!(v["primary"]["caption"], Value::from(""));
    assert!(v["labels"].as_array().unwrap().is_empty());
    assert!(v["notes"].as_array().unwrap().is_empty());
    assert!(v["related"].as_array().unwrap().is_empty());
    assert!(v["fixes"].as_array().unwrap().is_empty());
}

#[test]
fn all_severity_names_are_lowercase() {
    let mut d = Diagnostic::warning(DiagCode::W6001, rng(0, 3), "dead WITH");
    assert_eq!(to_json(&d)["severity"], Value::from("warning"));

    d.severity = Severity::Note;
    assert_eq!(to_json(&d)["severity"], Value::from("note"));

    d.severity = Severity::Help;
    assert_eq!(to_json(&d)["severity"], Value::from("help"));

    d.severity = Severity::Error;
    assert_eq!(to_json(&d)["severity"], Value::from("error"));
}

#[test]
fn labels_notes_related_roundtrip() {
    let d = Diagnostic::error(DiagCode::E0001, rng(10, 16), "boom")
        .with_label(rng(0, 5), "defined here")
        .with_note("try adding a RETURN clause");
    let mut d = d;
    d.related.push(Related {
        range: rng(2, 4),
        message: "see here".into(),
        file: None,
    });

    let v = to_json(&d);

    let labels = v["labels"].as_array().unwrap();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0]["range"], json!([0, 5]));
    assert_eq!(labels[0]["caption"], Value::from("defined here"));

    let notes = v["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0], Value::from("try adding a RETURN clause"));

    let related = v["related"].as_array().unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0]["range"], json!([2, 4]));
    assert_eq!(related[0]["message"], Value::from("see here"));
    assert_eq!(related[0]["file"], Value::Null);
}

#[test]
fn related_file_roundtrips_when_some() {
    let mut d = Diagnostic::error(DiagCode::E0001, rng(0, 1), "x");
    d.related.push(Related {
        range: rng(0, 1),
        message: "elsewhere".into(),
        file: Some("other.cypher".into()),
    });
    let v = to_json(&d);
    assert_eq!(v["related"][0]["file"], Value::from("other.cypher"));
}

#[test]
fn fixit_shape_and_applicability_kebab_case() {
    let mut d = Diagnostic::error(DiagCode::E0001, rng(0, 3), "x");
    d.fixes.push(FixIt {
        id: "cy-fix.uppercase".into(),
        title: "Uppercase keyword".into(),
        applicability: Applicability::MachineApplicable,
        edits: vec![TextEdit {
            range: rng(0, 5),
            replacement: "MATCH".into(),
        }],
    });
    let v = to_json(&d);

    let fix = &v["fixes"][0];
    assert_eq!(fix["id"], Value::from("cy-fix.uppercase"));
    assert_eq!(fix["title"], Value::from("Uppercase keyword"));
    assert_eq!(fix["applicability"], Value::from("machine-applicable"));

    let edit = &fix["edits"][0];
    assert_eq!(edit["range"], json!([0, 5]));
    assert_eq!(edit["replacement"], Value::from("MATCH"));
}

#[test]
fn all_applicability_values_are_kebab_case() {
    for (variant, expected) in [
        (Applicability::MachineApplicable, "machine-applicable"),
        (Applicability::MaybeIncorrect, "maybe-incorrect"),
        (Applicability::HasPlaceholders, "has-placeholders"),
        (Applicability::Unspecified, "unspecified"),
    ] {
        let mut d = Diagnostic::error(DiagCode::E0001, rng(0, 1), "x");
        d.fixes.push(FixIt {
            id: "id".into(),
            title: "t".into(),
            applicability: variant,
            edits: Vec::new(),
        });
        let v = to_json(&d);
        assert_eq!(
            v["fixes"][0]["applicability"],
            Value::from(expected),
            "variant {variant:?} did not render as {expected}",
        );
    }
}

#[test]
fn range_is_two_element_array() {
    let d = Diagnostic::error(DiagCode::E0001, rng(3, 7), "x");
    let v = to_json(&d);
    let range = v["primary"]["range"].as_array().unwrap();
    assert_eq!(range.len(), 2);
    assert_eq!(range[0], Value::from(3));
    assert_eq!(range[1], Value::from(7));
}

#[test]
fn to_json_string_is_valid_json() {
    let d = Diagnostic::error(DiagCode::E0001, rng(0, 1), "x");
    let s = to_json_string(&d);
    let reparsed: Value = serde_json::from_str(&s).expect("to_json_string must emit valid JSON");
    assert_eq!(reparsed, to_json(&d));
}

#[test]
fn ndjson_joins_with_newlines_no_trailing() {
    let d1 = Diagnostic::error(DiagCode::E0001, rng(0, 1), "a");
    let d2 = Diagnostic::error(DiagCode::E0001, rng(2, 3), "b");
    let s = to_ndjson(&[d1, d2]);
    assert_eq!(s.lines().count(), 2);
    assert!(!s.ends_with('\n'), "ndjson must not end with newline");

    // Every line must be independently parseable.
    for line in s.lines() {
        let _: Value = serde_json::from_str(line).expect("each ndjson line must be valid JSON");
    }
}

#[test]
fn ndjson_empty_slice_is_empty_string() {
    let s = to_ndjson(&[]);
    assert!(s.is_empty());
}
