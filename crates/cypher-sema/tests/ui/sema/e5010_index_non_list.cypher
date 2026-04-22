// E5010: indexing / slicing a non-list expression is a type error.
// The bead cy-7s6.1 scopes v1 to LIST targets; integer literals,
// strings, booleans, maps, node/rel/path variables, and null are
// provably non-list and caught here. STRING indexing is deferred
// (see docs/specs/0001-cypher-frontend.md §19 / §20).
MATCH (n) RETURN n[0]
