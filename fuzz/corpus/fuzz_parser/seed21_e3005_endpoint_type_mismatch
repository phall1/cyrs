// E3005: relationship endpoint type mismatch (schema-aware).
//
// Note: E3005 is registered in the code registry (DiagCode::E3005) but the
// endpoint check is not yet implemented in schema_aware.rs (it is documented
// in the module-level table but the emit site is absent — deferred). This
// corpus case documents the zero-diagnostic state.
//
// When endpoint checking lands, update to:
//   MATCH (a:Dog)-[r:KNOWS]->(b:Cat) RETURN r
// with a schema declaring KNOWS: Person → Person, and bless.
MATCH (a:Dog)-[r:KNOWS]->(b:Cat) RETURN r
