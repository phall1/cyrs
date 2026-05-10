// cy-7lf: bare pattern predicate `(a)-->(:Movie)` in WHERE position —
// spec §6.1 desugaring / §19 row "Pattern predicates in expressions".
// Sema must accept the form: inferred type is BOOLEAN, the pattern's
// local scope pulls `a` from the outer `MATCH` binder, and no labels
// or properties are asserted.
MATCH (a) WHERE (a)-->(:Movie) RETURN a
