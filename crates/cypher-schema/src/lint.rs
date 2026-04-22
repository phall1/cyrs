//! Schema-file linter (spec 0002 §9).
//!
//! A small pass over an [`InMemorySchema`] that surfaces issues a schema
//! author wants to know about after the file has loaded successfully.
//! Load-time errors (duplicate labels, unknown label references in rel
//! endpoints, malformed type strings) are not repeated here — those are
//! fatal and caught by [`crate::file::load_from_toml_str`]. The linter
//! returns only **post-load** issues that should not block loading but
//! are still worth surfacing.
//!
//! # Checks at v0
//!
//! | Code    | Severity | Check                                         |
//! | ------- | -------- | --------------------------------------------- |
//! | `E3010` | Error    | Property or parameter type is an opaque v0    |
//! |         |          | type (`DURATION`, `POINT`, `MAP`) whose       |
//! |         |          | structural meaning is deferred.               |
//! | `E3011` | Error    | Relationship type has the same label in both  |
//! |         |          | endpoint lists (self-loop).                   |
//! | `W6010` | Warning  | Label declared but not referenced by any      |
//! |         |          | relationship type endpoint.                   |
//!
//! Diagnostic codes are registered in
//! `crates/cypher-diag/src/codes.rs` per AGENTS.md §7. The linter
//! carries the code as a `&'static str` rather than the [`DiagCode`]
//! enum itself so `cypher-schema` can keep its single-edge dependency
//! on `cypher-syntax` (spec 0001 §3.1).
//!
//! [`DiagCode`]: https://docs.rs/cypher-diag

use std::collections::BTreeSet;

use smol_str::SmolStr;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{InMemorySchema, ParamDecl, PropertyDecl, PropertyType, in_memory::RelDecl};

/// Severity assigned to a [`SchemaLint`] issue.
///
/// Mirrors the `E` / `W` letter derived from the diagnostic-code range
/// (spec 0001 §10.2) without forcing a dependency on `cypher-diag`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum LintSeverity {
    /// A fatal issue — callers exit non-zero.
    Error,
    /// A non-fatal advisory — callers log and continue.
    Warning,
}

impl LintSeverity {
    /// Render as lowercase (`error`, `warning`) for text output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// A single issue surfaced by the schema linter.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub struct SchemaLint {
    /// Stable diagnostic code (`E3010`, `W6010`, …). Registered in
    /// `cypher-diag`.
    pub code: &'static str,
    /// Severity classification.
    pub severity: LintSeverity,
    /// Human-readable message. Stable wording for snapshot testing.
    pub message: String,
}

impl SchemaLint {
    fn error(code: &'static str, message: String) -> Self {
        Self {
            code,
            severity: LintSeverity::Error,
            message,
        }
    }

    fn warning(code: &'static str, message: String) -> Self {
        Self {
            code,
            severity: LintSeverity::Warning,
            message,
        }
    }
}

impl core::fmt::Display for SchemaLint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{sev}[{code}]: {msg}",
            sev = self.severity.as_str(),
            code = self.code,
            msg = self.message,
        )
    }
}

/// Run every v0 check against `schema` and return the resulting lint
/// list. Order is deterministic: checks run in registry order, each
/// check emits its findings in sorted-name order.
#[must_use]
pub fn lint(schema: &InMemorySchema) -> Vec<SchemaLint> {
    let mut out = Vec::new();
    lint_opaque_types(schema, &mut out);
    lint_self_referential_rel_types(schema, &mut out);
    lint_unreachable_labels(schema, &mut out);
    out
}

// ============================================================
// E3010 — opaque type
// ============================================================

fn lint_opaque_types(schema: &InMemorySchema, out: &mut Vec<SchemaLint>) {
    for (label, props) in schema.labels_iter() {
        for p in props {
            check_opaque_property(label, p, out);
        }
    }
    for rel in schema.rel_types() {
        for p in &rel.properties {
            check_opaque_relationship_property(rel, p, out);
        }
    }
    for param in schema.parameters() {
        check_opaque_parameter(param, out);
    }
}

