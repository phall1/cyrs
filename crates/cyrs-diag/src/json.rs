//! JSON diagnostic renderer (spec 0001 §10.3, §15).
//!
//! `cyrs-agent` serialises diagnostics over JSON-stdio. Field names
//! are **stable** — consumers parse the output. See spec §15 for the
//! full agent protocol envelope; this module only converts a single
//! [`Diagnostic`] into a [`serde_json::Value`].
//!
//! ## Shape
//!
//! Mirrors the [`Diagnostic`] struct laid out in §10.1 one-for-one, with
//! stable field names:
//!
//! ```json
//! {
//!   "code": "E0001",
//!   "severity": "error",
//!   "message": "unexpected token",
//!   "primary": { "range": [10, 16], "caption": "" },
//!   "labels": [ { "range": [0, 5], "caption": "defined here" } ],
//!   "notes": [ "try adding a RETURN clause" ],
//!   "related": [ { "range": [0, 5], "message": "...", "file": null } ],
//!   "fixes": [
//!     {
//!       "id": "cy-fix.uppercase",
//!       "title": "Uppercase keyword",
//!       "applicability": "machine-applicable",
//!       "edits": [ { "range": [0, 5], "replacement": "MATCH" } ]
//!     }
//!   ]
//! }
//! ```
//!
//! Ranges are emitted as a two-element `[start, end]` array (byte
//! offsets, half-open). Compact and unambiguous; consumers never have
//! to parse a nested object for a trivial pair.
//!
//! This module does **not** depend on the optional `serde` feature of
//! the crate. We build [`serde_json::Value`] by hand so the JSON wire
//! shape is independent of whatever `derive(Serialize)` would produce
//! from the Rust field layout — the wire shape is normative and cannot
//! drift when a struct field is renamed.

#![forbid(unsafe_code)]

use serde_json::{Value, json};

use crate::{Applicability, Diagnostic, FixIt, Label, Related, Severity, TextEdit};

/// Render one diagnostic to its stable JSON [`Value`] representation.
///
/// See the module docs for the full shape. Field order inside the
/// returned object is not contractual — consumers parse by name — but
/// `serde_json`'s preservation of insertion order makes the output
/// deterministic for snapshot testing.
#[must_use]
pub fn to_json(d: &Diagnostic) -> Value {
    json!({
        "code": d.code.to_string(),
        "severity": severity_str(d.severity),
        "message": d.message.as_str(),
        "primary": label_json(&d.primary),
        "labels": d.labels.iter().map(label_json).collect::<Vec<_>>(),
        "notes": d.notes.iter().map(|n| Value::from(n.as_str())).collect::<Vec<_>>(),
        "related": d.related.iter().map(related_json).collect::<Vec<_>>(),
        "fixes": d.fixes.iter().map(fixit_json).collect::<Vec<_>>(),
    })
}

/// Render one diagnostic to a compact single-line JSON string.
#[must_use]
pub fn to_json_string(d: &Diagnostic) -> String {
    to_json(d).to_string()
}

/// Render a batch of diagnostics as newline-delimited JSON (one object
/// per line). Used by `cyrs-agent` for streaming emission (§15).
///
/// The output has no trailing newline. An empty slice yields an empty
/// string.
#[must_use]
pub fn to_ndjson(diags: &[Diagnostic]) -> String {
    diags
        .iter()
        .map(|d| to_json(d).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
        Severity::Help => "help",
    }
}

fn applicability_str(a: Applicability) -> &'static str {
    match a {
        Applicability::MachineApplicable => "machine-applicable",
        Applicability::MaybeIncorrect => "maybe-incorrect",
        Applicability::HasPlaceholders => "has-placeholders",
        Applicability::Unspecified => "unspecified",
    }
}

fn label_json(l: &Label) -> Value {
    json!({
        "range": [u32::from(l.range.start()), u32::from(l.range.end())],
        "caption": l.caption.as_str(),
    })
}

fn related_json(r: &Related) -> Value {
    json!({
        "range": [u32::from(r.range.start()), u32::from(r.range.end())],
        "message": r.message.as_str(),
        "file": r.file.as_ref().map(smol_str::SmolStr::as_str),
    })
}

fn fixit_json(f: &FixIt) -> Value {
    json!({
        "id": f.id.as_str(),
        "title": f.title.as_str(),
        "applicability": applicability_str(f.applicability),
        "edits": f.edits.iter().map(edit_json).collect::<Vec<_>>(),
    })
}

fn edit_json(e: &TextEdit) -> Value {
    json!({
        "range": [u32::from(e.range.start()), u32::from(e.range.end())],
        "replacement": e.replacement.as_str(),
    })
}
