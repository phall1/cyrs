// cy-lve: EXISTS { <subquery> } block form — deferred per spec §19 / §20 D1.
// The `exists_subquery` dialect gate (E4017) rejects the form; the parser
// recovers by swallowing the balanced braces.
MATCH (a) WHERE EXISTS { MATCH (a) RETURN a } RETURN a
