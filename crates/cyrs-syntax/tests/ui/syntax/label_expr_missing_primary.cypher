// cy-p3cl: GQL §16.4 `labelExpression` — binary `&` with no right
// operand.  Surfaces E0103 (`EXPECTED_LABEL_EXPR`) at the position
// where the right-hand primary was expected.  The parser still closes
// the conjunction node so downstream recovery is clean.
// ISO/IEC 39075:2024 §16.4.
MATCH (n:A&) RETURN n
