// cy-7lf: regression guard — `(expr)` in expression position must
// still parse as PAREN_EXPR even after the bare pattern-predicate
// dispatch lands. The two-token lookahead past `(` bails to
// paren-expr when the inner tokens start an expression (here
// `INT_LITERAL PLUS`).
RETURN (1 + 2)
