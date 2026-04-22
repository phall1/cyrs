// E3004: property type conflicts with usage context (schema-aware).
//
// Same limitation as E3003: desugar lifts inline props before schema-aware
// checks run. E3004 is covered by insta unit tests in schema_aware.rs
// (snap_schema_node_prop_type_mismatch_error, snap_schema_rel_prop_type_mismatch_error).
//
// This corpus case documents the zero-diagnostic state from desugared text.
MATCH (p:Person) RETURN p
