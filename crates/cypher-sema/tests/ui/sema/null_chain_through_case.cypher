// cy-qh8: `CASE WHEN n.x IS NULL THEN 0 ELSE n.x END` — the fallback is
// Int, the else-branch is the property read (`Any` in schema-free mode).
// Result is the canonical union; no diagnostics expected.
// Spec §7.2, §19 row "CASE".
MATCH (n)
RETURN CASE WHEN n.x IS NULL THEN 0 ELSE n.x END
