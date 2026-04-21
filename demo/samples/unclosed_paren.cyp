// Parser recovery demo — unclosed node pattern.
// Expected: error[E0011] expected ')' to close node pattern
// The parser recovers and continues lexing the rest of the statement.

MATCH (n:Person
RETURN n
