//! In-memory [`SchemaProvider`] — a concrete, data-shaped schema the CLI,
//! LSP, and agent all share (spec 0001 §8, spec 0002 §12).
//!
//! [`InMemorySchema`] is the target of the TOML loader in [`crate::file`]
//! and the canonical schema used by tests and the `cypher schema load`
//! subcommand. It is intentionally small — three `BTreeMap`s and a
//! `BTreeMap` of parameter declarations — so that iteration order is
//! deterministic (spec 0001 §17.14).

use std::collections::BTreeMap;

use smol_str::SmolStr;

use crate::{
    Cardinality, EndpointDecl, FunctionSignature, ParamDecl, ProcedureSignature, PropertyDecl,
    SchemaProvider,
};

/// A declared relationship type. The file-format counterpart to
/// [`EndpointDecl`]: one rel type may permit many `(start, end)` label
/// pairings, all at many-to-many at v0 (spec 0002 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelDecl {
    /// Relationship type name (e.g. `KNOWS`).
    pub name: SmolStr,
    /// Allowed source labels. Empty = polymorphic (any label).
    pub start_labels: Vec<SmolStr>,
    /// Allowed target labels. Empty = polymorphic (any label).
    pub end_labels: Vec<SmolStr>,
    /// Properties declared on the relationship.
    pub properties: Vec<PropertyDecl>,
}

/// A concrete, in-memory [`SchemaProvider`].
///
/// Fields are `pub(crate)` — construction goes through
/// [`InMemorySchema::builder`] or the TOML loader in [`crate::file`].
/// This keeps the invariants in one place (unique label / rel type
/// names, deterministic iteration).
#[derive(Debug, Default, Clone)]
pub struct InMemorySchema {
    pub(crate) labels: BTreeMap<SmolStr, Vec<PropertyDecl>>,
    pub(crate) rel_types: BTreeMap<SmolStr, RelDecl>,
    pub(crate) parameters: BTreeMap<SmolStr, ParamDecl>,
    pub(crate) schema_name: Option<SmolStr>,
    pub(crate) description: Option<String>,
}

impl InMemorySchema {
    /// Start a new builder. Use [`InMemorySchemaBuilder::build`] to
    /// finalise.
    #[must_use]
    pub fn builder() -> InMemorySchemaBuilder {
        InMemorySchemaBuilder::default()
    }

    /// All label names, in sorted order.
    #[must_use]
    pub fn label_names(&self) -> Vec<SmolStr> {
        self.labels.keys().cloned().collect()
    }

    /// All relationship type names, in sorted order.
    #[must_use]
    pub fn rel_type_names(&self) -> Vec<SmolStr> {
        self.rel_types.keys().cloned().collect()
    }

    /// All declared rel types, in sorted order.
    pub fn rel_types(&self) -> impl Iterator<Item = &RelDecl> {
        self.rel_types.values()
    }

    /// All declared parameters, in sorted order.
    pub fn parameters(&self) -> impl Iterator<Item = &ParamDecl> {
        self.parameters.values()
    }

    /// Optional human-readable schema name from the `[meta]` block.
    #[must_use]
    pub fn schema_name(&self) -> Option<&str> {
        self.schema_name.as_deref()
    }

    /// Optional human-readable description from the `[meta]` block.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Count of labels.
    #[must_use]
    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    /// Count of rel types.
    #[must_use]
    pub fn rel_type_count(&self) -> usize {
        self.rel_types.len()
    }

    /// Count of parameters.
    #[must_use]
    pub fn parameter_count(&self) -> usize {
        self.parameters.len()
    }
}

impl SchemaProvider for InMemorySchema {
    fn labels(&self) -> Vec<SmolStr> {
        self.label_names()
    }

    fn relationship_types(&self) -> Vec<SmolStr> {
        self.rel_type_names()
    }

    fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>> {
        self.labels.get(label).cloned()
    }

    fn relationship_properties(&self, rel_type: &str) -> Option<Vec<PropertyDecl>> {
        self.rel_types.get(rel_type).map(|r| r.properties.clone())
    }

    fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl> {
        let Some(r) = self.rel_types.get(rel_type) else {
            return Vec::new();
        };
        if r.start_labels.is_empty() || r.end_labels.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(r.start_labels.len() * r.end_labels.len());
        for from in &r.start_labels {
            for to in &r.end_labels {
                out.push(EndpointDecl {
                    from: from.clone(),
                    to: to.clone(),
                    cardinality: Cardinality::ManyToMany,
                });
            }
        }
        out
    }

    fn inverse_of(&self, _rel_type: &str) -> Option<SmolStr> {
        None
    }

