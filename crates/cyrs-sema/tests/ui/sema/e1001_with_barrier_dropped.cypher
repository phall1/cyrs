// E1001: variable dropped by WITH barrier.
//
// Note: the WITH clause is handled by the resolver (resolve.rs) but NOT yet
// lowered from text by the HIR lowerer (lower.rs). The input below would
// ideally emit E1001 for `b` after the WITH barrier, but the WITH is silently
// skipped during lowering so `b` remains visible.
//
// When WITH lowering lands, update this file to:
//   MATCH (a) MATCH (b) WITH a RETURN b
// and bless. E1001 from WITH barriers is covered by insta unit tests in
// tests/resolved_hir.rs (snap_12_with_barrier_dropped).
MATCH (a) RETURN z
