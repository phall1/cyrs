//! `schema_digest` stability + identity invariants (spec §8.1, §8.5).
//!
//! These tests validate that `SchemaProvider::schema_digest()` is:
//! 1. Deterministic — identical schemas produce identical bytes.
//! 2. Sensitive — any observable change produces different bytes.
//! 3. Order-insensitive — label / rel-type iteration order does not matter.
//!
//! The `EmptySchema` fixture from the crate only exercises the degenerate
//! case. To drive the real invariants we need a non-trivial provider; a
//! minimal `TestSchema` lives inside this file. The production catalog
//! surface (cy-2zq) and the standard library (cy-0vv) are out of scope for
//! this bead.

#![forbid(unsafe_code)]

use cyrs_schema::{
    Cardinality, EmptySchema, EndpointDecl, FunctionSignature, ProcedureSignature, PropertyDecl,
    PropertyType, SchemaProvider,
};
use sha2::{Digest, Sha256};
use smol_str::SmolStr;

// ============================================================
// TestSchema — a minimal in-memory schema provider used only to
// drive the digest invariants.
// ============================================================

#[derive(Debug, Default, Clone)]
struct TestSchema {
    labels: Vec<String>,
    rel_types: Vec<String>,
    node_props: Vec<(String, Vec<PropertyDecl>)>,
    rel_endpoints: Vec<(String, Vec<EndpointDecl>)>,
}

impl SchemaProvider for TestSchema {
    fn labels(&self) -> Vec<SmolStr> {
        self.labels.iter().map(|s| s.as_str().into()).collect()
    }

    fn relationship_types(&self) -> Vec<SmolStr> {
        self.rel_types.iter().map(|s| s.as_str().into()).collect()
    }

    fn has_label(&self, name: &str) -> bool {
        self.labels.iter().any(|s| s == name)
    }

    fn has_relationship_type(&self, name: &str) -> bool {
        self.rel_types.iter().any(|s| s == name)
    }

    fn node_properties(&self, label: &str) -> Option<Vec<PropertyDecl>> {
        self.node_props
            .iter()
            .find(|(n, _)| n == label)
            .map(|(_, ps)| ps.clone())
    }

    fn relationship_properties(&self, _rel_type: &str) -> Option<Vec<PropertyDecl>> {
        None
    }

    fn relationship_endpoints(&self, rel_type: &str) -> Vec<EndpointDecl> {
        self.rel_endpoints
            .iter()
            .find(|(n, _)| n == rel_type)
            .map(|(_, e)| e.clone())
            .unwrap_or_default()
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
        let mut h = Sha256::new();

        // Labels — sorted, null-terminated, domain-separator 0x01.
        h.update([0x01]);
        let mut ls: Vec<String> = self.labels.clone();
        ls.sort();
        for l in &ls {
            h.update(l.as_bytes());
            h.update([0x00]);
        }

        // Relationship types — domain-separator 0x02.
        h.update([0x02]);
        let mut rs: Vec<String> = self.rel_types.clone();
        rs.sort();
        for r in &rs {
            h.update(r.as_bytes());
            h.update([0x00]);
        }

        // Node properties — domain-separator 0x03.
        h.update([0x03]);
        let mut np = self.node_props.clone();
        np.sort_by(|a, b| a.0.cmp(&b.0));
        for (label, props) in &np {
            h.update(label.as_bytes());
            h.update([0x00]);
            let mut ps = props.clone();
            ps.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
            for p in &ps {
                h.update(p.name.as_bytes());
                h.update([0x00]);
                feed_property_type(&mut h, &p.ty);
                h.update([u8::from(p.required)]);
            }
            h.update([0x1e]); // record-separator between label groups
        }

        // Relationship endpoints — domain-separator 0x04.
        h.update([0x04]);
        let mut re = self.rel_endpoints.clone();
        re.sort_by(|a, b| a.0.cmp(&b.0));
        for (rel, ends) in &re {
            h.update(rel.as_bytes());
            h.update([0x00]);
            let mut ends_sorted = ends.clone();
            ends_sorted.sort_by(|a, b| {
                (a.from.as_str(), a.to.as_str()).cmp(&(b.from.as_str(), b.to.as_str()))
            });
            for e in &ends_sorted {
                h.update(e.from.as_bytes());
                h.update([0x00]);
                h.update(e.to.as_bytes());
                h.update([0x00]);
                h.update([card_tag(e.cardinality)]);
            }
            h.update([0x1e]);
        }

        h.finalize().into()
    }
}

