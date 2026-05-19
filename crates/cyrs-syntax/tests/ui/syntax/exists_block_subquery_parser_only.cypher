// cy-p1u5: EXISTS { <subquery> } block form — parser-only widening.
//
// Spec §0 amendment 2026-05-19 (cy-5e3f) opened the parser to accept
// the braced block-subquery form so the OpenGQL
// `match_with_exists_predicate_*` samples flip to `Parser accepts?
// yes`. The semantic surface (scope graph, existential semantics)
// remains deferred per spec §20 D1 / N4; the sema dialect-gate pass
// fires E4017 on every occurrence (see
// `crates/cyrs-sema/tests/ui/sema/e4017_exists_subquery_deferred.cypher`).
//
// This fixture pins the parser-level contract: zero CST errors. The
// blessed `.stderr` companion must remain empty — any diagnostic that
// appears here is a regression that re-couples the parser to the
// deferred-feature decision.
MATCH (a) WHERE EXISTS { MATCH (a) RETURN a } RETURN a
