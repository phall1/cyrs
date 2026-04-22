// E3003: property not declared in schema (schema-aware).
//
// Note: `desugar_shorthand_props` lifts inline pattern property maps into
// WHERE predicates (`props.take()` → Clause::Where), so the schema-aware
// pass's `check_node_props` never sees inline props from desugared HIR.
// E3003 fires only on hand-constructed HIR with inline props intact.
// See insta unit test `schema_unknown_node_prop_error` in schema_aware.rs.
//
// This corpus case documents the zero-diagnostic state from desugared text.
// If a future desugaring strategy preserves inline props for schema checks,
// update to: MATCH (p:Person {ghost_prop: 1}) RETURN p  and bless.
MATCH (p:Person) RETURN p
