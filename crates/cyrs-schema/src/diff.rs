//! Schema diff (spec 0002 §9, §12).
//!
//! A pure function producing a stable [`SchemaDiff`] between two
//! [`InMemorySchema`] values. The output is designed to be serialised
//! as JSON for CI "schema-compat" gates: consumer projects run
//! `cypher schema diff old.toml new.toml` and compare the `breaking`
//! list to decide whether the new schema is a drop-in replacement.
//!
//! # What counts as "breaking"
//!
//! At v0 the following changes are classed as breaking:
//!
//! - A label is removed (existing queries that reference the label
//!   break).
//! - A property on an existing label loses `required = true`, changes
//!   type, or is removed entirely.
//! - A relationship type is removed.
//! - A rel type's `start_labels` or `end_labels` shrink (existing
//!   patterns may now be rejected by the semantic pass).
//! - A rel type changes or loses a property (same rules as labels).
//! - A parameter is removed, or changes type, or loses its default
//!   value.
//!
//! Non-breaking changes land in `adds`:
//!
//! - New labels, rel types, parameters.
//! - New **optional** properties on an existing declaration.
//! - A rel type's endpoint lists expanding (more labels allowed).
//! - A parameter gaining a default.
//!
//! The diff is **structural**: it does not know about the semantic
//! content of queries that depend on the schema. A downstream CI gate
//! that wants stronger compatibility should check breakages against
//! its own corpus.

use std::collections::{BTreeMap, BTreeSet};

use smol_str::SmolStr;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{InMemorySchema, ParamDecl, PropertyDecl, PropertyType, in_memory::RelDecl};

// ============================================================
// Public shape
// ============================================================

/// Structured diff between two schemas.
///
/// Iteration order of each field is deterministic (`BTreeMap`-backed
/// internally), so the serialised JSON is suitable for snapshot tests
/// and CI attestation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SchemaDiff {
    /// Additive-only changes: new labels, rels, parameters, and
    /// optional properties.
    pub adds: Vec<DiffEntry>,
    /// Removals of previously-declared items.
    pub removes: Vec<DiffEntry>,
    /// Changes that are structurally backwards-incompatible.
    pub breaking: Vec<DiffEntry>,
}

impl SchemaDiff {
    /// `true` iff every bucket is empty — the two schemas are
    /// semantically equal.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.adds.is_empty() && self.removes.is_empty() && self.breaking.is_empty()
    }

    /// `true` iff there is at least one breaking change.
    #[must_use]
    pub fn has_breaking(&self) -> bool {
        !self.breaking.is_empty()
    }
}

/// One discrete change recorded in a [`SchemaDiff`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DiffEntry {
    /// Kind of change: `"add"`, `"remove"`, `"breaking"`. Matches the
    /// bucket the entry appears in; duplicated on the entry so a flat
    /// serialised stream is self-describing.
    pub kind: String,
    /// Category of item affected: `"label"`, `"rel_type"`, `"parameter"`,
    /// `"label_property"`, `"rel_type_property"`,
    /// `"rel_type_endpoints"`, `"parameter_type"`,
    /// `"parameter_default"`.
    pub category: String,
    /// Stable path identifier (`Person`, `Person.age`, `ACTED_IN.role`,
    /// `$since_year`). Sort key for deterministic output.
    pub path: String,
    /// Human-readable description of the change. Stable wording for
    /// snapshot tests.
    pub detail: String,
}

impl DiffEntry {
    fn add(category: &'static str, path: String, detail: String) -> Self {
        Self {
            kind: "add".to_owned(),
            category: category.to_owned(),
            path,
            detail,
        }
    }

    fn remove(category: &'static str, path: String, detail: String) -> Self {
        Self {
            kind: "remove".to_owned(),
            category: category.to_owned(),
            path,
            detail,
        }
    }

    fn breaking(category: &'static str, path: String, detail: String) -> Self {
        Self {
            kind: "breaking".to_owned(),
            category: category.to_owned(),
            path,
            detail,
        }
    }
}

// ============================================================
// Entry point
// ============================================================