    fn function(&self, _name: &str) -> Option<FunctionSignature> {
        None
    }

    fn procedure(&self, _name: &str) -> Option<ProcedureSignature> {
        None
    }

    fn schema_digest(&self) -> [u8; 32] {
        // Deterministic, content-addressed. A simple FNV-1a variant
        // walks the canonical BTreeMap iteration; callers who want
        // cryptographic strength wrap with SHA-256 at their boundary.
        let mut acc: [u8; 32] = [0; 32];
        let mut h = fnv1a_64_init();
        for (name, props) in &self.labels {
            fnv1a_64_str(&mut h, "L:");
            fnv1a_64_str(&mut h, name);
            for p in props {
                fnv1a_64_str(&mut h, "p:");
                fnv1a_64_str(&mut h, &p.name);
                fnv1a_64_str(&mut h, if p.required { "!" } else { "?" });
            }
        }
        for (name, r) in &self.rel_types {
            fnv1a_64_str(&mut h, "R:");
            fnv1a_64_str(&mut h, name);
            for l in &r.start_labels {
                fnv1a_64_str(&mut h, "s:");
                fnv1a_64_str(&mut h, l);
            }
            for l in &r.end_labels {
                fnv1a_64_str(&mut h, "e:");
                fnv1a_64_str(&mut h, l);
            }
            for p in &r.properties {
                fnv1a_64_str(&mut h, "p:");
                fnv1a_64_str(&mut h, &p.name);
            }
        }
        for (name, p) in &self.parameters {
            fnv1a_64_str(&mut h, "P:");
            fnv1a_64_str(&mut h, name);
            if let Some(d) = &p.default {
                fnv1a_64_str(&mut h, "=");
                fnv1a_64_str(&mut h, d);
            }
        }
        // Smear the 64-bit hash across the 32-byte buffer deterministically.
        for (i, byte) in acc.iter_mut().enumerate() {
            let rot = u32::try_from(i).unwrap_or(0).wrapping_mul(7);
            *byte = u8::try_from(h.rotate_left(rot) & 0xff).unwrap_or(0);
        }
        acc
    }
}

fn fnv1a_64_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

fn fnv1a_64_str(h: &mut u64, s: &str) {
    for b in s.as_bytes() {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

// ============================================================
// Builder
// ============================================================

/// Builder for [`InMemorySchema`].
///
/// All mutation goes through this builder so the target struct remains
/// invariant-preserving: unique label / rel type / parameter names,
/// deterministic iteration order (spec 0001 §17.14).
#[derive(Debug, Default)]
pub struct InMemorySchemaBuilder {
    inner: InMemorySchema,
    duplicate_label: Option<SmolStr>,
    duplicate_rel_type: Option<SmolStr>,
    duplicate_parameter: Option<SmolStr>,
}

impl InMemorySchemaBuilder {
    /// Declare a label. The first insertion wins; subsequent
    /// insertions with the same name are recorded and surfaced by
    /// [`InMemorySchemaBuilder::build`].
    #[must_use]
    pub fn add_label(mut self, name: SmolStr, properties: Vec<PropertyDecl>) -> Self {
        if self.inner.labels.contains_key(&name) {
            self.duplicate_label.get_or_insert(name);
            return self;
        }
        self.inner.labels.insert(name, properties);
        self
    }

    /// Declare a relationship type. The first insertion wins;
    /// subsequent insertions are recorded and surfaced by
    /// [`InMemorySchemaBuilder::build`].
    #[must_use]
    pub fn add_rel_type(mut self, rel: RelDecl) -> Self {
        if self.inner.rel_types.contains_key(&rel.name) {
            self.duplicate_rel_type.get_or_insert(rel.name.clone());
            return self;
        }
        self.inner.rel_types.insert(rel.name.clone(), rel);
        self
    }

    /// Declare a query parameter.
    #[must_use]
    pub fn add_parameter(mut self, param: ParamDecl) -> Self {
        if self.inner.parameters.contains_key(&param.name) {
            self.duplicate_parameter.get_or_insert(param.name.clone());
            return self;
        }
        self.inner.parameters.insert(param.name.clone(), param);
        self
    }

    /// Set the schema name from a `[meta]` block.
    #[must_use]
    pub fn schema_name(mut self, name: Option<SmolStr>) -> Self {
        self.inner.schema_name = name;
        self
    }

    /// Set the description from a `[meta]` block.
    #[must_use]
    pub fn description(mut self, desc: Option<String>) -> Self {
        self.inner.description = desc;
        self
    }

    /// Finalise the builder.
    ///
    /// Returns the first duplicate label name encountered (if any),
    /// the first duplicate rel type name, or the built schema.
    /// Reference validation (`rel_type` endpoints referring to declared
    /// labels) lives in the TOML loader, not the builder — the
    /// builder has no view of the original source ordering.
    pub fn build(self) -> Result<InMemorySchema, BuilderError> {
        if let Some(n) = self.duplicate_label {
            return Err(BuilderError::DuplicateLabel(n));
        }
        if let Some(n) = self.duplicate_rel_type {
            return Err(BuilderError::DuplicateRelType(n));
        }
        if let Some(n) = self.duplicate_parameter {
            return Err(BuilderError::DuplicateParameter(n));
        }
        Ok(self.inner)
    }
}

/// Errors produced by [`InMemorySchemaBuilder::build`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // spec 0002 §11 — all three variants are "Duplicate*" by design.
pub enum BuilderError {
    /// Same label name declared twice.
    DuplicateLabel(SmolStr),
    /// Same rel type name declared twice.
    DuplicateRelType(SmolStr),
    /// Same parameter name declared twice.
    DuplicateParameter(SmolStr),
}

impl core::fmt::Display for BuilderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicateLabel(n) => write!(f, "duplicate label `{n}`"),
            Self::DuplicateRelType(n) => write!(f, "duplicate rel type `{n}`"),
            Self::DuplicateParameter(n) => write!(f, "duplicate parameter `{n}`"),
        }
    }
}

