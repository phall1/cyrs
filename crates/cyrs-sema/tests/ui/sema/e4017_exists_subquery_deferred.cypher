// E4017: EXISTS { ... } block subqueries are deferred per spec §20 D1 / N4.
//
// Bead cy-p1u5 (spec §0 amendment dated 2026-05-19, cy-5e3f) widened
// the parser to accept the braced `EXISTS { <Statement> }` and
// parenthesised `EXISTS ( MATCH ... )` subquery forms so the OpenGQL
// `match_with_exists_predicate_*` samples flip to `Parser accepts?
// yes`. The semantic surface (scope graph, existential semantics)
// remains deferred: HIR lowering produces an inert
// `Expr::ExistsSubqueryDeferred` marker and the sema dialect-gate pass
// fires `DiagCode::E4017` (`exists_subquery` gate) on every
// occurrence.
//
// This fixture is the load-bearing proof that N4 / D1 are still in
// force at the semantic surface even after the parser-only widening.
// Both surface forms must trigger the diagnostic.
MATCH (p) WHERE EXISTS { MATCH (p)-[:R]->(:X) RETURN p } RETURN p