/// Compute the diff between `old` and `new`.
///
/// The function is deterministic and side-effect-free; running it twice
/// on the same inputs always yields identical output (sorted by path).
#[must_use]
pub fn diff(old: &InMemorySchema, new: &InMemorySchema) -> SchemaDiff {
    let mut out = SchemaDiff::default();
    diff_labels(old, new, &mut out);
    diff_rel_types(old, new, &mut out);
    diff_parameters(old, new, &mut out);

    // Sort each bucket by (category, path, detail) for snapshot stability.
    for bucket in [&mut out.adds, &mut out.removes, &mut out.breaking] {
        bucket.sort_by(|a, b| {
            a.category
                .cmp(&b.category)
                .then(a.path.cmp(&b.path))
                .then(a.detail.cmp(&b.detail))
        });
    }
    out
}

// ============================================================
// Labels
// ============================================================

fn diff_labels(old: &InMemorySchema, new: &InMemorySchema, out: &mut SchemaDiff) {
    let old_labels: BTreeMap<SmolStr, Vec<PropertyDecl>> = old
        .label_names()
        .into_iter()
        .map(|n| {
            let props = old.node_properties_internal(&n).unwrap_or_default();
            (n, props)
        })
        .collect();
    let new_labels: BTreeMap<SmolStr, Vec<PropertyDecl>> = new
        .label_names()
        .into_iter()
        .map(|n| {
            let props = new.node_properties_internal(&n).unwrap_or_default();
            (n, props)
        })
        .collect();

    for (name, new_props) in &new_labels {
        if !old_labels.contains_key(name) {
            out.adds.push(DiffEntry::add(
                "label",
                name.to_string(),
                format!("added label `{name}`"),
            ));
        } else if let Some(old_props) = old_labels.get(name) {
            diff_property_sets(
                &format!("{name}"),
                "label_property",
                old_props,
                new_props,
                out,
            );
        }
    }
    for name in old_labels.keys() {
        if !new_labels.contains_key(name) {
            out.breaking.push(DiffEntry::breaking(
                "label",
                name.to_string(),
                format!("removed label `{name}`"),
            ));
            out.removes.push(DiffEntry::remove(
                "label",
                name.to_string(),
                format!("removed label `{name}`"),
            ));
        }
    }
}

// ============================================================
// Rel types
// ============================================================

fn diff_rel_types(old: &InMemorySchema, new: &InMemorySchema, out: &mut SchemaDiff) {
    let old_rels: BTreeMap<SmolStr, &RelDecl> =
        old.rel_types().map(|r| (r.name.clone(), r)).collect();
    let new_rels: BTreeMap<SmolStr, &RelDecl> =
        new.rel_types().map(|r| (r.name.clone(), r)).collect();

    for (name, new_rel) in &new_rels {
        match old_rels.get(name) {
            None => {
                out.adds.push(DiffEntry::add(
                    "rel_type",
                    name.to_string(),
                    format!("added relationship type `{name}`"),
                ));
            }
            Some(old_rel) => {
                diff_endpoint_list(
                    name,
                    "start_labels",
                    &old_rel.start_labels,
                    &new_rel.start_labels,
                    out,
                );
                diff_endpoint_list(
                    name,
                    "end_labels",
                    &old_rel.end_labels,
                    &new_rel.end_labels,
                    out,
                );
                diff_property_sets(
                    &format!("{name}"),
                    "rel_type_property",
                    &old_rel.properties,
                    &new_rel.properties,
                    out,
                );
            }
        }
    }
    for name in old_rels.keys() {
        if !new_rels.contains_key(name) {
            out.breaking.push(DiffEntry::breaking(
                "rel_type",
                name.to_string(),
                format!("removed relationship type `{name}`"),
            ));
            out.removes.push(DiffEntry::remove(
                "rel_type",
                name.to_string(),
                format!("removed relationship type `{name}`"),
            ));
        }
    }
}

