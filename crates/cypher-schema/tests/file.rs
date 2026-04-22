//! Integration tests for `cypher_schema::file` (spec 0002 §10).
//!
//! - Happy path: load a representative schema and check every
//!   declaration is present and properly typed.
//! - Round-trip: parse → serialise → parse yields a semantically
//!   equal schema.
//! - Error paths: unknown label ref, duplicate label, malformed type.

#![cfg(feature = "file")]

use cypher_schema::{
    InMemorySchema, PropertyType, SchemaProvider,
    file::{SchemaLoadError, load_from_toml_str, serialise_to_toml},
};
use smol_str::SmolStr;

const SCHEMA_FIXTURE: &str = r#"
[meta]
cyrs_schema_version = "0.1.0"
schema_name = "test"
description = "round-trip fixture."

[[label]]
name = "Person"
properties = [
    { name = "name",   type = "STRING",  required = true },
    { name = "age",    type = "INTEGER" },
    { name = "active", type = "BOOLEAN" },
]

[[label]]
name = "Movie"
properties = [
    { name = "title",       type = "STRING",  required = true },
    { name = "released",    type = "INTEGER" },
    { name = "genres",      type = "LIST<STRING>" },
]

[[rel_type]]
name = "ACTED_IN"
start_labels = ["Person"]
end_labels   = ["Movie"]
properties = [
    { name = "role", type = "STRING" },
]

[[parameter]]
name    = "since_year"
type    = "INTEGER"
default = 1990

[[parameter]]
name    = "person_name"
type    = "STRING"
default = "anonymous"
"#;

/// Equality that ignores collection ordering (internally `InMemorySchema`
/// already uses `BTreeMap`, but tests that want to assert semantic
/// equality go through this helper for clarity).
fn semantic_eq(a: &InMemorySchema, b: &InMemorySchema) -> bool {
    if a.label_names() != b.label_names() {
        return false;
    }
    if a.rel_type_names() != b.rel_type_names() {
        return false;
    }
    for label in a.label_names() {
        if a.node_properties(&label) != b.node_properties(&label) {
            return false;
        }
    }
    let a_rels: Vec<_> = a.rel_types().collect();
    let b_rels: Vec<_> = b.rel_types().collect();
    if a_rels != b_rels {
        return false;
    }
    let a_params: Vec<_> = a.parameters().collect();
    let b_params: Vec<_> = b.parameters().collect();
    if a_params != b_params {
        return false;
    }
    a.schema_name() == b.schema_name() && a.description() == b.description()
}

#[test]
fn load_reads_every_declaration() {
    let schema = load_from_toml_str(SCHEMA_FIXTURE).expect("loads");

    // Labels.
    assert_eq!(schema.label_count(), 2);
    assert_eq!(
        schema.label_names(),
        vec![SmolStr::new("Movie"), SmolStr::new("Person")],
    );
    let person_props = schema.node_properties("Person").expect("Person exists");
    assert_eq!(person_props.len(), 3);
    assert_eq!(person_props[0].name, SmolStr::new("name"));
    assert_eq!(person_props[0].ty, PropertyType::String);
    assert!(person_props[0].required);
    assert_eq!(person_props[1].ty, PropertyType::Int);
    assert_eq!(person_props[2].ty, PropertyType::Bool);

    let movie_props = schema.node_properties("Movie").expect("Movie exists");
    assert_eq!(movie_props.len(), 3);
    assert_eq!(
        movie_props[2].ty,
        PropertyType::List(Box::new(PropertyType::String)),
    );

    // Rel types.
    assert_eq!(schema.rel_type_count(), 1);
    assert_eq!(schema.relationship_types(), vec![SmolStr::new("ACTED_IN")]);
    let endpoints = schema.relationship_endpoints("ACTED_IN");
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].from, SmolStr::new("Person"));
    assert_eq!(endpoints[0].to, SmolStr::new("Movie"));

    // Parameters.
    assert_eq!(schema.parameter_count(), 2);
    let params: Vec<_> = schema.parameters().collect();
    // BTreeMap-sorted: "person_name" < "since_year".
    assert_eq!(params[0].name, SmolStr::new("person_name"));
    assert_eq!(params[0].ty, PropertyType::String);
    assert_eq!(params[0].default.as_deref(), Some("anonymous"));
    assert_eq!(params[1].name, SmolStr::new("since_year"));
    assert_eq!(params[1].ty, PropertyType::Int);
    assert_eq!(params[1].default.as_deref(), Some("1990"));

    // Meta.
    assert_eq!(schema.schema_name(), Some("test"));
    assert_eq!(schema.description(), Some("round-trip fixture."));
}

#[test]
fn round_trip_is_semantically_stable() {
    let original = load_from_toml_str(SCHEMA_FIXTURE).expect("loads");
    let rendered = serialise_to_toml(&original);
    let reloaded = load_from_toml_str(&rendered).expect("round-trip parses");
    assert!(
        semantic_eq(&original, &reloaded),
        "round-trip schema must be semantically equal\n--- rendered ---\n{rendered}",
    );

    // Render once more to ensure the serialiser is idempotent at the
    // TOML level. (This is stronger than the spec §10 requirement but
    // catches accidental ordering drift cheaply.)
    let rendered_again = serialise_to_toml(&reloaded);
    assert_eq!(rendered, rendered_again);
}

#[test]
fn unknown_label_ref_surfaces_as_typed_error() {
    let input = r#"
[[label]]
name = "Known"

[[rel_type]]
name = "BAD"
start_labels = ["Known"]
end_labels   = ["Stranger"]
"#;
    let err = load_from_toml_str(input).expect_err("should fail");
    match err {
        SchemaLoadError::UnknownLabelRef(n) => {
            assert_eq!(n, SmolStr::new("Stranger"));
        }
        other => panic!("expected UnknownLabelRef, got {other:?}"),
    }
}

#[test]
fn duplicate_label_surfaces_as_typed_error() {
    let input = r#"
[[label]]
name = "Dup"

[[label]]
name = "Dup"
"#;
    let err = load_from_toml_str(input).expect_err("should fail");
    match err {
        SchemaLoadError::DuplicateLabel(n) => {
            assert_eq!(n, SmolStr::new("Dup"));
        }
        other => panic!("expected DuplicateLabel, got {other:?}"),
    }
}

#[test]
fn malformed_type_string_surfaces_as_bad_type() {
    let input = r#"
[[label]]
name = "X"
properties = [ { name = "p", type = "NOT_A_TYPE" } ]
"#;
    let err = load_from_toml_str(input).expect_err("should fail");
    match err {
        SchemaLoadError::BadType(s) => {
            assert!(s.contains("NOT_A_TYPE"), "message should mention type: {s}");
        }
        other => panic!("expected BadType, got {other:?}"),
    }
}

#[test]
fn unsupported_schema_version_rejected() {
    let input = r#"
[meta]
cyrs_schema_version = "99.0.0"
"#;
    let err = load_from_toml_str(input).expect_err("should fail");
    assert!(matches!(err, SchemaLoadError::BadType(_)));
}

#[test]
fn empty_file_loads_to_empty_schema() {
    let schema = load_from_toml_str("").expect("empty file loads");
    assert_eq!(schema.label_count(), 0);
    assert_eq!(schema.rel_type_count(), 0);
    assert_eq!(schema.parameter_count(), 0);
}
