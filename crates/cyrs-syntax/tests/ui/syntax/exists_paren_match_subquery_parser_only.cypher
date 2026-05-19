// cy-p1u5: EXISTS ( MATCH ... ) parenthesised subquery — parser-only
// widening (ISO/IEC 39075:2024 §14.10).
//
// Companion fixture to `exists_block_subquery_parser_only.cypher`. The
// OpenGQL samples `match_with_exists_predicate_(match_block_statement
// _in_braces|_in_parentheses).gql` use this form, where the body is a
// single MATCH clause inside parens (no `RETURN`). Same parser-only
// contract: zero CST errors here; sema fires E4017 (see
// `crates/cyrs-sema/tests/ui/sema/e4017_exists_subquery_deferred.cypher`).
MATCH (p) WHERE EXISTS ( MATCH (p)-[:R]->(:X) ) RETURN p