impl std::error::Error for BuilderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder_builds_empty_schema() {
        let s = InMemorySchema::builder().build().expect("builds");
        assert_eq!(s.label_count(), 0);
        assert_eq!(s.rel_type_count(), 0);
        assert_eq!(s.parameter_count(), 0);
    }

    #[test]
    fn builder_preserves_sorted_iteration() {
        let s = InMemorySchema::builder()
            .add_label(SmolStr::new("Zebra"), vec![])
            .add_label(SmolStr::new("Apple"), vec![])
            .add_label(SmolStr::new("Mango"), vec![])
            .build()
            .expect("builds");
        assert_eq!(
            s.label_names(),
            vec![
                SmolStr::new("Apple"),
                SmolStr::new("Mango"),
                SmolStr::new("Zebra"),
            ]
        );
    }

    #[test]
    fn duplicate_label_surfaces_as_error() {
        let err = InMemorySchema::builder()
            .add_label(SmolStr::new("X"), vec![])
            .add_label(SmolStr::new("X"), vec![])
            .build()
            .expect_err("duplicate");
        assert_eq!(err, BuilderError::DuplicateLabel(SmolStr::new("X")));
    }

    #[test]
    fn rel_endpoints_cross_product_for_declared_labels() {
        let s = InMemorySchema::builder()
            .add_label(SmolStr::new("A"), vec![])
            .add_label(SmolStr::new("B"), vec![])
            .add_rel_type(RelDecl {
                name: SmolStr::new("R"),
                start_labels: vec![SmolStr::new("A")],
                end_labels: vec![SmolStr::new("A"), SmolStr::new("B")],
                properties: vec![],
            })
            .build()
            .expect("builds");
        let ends = s.relationship_endpoints("R");
        assert_eq!(ends.len(), 2);
        assert!(
            ends.iter()
                .all(|e| e.cardinality == Cardinality::ManyToMany)
        );
    }

    #[test]
    fn rel_endpoints_empty_when_polymorphic() {
        let s = InMemorySchema::builder()
            .add_rel_type(RelDecl {
                name: SmolStr::new("R"),
                start_labels: vec![],
                end_labels: vec![],
                properties: vec![],
            })
            .build()
            .expect("builds");
        assert!(s.relationship_endpoints("R").is_empty());
    }

    #[test]
    fn schema_digest_is_deterministic() {
        let build = || {
            InMemorySchema::builder()
                .add_label(
                    SmolStr::new("Person"),
                    vec![PropertyDecl {
                        name: SmolStr::new("name"),
                        ty: crate::PropertyType::String,
                        required: true,
                    }],
                )
                .build()
                .expect("builds")
        };
        assert_eq!(build().schema_digest(), build().schema_digest());
    }

    #[test]
    fn schema_digest_changes_on_observable_change() {
        let a = InMemorySchema::builder()
            .add_label(SmolStr::new("A"), vec![])
            .build()
            .expect("builds");
        let b = InMemorySchema::builder()
            .add_label(SmolStr::new("B"), vec![])
            .build()
            .expect("builds");
        assert_ne!(a.schema_digest(), b.schema_digest());
    }
}
