// cy-p3cl: GQL §16.4 `labelExpression` — parenthesised sub-expression
// missing its closing `)`.  Surfaces E0102 (`EXPECTED_RPAREN_LABEL`);
// the outer `NODE_PATTERN` close still recovers via E0011.
// ISO/IEC 39075:2024 §16.4.
MATCH (n:(A&B RETURN n
