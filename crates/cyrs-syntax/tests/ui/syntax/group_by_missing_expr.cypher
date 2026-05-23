// cy-71t0: GQL `RETURN ... GROUP BY` clause with no expression
// following the keyword pair.  Surfaces E0100
// (`EXPECTED_GROUPBY_EXPR`); the parser still produces a recoverable
// CST.  ISO/IEC 39075:2024 §14.13.3 `groupingElement`.
MATCH (n:Person)
RETURN n.age AS a GROUP BY
