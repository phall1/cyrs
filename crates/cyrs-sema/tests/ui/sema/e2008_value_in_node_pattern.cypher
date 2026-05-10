// E2008: value variable used where a node binder is expected.
//
// Note: E2008 from text requires WITH or UNWIND to bind a Value variable,
// which the lowerer does not yet support. This corpus case documents the
// zero-diagnostic state; E2008 is covered by insta unit tests in kinds.rs.
//
// When WITH lowering lands, update this file and bless.
MATCH (n) RETURN n.name
