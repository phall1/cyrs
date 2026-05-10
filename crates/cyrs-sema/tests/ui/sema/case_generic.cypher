// cy-41u: generic CASE — WHEN <predicate> THEN <expr>, with ELSE branch.
// Result type is the union of the arm types (spec §19 row "CASE").
MATCH (n)
RETURN CASE WHEN n.age > 18 THEN 'adult' ELSE 'minor' END AS label
