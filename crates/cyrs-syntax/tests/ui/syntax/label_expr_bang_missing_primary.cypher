// cy-p3cl: GQL §16.4 `labelExpression` — `!` with no following
// label primary.  Surfaces E0101 (`EXPECTED_LABEL_AFTER_BANG`); the
// parser still emits a `LABEL_NEGATION_EXPR` so the surrounding node
// pattern recovers cleanly.  ISO/IEC 39075:2024 §16.4.
MATCH (n:!&B) RETURN n
