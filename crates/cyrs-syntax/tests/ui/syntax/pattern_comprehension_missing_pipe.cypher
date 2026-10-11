// Pattern comprehension whose `WHERE` commits the parse but which has
// no `|` projection. Surfaces E0106 (`EXPECTED_PIPE_PATTERN_COMP`); the
// rest of the statement still parses.
MATCH (a)
RETURN [(a)-->(b) WHERE b.x > 1]