fn check_opaque_property(label: &SmolStr, p: &PropertyDecl, out: &mut Vec<SchemaLint>) {
    if let Some(name) = opaque_type_name(&p.ty) {
        out.push(SchemaLint::error(
            "E3010",
            format!(
                "property `{label}.{pname}` has opaque type `{ty}`; \
                 v0 does not yet resolve `{ty}` structurally (spec 0002 §4, §20)",
                pname = p.name,
                ty = name,
            ),
        ));
    }
}

fn check_opaque_relationship_property(rel: &RelDecl, p: &PropertyDecl, out: &mut Vec<SchemaLint>) {
    if let Some(name) = opaque_type_name(&p.ty) {
        out.push(SchemaLint::error(
            "E3010",
            format!(
                "relationship `{rname}` property `{pname}` has opaque type `{ty}`; \
                 v0 does not yet resolve `{ty}` structurally (spec 0002 §4, §20)",
                rname = rel.name,
                pname = p.name,
                ty = name,
            ),
        ));
    }
}

fn check_opaque_parameter(param: &ParamDecl, out: &mut Vec<SchemaLint>) {
    if let Some(name) = opaque_type_name(&param.ty) {
        out.push(SchemaLint::error(
            "E3010",
            format!(
                "parameter `${pname}` has opaque type `{ty}`; \
                 v0 does not yet resolve `{ty}` structurally (spec 0002 §4, §20)",
                pname = param.name,
                ty = name,
            ),
        ));
    }
}

/// Return `Some(name)` when `ty` (or a nested list element) is an opaque
/// type deferred by spec 0002 §4.
fn opaque_type_name(ty: &PropertyType) -> Option<&str> {
    match ty {
        PropertyType::Opaque(n) => Some(n.as_str()),
        PropertyType::List(inner) => opaque_type_name(inner),
        _ => None,
    }
}

// ============================================================
// E3011 — self-referential rel type
// ============================================================

fn lint_self_referential_rel_types(schema: &InMemorySchema, out: &mut Vec<SchemaLint>) {
    for rel in schema.rel_types() {
        let starts: BTreeSet<&SmolStr> = rel.start_labels.iter().collect();
        let ends: BTreeSet<&SmolStr> = rel.end_labels.iter().collect();
        let overlap: Vec<&&SmolStr> = starts.intersection(&ends).collect();
        for label in overlap {
            out.push(SchemaLint::error(
                "E3011",
                format!(
                    "relationship `{rname}` declares `{lname}` on both \
                     `start_labels` and `end_labels`; confirm the self-loop \
                     is intentional (spec 0002 §6)",
                    rname = rel.name,
                    lname = label,
                ),
            ));
        }
    }
}

// ============================================================
// W6010 — unreachable labels
// ============================================================

fn lint_unreachable_labels(schema: &InMemorySchema, out: &mut Vec<SchemaLint>) {
    let mut reached: BTreeSet<SmolStr> = BTreeSet::new();
    for rel in schema.rel_types() {
        for l in rel.start_labels.iter().chain(rel.end_labels.iter()) {
            reached.insert(l.clone());
        }
    }
    for label in schema.label_names() {
        if !reached.contains(&label) {
            out.push(SchemaLint::warning(
                "W6010",
                format!(
                    "label `{label}` is declared but not used by any \
                     relationship type; confirm this is intentional \
                     (spec 0002 §9)",
                ),
            ));
        }
    }
}

// Expose an iterator over `(name, properties)` for the linter — the
// existing `label_names()` + `node_properties()` dance would clone
// twice per label; this is a thin projection over the private
// `labels` map kept behind `pub(crate)`.
impl InMemorySchema {
    /// Iterate labels with their property list in sorted order.
    ///
    /// Intended for internal tools (linter, diff) that already hold a
    /// reference to the schema and want to avoid cloning property
    /// vectors. External callers should prefer
    /// [`crate::SchemaProvider::node_properties`].
    pub(crate) fn labels_iter(&self) -> impl Iterator<Item = (&SmolStr, &Vec<PropertyDecl>)> {
        self.labels.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemorySchema, ParamDecl, PropertyDecl, PropertyType, in_memory::RelDecl};

