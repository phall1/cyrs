// E2012: IN list element type mismatch.
//
// Note: triggering E2012 from text input requires the parser to produce
// an IN_EXPR where the list literal can be unambiguously typed, but the
// grammar's precedence binds `[` as a subscript operator before IN can
// consume it. This corpus case documents the zero-diagnostic state.
// E2012 is covered by insta unit tests in infer.rs.
//
// When the parser handles `x IN [literal_list]` unambiguously, update
// this file and bless.
MATCH (n) WHERE n.age IN n.ages RETURN n
