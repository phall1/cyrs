// Formatter demo — this file is valid but poorly spaced.
// Run `cypher fmt needs_fmt.cyp` or `:lua vim.lsp.buf.format()` in nvim
// to normalize whitespace and clause layout.

MATCH (p:Person)-[:KNOWS]->(f:Person)
WHERE p.age>30
RETURN p.name,f.name
LIMIT 10

