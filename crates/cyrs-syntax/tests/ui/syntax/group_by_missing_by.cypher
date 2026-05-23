// cy-71t0: GQL `RETURN ... GROUP BY` clause with the `BY` keyword
// omitted.  Surfaces E0099 (`EXPECTED_BY_AFTER_GROUP`); the parser
// still produces a recoverable CST so the rest of the program is
// analysed.  ISO/IEC 39075:2024 §14.13.3 `groupByClause`.
MATCH (n:Person)
RETURN n.age AS a GROUP a
