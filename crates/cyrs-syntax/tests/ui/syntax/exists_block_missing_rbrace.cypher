// cy-p1u5: EXISTS { ... } block subquery missing closing '}'. Surfaces
// E0091 (`EXPECTED_RBRACE_EXISTS`). The parser still produces a
// recoverable CST so downstream passes can keep running.
RETURN EXISTS { MATCH (a) RETURN a
