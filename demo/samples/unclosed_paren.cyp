// Parser recovery demo — unclosed node pattern.
// Expected: error[E0011] expected ')' to close node pattern
// The parser recovers and continues, so you also see the rest of the tree.

MATCH (n RETURN n