fn diff_endpoint_list(
    rel: &SmolStr,
    side: &'static str,
    old: &[SmolStr],
    new: &[SmolStr],
    out: &mut SchemaDiff,
) {
    let old_set: BTreeSet<&SmolStr> = old.iter().collect();
    let new_set: BTreeSet<&SmolStr> = new.iter().collect();
    if old_set == new_set {
        return;
    }
    let added: Vec<&&SmolStr> = new_set.difference(&old_set).collect();
    let removed: Vec<&&SmolStr> = old_set.difference(&new_set).collect();
    let path = format!("{rel}.{side}");
    if !added.is_empty() {
        let names: Vec<String> = added.iter().map(|s| format!("`{s}`")).collect();
        out.adds.push(DiffEntry::add(
            "rel_type_endpoints",
            path.clone(),
            format!(
                "relationship `{rel}` {side} gained {names}",
                names = names.join(", "),
            ),
        ));
    }
    if !removed.is_empty() {
        let names: Vec<String> = removed.iter().map(|s| format!("`{s}`")).collect();
        out.breaking.push(DiffEntry::breaking(
            "rel_type_endpoints",
            path,
            format!(
                "relationship `{rel}` {side} lost {names}",
                names = names.join(", "),
            ),
        ));
    }
}

// ============================================================
// Property sets (labels + rel types share the same logic)
// ============================================================

fn diff_property_sets(
    owner: &str,
    category: &'static str,
    old: &[PropertyDecl],
    new: &[PropertyDecl],
    out: &mut SchemaDiff,
) {
    let old_map: BTreeMap<&SmolStr, &PropertyDecl> = old.iter().map(|p| (&p.name, p)).collect();
    let new_map: BTreeMap<&SmolStr, &PropertyDecl> = new.iter().map(|p| (&p.name, p)).collect();
    for (name, new_prop) in &new_map {
        match old_map.get(name) {
            None => {
                let path = format!("{owner}.{name}");
                if new_prop.required {
                    out.breaking.push(DiffEntry::breaking(
                        category,
                        path,
                        format!(
                            "property `{owner}.{name}` added as required \
                             (existing instances may violate the invariant)",
                        ),
                    ));
                } else {
                    out.adds.push(DiffEntry::add(
                        category,
                        path,
                        format!("property `{owner}.{name}` added as optional"),
                    ));
                }
            }
            Some(old_prop) => {
                let path = format!("{owner}.{name}");
                if old_prop.ty != new_prop.ty {
                    out.breaking.push(DiffEntry::breaking(
                        category,
                        path.clone(),
                        format!(
                            "property `{owner}.{name}` type changed from `{}` to `{}`",
                            render_type(&old_prop.ty),
                            render_type(&new_prop.ty),
                        ),
                    ));
                }
                if old_prop.required && !new_prop.required {
                    out.adds.push(DiffEntry::add(
                        category,
                        path.clone(),
                        format!("property `{owner}.{name}` relaxed from required to optional"),
                    ));
                } else if !old_prop.required && new_prop.required {
                    out.breaking.push(DiffEntry::breaking(
                        category,
                        path,
                        format!("property `{owner}.{name}` tightened from optional to required",),
                    ));
                }
            }
        }
    }
    for name in old_map.keys() {
        if !new_map.contains_key(name) {
            let path = format!("{owner}.{name}");
            out.breaking.push(DiffEntry::breaking(
                category,
                path.clone(),
                format!("property `{owner}.{name}` removed"),
            ));
            out.removes.push(DiffEntry::remove(
                category,
                path,
                format!("property `{owner}.{name}` removed"),
            ));
        }
    }
}

// ============================================================
// Parameters
// ============================================================

