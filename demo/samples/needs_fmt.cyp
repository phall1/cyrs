// Formatter demo — this file is valid but poorly spaced.
// Run `cypher fmt needs_fmt.cyp` or `:lua vim.lsp.buf.format()` in nvim
// to normalize whitespace and clause layout.

match   (p:Person)-[:KNOWS]->(f:Person)where p.age>30 return p.name,f.name limit 10