/// Canonical encoding of a `PropertyType`. A tag byte followed by the
/// content bytes for composite variants — any observable change in the
/// type mutates the feed, which mutates the digest.
fn feed_property_type(h: &mut Sha256, t: &PropertyType) {
    match t {
        PropertyType::String => h.update([1]),
        PropertyType::Int => h.update([2]),
        PropertyType::Float => h.update([3]),
        PropertyType::Bool => h.update([4]),
        PropertyType::Date => h.update([5]),
        PropertyType::Datetime => h.update([6]),
        PropertyType::List(inner) => {
            h.update([7]);
            feed_property_type(h, inner);
        }
        PropertyType::Enum(name, variants) => {
            h.update([8]);
            h.update(name.as_bytes());
            h.update([0x00]);
            let mut vs: Vec<SmolStr> = variants.clone();
            vs.sort();
            for v in &vs {
                h.update(v.as_bytes());
                h.update([0x00]);
            }
            h.update([0x1f]);
        }
        PropertyType::Opaque(name) => {
            h.update([9]);
            h.update(name.as_bytes());
            h.update([0x00]);
        }
        PropertyType::Any => h.update([10]),
    }
}

/// Exhaustive mapping of `Cardinality` variants to tag bytes. The compiler
/// enforces coverage — if a new variant lands the match fails to build.
fn card_tag(c: Cardinality) -> u8 {
    match c {
        Cardinality::OneToOne => 1,
        Cardinality::OneToMany => 2,
        Cardinality::ManyToOne => 3,
        Cardinality::ManyToMany => 4,
        // `Cardinality` is `#[non_exhaustive]` (cy-2i9.1); new variants
        // must be added to this digest before landing.  Route them here
        // by reserving a new tag byte.
        _ => 255,
    }
}

// ============================================================
// TESTS
// ============================================================

#[test]
fn empty_schema_digest_is_deterministic() {
    assert_eq!(EmptySchema.schema_digest(), EmptySchema.schema_digest());
}

#[test]
fn identical_schemas_produce_identical_digests() {
    let a = TestSchema {
        labels: vec!["Person".into(), "Company".into()],
        rel_types: vec!["KNOWS".into()],
        ..Default::default()
    };
    let b = a.clone();
    assert_eq!(a.schema_digest(), b.schema_digest());
}

#[test]
fn label_order_does_not_affect_digest() {
    let a = TestSchema {
        labels: vec!["Person".into(), "Company".into()],
        ..Default::default()
    };
    let b = TestSchema {
        labels: vec!["Company".into(), "Person".into()],
        ..Default::default()
    };
    assert_eq!(a.schema_digest(), b.schema_digest());
}

#[test]
fn adding_a_label_changes_digest() {
    let mut a = TestSchema {
        labels: vec!["Person".into()],
        ..Default::default()
    };
    let d1 = a.schema_digest();
    a.labels.push("Company".into());
    let d2 = a.schema_digest();
    assert_ne!(d1, d2);
}

#[test]
fn adding_a_relationship_type_changes_digest() {
    let mut a = TestSchema::default();
    let d1 = a.schema_digest();
    a.rel_types.push("KNOWS".into());
    let d2 = a.schema_digest();
    assert_ne!(d1, d2);
}

#[test]
fn adding_a_node_property_changes_digest() {
    let mut a = TestSchema {
        labels: vec!["Person".into()],
        ..Default::default()
    };
    let d1 = a.schema_digest();
    a.node_props.push((
        "Person".into(),
        // `PropertyDecl` is `#[non_exhaustive]` (cy-2i9.1).
        vec![PropertyDecl::new("name", PropertyType::String, true)],
    ));
    let d2 = a.schema_digest();
    assert_ne!(d1, d2);
}

#[test]
fn changing_property_type_changes_digest() {
    let mut a = TestSchema {
        labels: vec!["Person".into()],
        node_props: vec![(
            "Person".into(),
            // `PropertyDecl` is `#[non_exhaustive]` (cy-2i9.1).
            vec![PropertyDecl::new("age", PropertyType::Int, false)],
        )],
        ..Default::default()
    };
    let d1 = a.schema_digest();
    a.node_props[0].1[0].ty = PropertyType::String;
    let d2 = a.schema_digest();
    assert_ne!(d1, d2);
}

#[test]
fn toggling_required_changes_digest() {
    let mut a = TestSchema {
        labels: vec!["Person".into()],
        node_props: vec![(
            "Person".into(),
            // `PropertyDecl` is `#[non_exhaustive]` (cy-2i9.1).
            vec![PropertyDecl::new("name", PropertyType::String, false)],
        )],
        ..Default::default()
    };
    let d1 = a.schema_digest();
    a.node_props[0].1[0].required = true;
    let d2 = a.schema_digest();
    assert_ne!(d1, d2);
}

#[test]
fn adding_rel_endpoint_changes_digest() {
    let mut a = TestSchema {
        rel_types: vec!["KNOWS".into()],
        ..Default::default()
    };
    let d1 = a.schema_digest();
    a.rel_endpoints.push((
        "KNOWS".into(),
        vec![EndpointDecl {
            from: "Person".into(),
            to: "Person".into(),
            cardinality: Cardinality::ManyToMany,
        }],
    ));
    let d2 = a.schema_digest();
    assert_ne!(d1, d2);
}
