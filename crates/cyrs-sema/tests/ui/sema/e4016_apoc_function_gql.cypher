// E4016: APOC vendor-prefixed procedure denied in GqlAligned dialect.
//
// Note: E4016 fires when a Clause::Call with an apoc-prefixed procedure name
// is in the HIR. The CALL clause lowering is deferred (spec §20 / lower.rs
// comment). This corpus case documents the zero-diagnostic state from text.
// E4016 is covered by insta unit tests in dialect.rs.
//
// When CALL lowering lands, update this file to:
//   CALL apoc.util.sleep(100) YIELD value RETURN value
// and bless.
MATCH (n) RETURN n
