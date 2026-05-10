// cy-qh8: property chain through an OPTIONAL-MATCH-bound variable with
// no explicit null guard. Schema-free inference types `e.address` as
// `Any`, which admits null silently. No W6xxx style lint for missing
// null-guards exists yet — this fixture locks the accept-with-Null-
// typed-result behaviour; adding a lint is a future spec follow-up.
// Spec §7.2 (nullability), §19.
MATCH (n:Person)
OPTIONAL MATCH (n)-[:HAS_EMAIL]->(e)
RETURN e.address AS address