fn diff_parameters(old: &InMemorySchema, new: &InMemorySchema, out: &mut SchemaDiff) {
    let old_params: BTreeMap<&SmolStr, &ParamDecl> =
        old.parameters().map(|p| (&p.name, p)).collect();
    let new_params: BTreeMap<&SmolStr, &ParamDecl> =
        new.parameters().map(|p| (&p.name, p)).collect();
    for (name, new_param) in &new_params {
        match old_params.get(name) {
            None => {
                let path = format!("${name}");
                if new_param.default.is_some() {
                    out.adds.push(DiffEntry::add(
                        "parameter",
                        path,
                        format!("parameter `${name}` added with a default"),
                    ));
                } else {
                    // Adding a parameter without a default may break
                    // consumers that don't yet pass it, but v0 leaves
                    // that to the caller — the schema itself gains
                    // expressive power; queries either supply the
                    // parameter or the front-end reports a missing-param
                    // diagnostic. Treat as additive.
                    out.adds.push(DiffEntry::add(
                        "parameter",
                        path,
                        format!("parameter `${name}` added"),
                    ));
                }
            }
            Some(old_param) => {
                let path = format!("${name}");
                if old_param.ty != new_param.ty {
                    out.breaking.push(DiffEntry::breaking(
                        "parameter_type",
                        path.clone(),
                        format!(
                            "parameter `${name}` type changed from `{}` to `{}`",
                            render_type(&old_param.ty),
                            render_type(&new_param.ty),
                        ),
                    ));
                }
                match (&old_param.default, &new_param.default) {
                    (Some(old_default), None) => {
                        out.breaking.push(DiffEntry::breaking(
                            "parameter_default",
                            path.clone(),
                            format!(
                                "parameter `${name}` lost its default value (was `{old_default}`)",
                            ),
                        ));
                    }
                    (None, Some(new_default)) => {
                        out.adds.push(DiffEntry::add(
                            "parameter_default",
                            path.clone(),
                            format!(
                                "parameter `${name}` gained a default value of `{new_default}`",
                            ),
                        ));
                    }
                    (Some(old_default), Some(new_default)) if old_default != new_default => {
                        out.adds.push(DiffEntry::add(
                            "parameter_default",
                            path.clone(),
                            format!(
                                "parameter `${name}` default changed from `{old_default}` to `{new_default}`",
                            ),
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
    for (name, old_param) in &old_params {
        if !new_params.contains_key(name) {
            let path = format!("${name}");
            out.breaking.push(DiffEntry::breaking(
                "parameter",
                path.clone(),
                format!(
                    "parameter `${name}` removed (was `{}`)",
                    render_type(&old_param.ty),
                ),
            ));
            out.removes.push(DiffEntry::remove(
                "parameter",
                path,
                format!("parameter `${name}` removed"),
            ));
        }
    }
}

// ============================================================
// Internal helpers
// ============================================================

/// Render a [`PropertyType`] in the same spelling as the v0 file format
/// (spec 0002 §4). Kept in sync with `file::render_type` — duplicated
/// here so the `file` feature stays optional.
fn render_type(ty: &PropertyType) -> String {
    match ty {
        PropertyType::String => "STRING".to_owned(),
        PropertyType::Int => "INTEGER".to_owned(),
        PropertyType::Float => "FLOAT".to_owned(),
        PropertyType::Bool => "BOOLEAN".to_owned(),
        PropertyType::Date => "DATE".to_owned(),
        PropertyType::Datetime => "DATETIME".to_owned(),
        PropertyType::List(inner) => format!("LIST<{}>", render_type(inner)),
        PropertyType::Opaque(n) | PropertyType::Enum(n, _) => n.to_string(),
        PropertyType::Any => "MAP".to_owned(),
    }
}

// Helper accessor used by the diff without cloning twice. Avoids paying
// the `SchemaProvider` trait's clone-on-return cost twice per label.
impl InMemorySchema {
    fn node_properties_internal(&self, label: &str) -> Option<Vec<PropertyDecl>> {
        self.labels.get(label).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemorySchema, ParamDecl, PropertyDecl, PropertyType, in_memory::RelDecl};

    fn base_schema() -> InMemorySchema {
        InMemorySchema::builder()
            .add_label(
                SmolStr::new("Person"),
                vec![
                    PropertyDecl::new(SmolStr::new("name"), PropertyType::String, true),
                    PropertyDecl::new(SmolStr::new("age"), PropertyType::Int, false),
                ],
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
                properties: vec![PropertyDecl::new(
                    SmolStr::new("role"),
                    PropertyType::String,
                    false,
                )],
            })
            .add_parameter(ParamDecl {
                name: SmolStr::new("since_year"),
                ty: PropertyType::Int,
                default: Some(SmolStr::new("1990")),
            })
            .build()
            .expect("builds")
    }

    #[test]
    fn identical_schemas_diff_empty() {
        let s = base_schema();
        let d = diff(&s, &s);
        assert!(d.is_empty());
    }

    #[test]
    fn added_label_is_non_breaking() {
        let old = base_schema();
        let new = InMemorySchema::builder()
            .add_label(
                SmolStr::new("Person"),
                old.node_properties_internal("Person").unwrap(),
            )
            .add_label(
                SmolStr::new("Movie"),
                old.node_properties_internal("Movie").unwrap(),
            )
            .add_label(SmolStr::new("Director"), vec![])
            .add_rel_type(RelDecl {
                name: SmolStr::new("ACTED_IN"),
                start_labels: vec![SmolStr::new("Person")],
                end_labels: vec![SmolStr::new("Movie")],
                properties: vec![PropertyDecl::new(
                    SmolStr::new("role"),
                    PropertyType::String,
                    false,
                )],
            })
            .add_parameter(ParamDecl {
                name: SmolStr::new("since_year"),
                ty: PropertyType::Int,
                default: Some(SmolStr::new("1990")),
            })
            .build()
            .expect("builds");
        let d = diff(&old, &new);
        assert!(!d.has_breaking());
        assert!(d.adds.iter().any(|e| e.path == "Director"));
    }

    #[test]
    fn removed_label_is_breaking() {
        let old = base_schema();
        let new = InMemorySchema::builder()
            .add_label(
                SmolStr::new("Person"),
                old.node_properties_internal("Person").unwrap(),
            )
            .build()
            .expect("builds");
        let d = diff(&old, &new);
        assert!(d.has_breaking());
        assert!(d.breaking.iter().any(|e| e.path == "Movie"));
        assert!(d.removes.iter().any(|e| e.path == "Movie"));
    }

    #[test]
    fn property_type_change_is_breaking() {
        let old = base_schema();
        let new = InMemorySchema::builder()
            .add_label(
                SmolStr::new("Person"),
                vec![
                    PropertyDecl::new(SmolStr::new("name"), PropertyType::String, true),
                    PropertyDecl::new(SmolStr::new("age"), PropertyType::String, false),
                ],
            )
            .add_label(
                SmolStr::new("Movie"),
                old.node_properties_internal("Movie").unwrap(),
            )
            .build()
            .expect("builds");
        let d = diff(&old, &new);
        assert!(d.breaking.iter().any(|e| e.path == "Person.age"));
    }

    #[test]
    fn rel_type_endpoint_removal_is_breaking() {
        let old = base_schema();
        let new = InMemorySchema::builder()
            .add_label(
                SmolStr::new("Person"),
                old.node_properties_internal("Person").unwrap(),
            )
            .add_label(
                SmolStr::new("Movie"),
                old.node_properties_internal("Movie").unwrap(),
            )
            .add_rel_type(RelDecl {
                name: SmolStr::new("ACTED_IN"),
                start_labels: vec![],
                end_labels: vec![SmolStr::new("Movie")],
                properties: vec![PropertyDecl::new(
                    SmolStr::new("role"),
                    PropertyType::String,
                    false,
                )],
            })
            .build()
            .expect("builds");
        let d = diff(&old, &new);
        assert!(
            d.breaking
                .iter()
                .any(|e| e.category == "rel_type_endpoints" && e.path == "ACTED_IN.start_labels")
        );
    }

    #[test]
    fn parameter_losing_default_is_breaking() {
        let old = base_schema();
        let new = InMemorySchema::builder()
            .add_label(
                SmolStr::new("Person"),
                old.node_properties_internal("Person").unwrap(),
            )
            .add_label(
                SmolStr::new("Movie"),
                old.node_properties_internal("Movie").unwrap(),
            )
            .add_parameter(ParamDecl {
                name: SmolStr::new("since_year"),
                ty: PropertyType::Int,
                default: None,
            })
            .build()
            .expect("builds");
        let d = diff(&old, &new);
        assert!(d.breaking.iter().any(|e| e.category == "parameter_default"));
    }

    #[test]
    fn diff_is_deterministic() {
        let a = base_schema();
        let b = base_schema();
        assert_eq!(diff(&a, &b), diff(&a, &b));
    }
}
