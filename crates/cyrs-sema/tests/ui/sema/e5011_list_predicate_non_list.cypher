// E5011: the iterable passed to ANY / ALL / NONE / SINGLE must be a
// list. Structural non-list targets (scalar literal, map, Node /
// Relationship / Path variable, pattern predicate) are provably wrong
// and caught here; `Value`-kinded values defer to schema-aware /
// runtime inspection. cy-8x5 (spec §19 row "List predicates").
MATCH (n) RETURN ANY(x IN n WHERE x > 0)
