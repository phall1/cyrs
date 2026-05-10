// cy-lve: EXISTS(<pattern>) is a pattern predicate — sugar desugared in
// HIR (spec §6.1 / §19 row "Pattern predicates in expressions"). Result
// type is BOOLEAN; sema must accept the form without diagnostics.
MATCH (a) WHERE EXISTS((a)-->(:Movie)) RETURN a
