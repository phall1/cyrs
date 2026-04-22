// cy-qh8: CASE + IS NULL on an OPTIONAL-MATCH-bound property chain.
// Spec §7.2 — nullability; §19 row "CASE".
//
// The ELSE branch returns `e.address` when it is not null; the THEN
// branch returns the string fallback. Result type = union of arm types;
// in schema-free mode both arms resolve to `Any`/`String` and no
// diagnostic fires.
MATCH (n:Person)
OPTIONAL MATCH (n)-[:HAS_EMAIL]->(e)
RETURN CASE WHEN e.address IS NULL THEN 'no email' ELSE e.address END
