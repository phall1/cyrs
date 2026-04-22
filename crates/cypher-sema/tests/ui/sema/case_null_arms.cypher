// cy-41u: CASE with a NULL-producing arm — result type is the union of
// the explicit arm and the null branch (spec §19 row "CASE").
MATCH (n)
RETURN CASE WHEN n.x IS NULL THEN 0 ELSE n.x END AS value
