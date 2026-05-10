// E1002: variable re-bound in an outer scope (shadowing).
//
// Note: MATCH (n) MATCH (n) is intentionally pattern-reuse (same var in
// multi-hop patterns) and does NOT emit E1002. E1002 requires a name
// introduced in a new scope that shadows an outer binding — e.g., via
// UNWIND or WITH (both deferred in the lowerer). This corpus case
// documents the zero-diagnostic state; E1002 is covered by the insta
// unit test scope_shadowing_warn in resolve.rs.
//
// When WITH/UNWIND lowering lands, update this file to a case like:
//   MATCH (n) UNWIND [1,2] AS n RETURN n
// and bless.
MATCH (n) RETURN n.name
