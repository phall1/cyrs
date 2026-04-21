// Clean query — no diagnostics expected.
// `cypher check good.cyp` exits 0 with no output.

MATCH (p:Person)-[:KNOWS]->(f:Person)
WHERE p.age > 30
RETURN p.name, f.name
LIMIT 10
