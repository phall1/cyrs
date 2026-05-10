//! Shape tests for the `SchemaProvider` surface (spec §8.1, §8.2).

#![forbid(unsafe_code)]

use cyrs_schema::{EmptySchema, PropertyType, SchemaProvider};

#[test]
fn empty_schema_is_schema_provider() {
    let s = EmptySchema;
    assert!(s.labels().is_empty());
    assert!(s.relationship_types().is_empty());
    assert!(!s.has_label("User"));
    assert!(!s.has_relationship_type("KNOWS"));
    assert!(s.node_properties("User").is_none());
    assert!(s.relationship_properties("KNOWS").is_none());
    assert!(s.relationship_endpoints("KNOWS").is_empty());
    assert!(s.inverse_of("KNOWS").is_none());
    assert!(s.function("size").is_none());
    assert!(s.procedure("db.labels").is_none());
}

#[test]
fn empty_schema_digest_is_stable() {
    let a = EmptySchema.schema_digest();
    let b = EmptySchema.schema_digest();
    assert_eq!(a, b, "digest must be deterministic");
}

#[test]
fn trait_is_object_safe() {
    let _s: Box<dyn SchemaProvider> = Box::new(EmptySchema);
}

#[test]
fn property_type_variants_compile() {
    let _ = PropertyType::String;
    let _ = PropertyType::Int;
    let _ = PropertyType::Float;
    let _ = PropertyType::Bool;
    let _ = PropertyType::Date;
    let _ = PropertyType::Datetime;
    let _ = PropertyType::List(Box::new(PropertyType::Int));
    let _ = PropertyType::Enum("Color".into(), vec!["red".into(), "blue".into()]);
    let _ = PropertyType::Opaque("Uuid".into());
}