    #[test]
    fn clean_schema_has_no_lints() {
        let s = InMemorySchema::builder()
            .add_label(
                SmolStr::new("Person"),
                vec![PropertyDecl::new(
                    SmolStr::new("name"),
                    PropertyType::String,
                    true,
                )],
            )
            .add_label(
                SmolStr::new("Movie"),
                vec![PropertyDecl::new(
                    SmolStr::new("title"),
                    PropertyType::String,
                    true,
                )],
            )
            .add_rel_type(RelDecl {
                name: SmolStr::new("ACTED_IN"),
                start_labels: vec![SmolStr::new("Person")],
                end_labels: vec![SmolStr::new("Movie")],
                properties: vec![],
            })
            .build()
            .expect("builds");
        assert!(lint(&s).is_empty());
    }

    #[test]
    fn opaque_property_type_flagged() {
        let s = InMemorySchema::builder()
            .add_label(
                SmolStr::new("Event"),
                vec![PropertyDecl::new(
                    SmolStr::new("at"),
                    PropertyType::Opaque(SmolStr::new("DURATION")),
                    false,
                )],
            )
            .add_rel_type(RelDecl {
                name: SmolStr::new("R"),
                start_labels: vec![SmolStr::new("Event")],
                end_labels: vec![SmolStr::new("Event")],
                properties: vec![],
            })
            .build()
            .expect("builds");
        let issues = lint(&s);
        // Two findings expected: E3010 (DURATION) and E3011 (self-loop).
        let e3010: Vec<_> = issues.iter().filter(|i| i.code == "E3010").collect();
        assert_eq!(e3010.len(), 1);
        assert!(
            e3010[0].message.contains("DURATION"),
            "message: {}",
            e3010[0].message
        );
    }

    #[test]
    fn opaque_parameter_type_flagged() {
        let s = InMemorySchema::builder()
            .add_parameter(ParamDecl {
                name: SmolStr::new("loc"),
                ty: PropertyType::Opaque(SmolStr::new("POINT")),
                default: None,
            })
            .build()
            .expect("builds");
        let issues = lint(&s);
        assert!(issues.iter().any(|i| i.code == "E3010"));
    }

    #[test]
    fn self_loop_rel_type_flagged() {
        let s = InMemorySchema::builder()
            .add_label(SmolStr::new("Team"), vec![])
            .add_rel_type(RelDecl {
                name: SmolStr::new("REPORTS_TO"),
                start_labels: vec![SmolStr::new("Team")],
                end_labels: vec![SmolStr::new("Team")],
                properties: vec![],
            })
            .build()
            .expect("builds");
        let e3011: Vec<_> = lint(&s).into_iter().filter(|i| i.code == "E3011").collect();
        assert_eq!(e3011.len(), 1);
        assert!(e3011[0].message.contains("REPORTS_TO"));
    }

    #[test]
    fn unreachable_label_flagged() {
        let s = InMemorySchema::builder()
            .add_label(SmolStr::new("Connected"), vec![])
            .add_label(SmolStr::new("Orphan"), vec![])
            .add_rel_type(RelDecl {
                name: SmolStr::new("R"),
                start_labels: vec![SmolStr::new("Connected")],
                end_labels: vec![SmolStr::new("Connected")],
                properties: vec![],
            })
            .build()
            .expect("builds");
        let issues = lint(&s);
        let w6010: Vec<_> = issues.iter().filter(|i| i.code == "W6010").collect();
        assert_eq!(w6010.len(), 1);
        assert!(w6010[0].message.contains("Orphan"));
    }

    #[test]
    fn severity_classification() {
        assert_eq!(LintSeverity::Error.as_str(), "error");
        assert_eq!(LintSeverity::Warning.as_str(), "warning");
    }

    #[test]
    fn lint_is_deterministic() {
        let s = InMemorySchema::builder()
            .add_label(SmolStr::new("A"), vec![])
            .add_label(SmolStr::new("B"), vec![])
            .add_label(SmolStr::new("C"), vec![])
            .build()
            .expect("builds");
        assert_eq!(lint(&s), lint(&s));
    }
}
