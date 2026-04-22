// cy-41u: simple-when CASE — scrutinee compared against each WHEN value.
// Result type is the union of the arm result types (spec §19 row "CASE").
RETURN CASE 1 WHEN 1 THEN 'one' WHEN 2 THEN 'two' ELSE 'other' END AS label
