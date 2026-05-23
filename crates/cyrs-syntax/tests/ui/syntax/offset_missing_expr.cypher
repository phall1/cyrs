// cy-z0x8: GQL `RETURN ... OFFSET` trailer with no following
// expression.  Surfaces E0105 (`EXPECTED_OFFSET_EXPR`); the parser
// still produces a recoverable CST so the rest of the program is
// analysed.  ISO/IEC 39075:2024 §14.13.7 `offsetClause` —
// `offsetSynonym ::= SKIP | OFFSET`.
MATCH (n:Person)
RETURN n OFFSET
