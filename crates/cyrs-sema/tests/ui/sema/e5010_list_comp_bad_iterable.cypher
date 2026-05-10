// E5010: iterating over a non-list expression in a list comprehension.
// The iterable position requires a list (spec §19); a node variable is
// structurally non-list and is caught by the same class of error that
// rejects `n[0]` (cy-7s6.1). cy-5gh.
MATCH (n) RETURN [x IN n WHERE x > 0]
