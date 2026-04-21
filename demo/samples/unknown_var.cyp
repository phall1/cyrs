// Name-resolution demo — `undefined_var` is referenced but never bound.
// Expected: error[E1001] unresolved variable `undefined_var`
//
// This exercises the sema pipeline (parse → HIR → name resolution),
// not just syntax — showing that the semantic layer runs in v1.

MATCH (n:Person)
RETURN n.name, undefined_var
